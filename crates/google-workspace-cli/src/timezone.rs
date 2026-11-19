// Copyright 2026 Google LLC
// Modifications Copyright 2026 Cru
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Account timezone resolution for Google Workspace CLI.
//!
//! Resolves the authenticated user's timezone with the following priority:
//! 1. Explicit `--timezone` CLI flag (hard error if invalid)
//! 2. Cached value from config dir (24h TTL)
//! 3. Google Calendar Settings API (`users/me/settings/timezone`)
//! 4. Machine-local timezone (fallback with warning)

use crate::auth::MaybeBearerAuth;
use crate::error::GwsError;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use std::path::PathBuf;

/// Cache filename stored in the gws config directory.
const CACHE_FILENAME: &str = "account_timezone";

/// Cache TTL in seconds (24 hours).
const CACHE_TTL_SECS: u64 = 86400;

/// Returns the path to the timezone cache file.
fn cache_path() -> PathBuf {
    crate::auth_commands::config_dir().join(CACHE_FILENAME)
}

/// Remove the cached timezone file. Called on auth login/logout to
/// invalidate stale values when the account changes.
pub fn invalidate_cache() {
    let path = cache_path();
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), error = %e, "failed to invalidate timezone cache");
        }
    }
}

/// Read the cached timezone if it exists and is fresh (< 24h old).
fn read_cache() -> Option<TimeZone> {
    let path = cache_path();
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = std::time::SystemTime::now().duration_since(modified).ok()?;
    if age.as_secs() > CACHE_TTL_SECS {
        return None;
    }
    let contents = std::fs::read_to_string(&path).ok()?;
    let tz_name = contents.trim();
    TimeZone::get(tz_name).ok()
}

/// Write a timezone name to the cache file.
fn write_cache(tz_name: &str) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(path = %parent.display(), error = %e, "failed to create timezone cache directory");
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, tz_name) {
        tracing::warn!(path = %path.display(), error = %e, "failed to write timezone cache");
    }
}

/// Fetch the account timezone from the Google Calendar Settings API.
async fn fetch_account_timezone(
    client: &reqwest::Client,
    token: &str,
) -> Result<TimeZone, GwsError> {
    let base = crate::api_base::api_base(crate::api_base::Api::Calendar).await?;
    let url = format!("{base}users/me/settings/timezone");
    let resp = client
        .get(&url)
        .maybe_bearer_auth(token)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch account timezone: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "timezone_fetch_failed".to_string(),
            enable_url: None,
        });
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse timezone response: {e}")))?;

    let tz_name = json
        .get("value")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            GwsError::Other(anyhow::anyhow!(
                "Timezone setting missing or empty 'value' field"
            ))
        })?;

    let tz = TimeZone::get(tz_name).map_err(|_| {
        GwsError::Other(anyhow::anyhow!(
            "Google returned unrecognized timezone: {tz_name}"
        ))
    })?;

    // Cache for future use
    write_cache(tz_name);
    tracing::info!(
        timezone = tz_name,
        source = "calendar_api",
        "resolved account timezone"
    );

    Ok(tz)
}

/// Parse an explicit timezone string, returning an error if invalid.
pub fn parse_timezone(tz_str: &str) -> Result<TimeZone, GwsError> {
    TimeZone::get(tz_str).map_err(|_| {
        GwsError::Validation(format!(
            "Invalid timezone '{tz_str}'. Use an IANA timezone name (e.g. America/Denver, Europe/London, UTC)."
        ))
    })
}

/// Resolve the user's timezone with this priority:
/// 1. `tz_override` (from `--timezone` flag) — hard error if invalid
/// 2. Cached value in config dir — use if < 24h old
/// 3. Google Calendar Settings API — fetch and cache
/// 4. Machine-local timezone (log warning)
pub async fn resolve_account_timezone(
    client: &reqwest::Client,
    token: &str,
    tz_override: Option<&str>,
) -> Result<TimeZone, GwsError> {
    // 1. Explicit override — fail if invalid
    if let Some(tz_str) = tz_override {
        let tz = parse_timezone(tz_str)?;
        tracing::info!(
            timezone = tz_str,
            source = "cli_flag",
            "using explicit timezone"
        );
        return Ok(tz);
    }

    // 2. Check cache
    if let Some(tz) = read_cache() {
        tracing::debug!(
            timezone = tz.iana_name().unwrap_or_default(),
            source = "cache",
            "using cached timezone"
        );
        return Ok(tz);
    }

    // 3. Fetch from Calendar Settings API
    match fetch_account_timezone(client, token).await {
        Ok(tz) => return Ok(tz),
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch account timezone, falling back to local");
        }
    }

    // 4. Fall back to machine-local timezone (TZ env var, then OS setting)
    let tz = TimeZone::system();
    tracing::warn!(
        timezone = describe_zone(&tz, Timestamp::now()),
        source = "local_machine",
        "using machine-local timezone as fallback"
    );
    Ok(tz)
}

/// Name a zone for logs. Unnamed zones (copied /etc/localtime, POSIX `TZ`
/// strings) show their offset at `at` instead.
fn describe_zone(tz: &TimeZone, at: Timestamp) -> String {
    match tz.iana_name() {
        Some(name) => name.to_string(),
        None => format!(
            "<unnamed, now {}>",
            at.to_zoned_datetime(tz.clone()).strftime("%:z")
        ),
    }
}

/// Current time in `tz`.
pub fn now_in(tz: &TimeZone) -> ZonedDateTime {
    Timestamp::now().to_zoned_datetime(tz.clone())
}

/// Start of today (first instant of the local day) in `tz`. If a DST
/// change skips midnight, this is the first valid time after it.
pub fn start_of_today(tz: &TimeZone) -> Result<ZonedDateTime, GwsError> {
    now_in(tz).start_of_day().map_err(|e| {
        GwsError::Other(anyhow::anyhow!(
            "Could not determine start of day in timezone '{}': {e}",
            tz.iana_name().unwrap_or("<unnamed>")
        ))
    })
}

/// Add `days` calendar days (not 24h blocks), so midnight stays midnight
/// across DST changes.
pub fn add_days(zdt: &ZonedDateTime, days: i64) -> Result<ZonedDateTime, GwsError> {
    jiff::Span::new()
        .try_days(days)
        .and_then(|span| zdt.checked_add(span))
        .map_err(|e| GwsError::Validation(format!("Date out of range adding {days} days: {e}")))
}

/// Format as RFC 3339 with the local offset (e.g. `2026-03-08T00:00:00-07:00`),
/// which is what the Calendar API expects for `timeMin`/`timeMax`.
pub fn to_rfc3339(zdt: &ZonedDateTime) -> String {
    zdt.timestamp()
        .display_with_offset(zdt.offset())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_iana_timezone() {
        let tz = parse_timezone("America/Denver").unwrap();
        assert_eq!(tz.iana_name(), Some("America/Denver"));
    }

    #[test]
    fn parse_utc_timezone() {
        let tz = parse_timezone("UTC").unwrap();
        assert_eq!(tz.iana_name(), Some("UTC"));
    }

    #[test]
    fn parse_invalid_timezone_fails() {
        let result = parse_timezone("Not/A/Zone");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid timezone"));
        assert!(err.contains("Not/A/Zone"));
    }

    #[test]
    fn parse_rejects_paths_and_posix_strings() {
        for bad in [
            "../../../../etc/passwd",
            "/etc/localtime",
            "EST5EDT,M3.2.0,M11.1.0",
        ] {
            assert!(parse_timezone(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn parse_empty_string_fails() {
        let result = parse_timezone("");
        assert!(result.is_err());
    }

    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_file = dir.path().join(CACHE_FILENAME);

        // Write directly to test location
        std::fs::write(&cache_file, "America/New_York").unwrap();
        let contents = std::fs::read_to_string(&cache_file).unwrap();
        let tz = TimeZone::get(contents.trim()).unwrap();
        assert_eq!(tz.iana_name(), Some("America/New_York"));
    }

    #[test]
    fn describe_zone_uses_iana_name() {
        let tz = TimeZone::get("America/Denver").unwrap();
        assert_eq!(describe_zone(&tz, Timestamp::UNIX_EPOCH), "America/Denver");
    }

    #[test]
    fn describe_zone_shows_offset_when_unnamed() {
        let tz = TimeZone::posix("MST7MDT,M3.2.0,M11.1.0").unwrap();
        assert_eq!(tz.iana_name(), None);
        let winter: Timestamp = "2026-01-15T12:00:00Z".parse().unwrap();
        let summer: Timestamp = "2026-07-15T12:00:00Z".parse().unwrap();
        assert_eq!(describe_zone(&tz, winter), "<unnamed, now -07:00>");
        assert_eq!(describe_zone(&tz, summer), "<unnamed, now -06:00>");
    }

    #[test]
    fn start_of_day_plus_one_day_spans_dst_change() {
        // 2026-03-08 is the US spring-forward day: that local day is 23h long.
        let tz = TimeZone::get("America/Denver").unwrap();
        let noon: Timestamp = "2026-03-08T19:00:00Z".parse().unwrap();
        let start = noon.to_zoned_datetime(tz).start_of_day().unwrap();
        let end = add_days(&start, 1).unwrap();
        assert_eq!(to_rfc3339(&start), "2026-03-08T00:00:00-07:00");
        assert_eq!(to_rfc3339(&end), "2026-03-09T00:00:00-06:00");
    }

    #[test]
    fn add_days_rejects_out_of_range() {
        let zdt = Timestamp::UNIX_EPOCH.to_zoned_datetime(TimeZone::UTC);
        assert!(add_days(&zdt, i64::MAX).is_err());
        assert!(add_days(&zdt, 10_000_000).is_err());
    }

    #[test]
    fn to_rfc3339_has_no_zone_annotation() {
        let tz = TimeZone::get("America/Denver").unwrap();
        let zdt = Timestamp::UNIX_EPOCH.to_zoned_datetime(tz);
        assert_eq!(to_rfc3339(&zdt), "1969-12-31T17:00:00-07:00");
    }
}

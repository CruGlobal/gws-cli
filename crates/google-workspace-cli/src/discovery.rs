// Copyright 2026 Google LLC
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

//! Discovery Document types and fetching.
//!
//! Types are re-exported from the `google_workspace` library crate.
//! The CLI wrapper provides default caching via `config_dir()`.

pub use google_workspace::discovery::*;

/// Env var overriding where discovery documents (and thus all API traffic, since
/// the CLI is discovery-driven) are fetched from. When set, discovery is fetched
/// from `{base}/discovery/v1/apis/{service}/{version}/rest` instead of Google.
const ENV_DISCOVERY_BASE_URL: &str = "GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL";

/// Fetches and caches a Google Discovery Document using the CLI's config directory.
///
/// This is a convenience wrapper around
/// [`google_workspace::discovery::fetch_discovery_document`] that automatically
/// uses the CLI's cache directory (`~/.config/gws/cache/`) and honors the
/// `GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL` proxy override.
pub async fn fetch_discovery_document(
    service: &str,
    version: &str,
) -> anyhow::Result<RestDescription> {
    let cache_dir = crate::auth_commands::config_dir().join("cache");
    let base_url = base_url_override();
    google_workspace::discovery::fetch_discovery_document(
        service,
        version,
        Some(&cache_dir),
        base_url.as_deref(),
    )
    .await
}

/// Warning for a discovery proxy used without `GOOGLE_WORKSPACE_CLI_AUTH=none`:
/// gws then sends its own bearer tokens to the proxy.
pub fn proxy_credentials_warning(base_url: Option<&str>, no_auth: bool) -> Option<String> {
    let base_url = base_url.filter(|_| !no_auth)?;
    Some(format!(
        "Warning: {ENV_DISCOVERY_BASE_URL} is set without GOOGLE_WORKSPACE_CLI_AUTH=none; \
         Google access tokens will be sent to {}",
        redact_url(base_url)
    ))
}

/// The `GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL` value, if set and non-blank.
pub fn base_url_override() -> Option<String> {
    std::env::var(ENV_DISCOVERY_BASE_URL)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_credentials_warning_only_when_tokens_reach_proxy() {
        let warning = proxy_credentials_warning(Some("http://127.0.0.1:8080"), false).unwrap();
        assert!(warning.contains("http://127.0.0.1:8080"));
        assert!(proxy_credentials_warning(Some("http://127.0.0.1:8080"), true).is_none());
        assert!(proxy_credentials_warning(None, false).is_none());
    }

    #[test]
    fn proxy_credentials_warning_redacts_url() {
        let warning =
            proxy_credentials_warning(Some("https://u:pw@proxy.test/b?key=k1"), false).unwrap();
        assert!(warning.contains("https://proxy.test/b"), "{warning}");
        assert!(
            !warning.contains("pw") && !warning.contains("k1"),
            "{warning}"
        );
    }

    async fn fetch_err(base: &str) -> String {
        google_workspace::discovery::fetch_discovery_document("drive", "v3", None, Some(base))
            .await
            .unwrap_err()
            .to_string()
    }

    #[tokio::test]
    async fn fetch_errors_redact_override() {
        // Connection refused: reqwest's own error would echo the query.
        let err = fetch_err("http://u:pw@127.0.0.1:9/p?key=k1").await;
        assert!(err.contains("http://127.0.0.1:9/p"), "{err}");
        assert!(!err.contains("pw") && !err.contains("k1"), "{err}");

        let (server, _) = crate::test_util::serve_json(|_| None).await;
        let err = fetch_err(&format!("{server}/p?key=k1")).await;
        assert!(err.contains("HTTP 404"), "{err}");
        assert!(err.contains(&format!("{server}/p")), "{err}");
        assert!(!err.contains("k1"), "{err}");
    }

    #[tokio::test]
    async fn fetch_body_error_redacts_override() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // 200 with a body cut short, so reading the body fails.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            let _ = stream.read(&mut buf).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{")
                .await;
        });
        let err = fetch_err(&format!("http://{addr}/p?key=k1")).await;
        assert!(err.contains("Failed to read Discovery Document"), "{err}");
        assert!(err.contains(&format!("http://{addr}/p")), "{err}");
        assert!(!err.contains("k1"), "{err}");
    }
}

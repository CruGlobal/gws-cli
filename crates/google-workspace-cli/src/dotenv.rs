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

//! Loads `./.env` (current directory only, never parents).
//!
//! `GOOGLE_WORKSPACE_CLI_DOTENV`: unset = load and warn about sensitive
//! variables, `1` = load quietly, `0` = don't load.

use std::path::Path;

pub const ENV_DOTENV: &str = "GOOGLE_WORKSPACE_CLI_DOTENV";
const DOTENV_FILE: &str = ".env";

/// `GOOGLE_WORKSPACE_CLI_*` variables that are safe to take from `.env` silently.
const QUIET_GWS_VARS: &[&str] = &[
    "GOOGLE_WORKSPACE_CLI_LOG",
    "GOOGLE_WORKSPACE_CLI_LOG_FILE",
    "GOOGLE_WORKSPACE_CLI_SANITIZE_MODE",
];

/// Non-gws variables that change where traffic goes or which credentials are used.
const SENSITIVE_OTHER_VARS: &[&str] = &[
    "GOOGLE_APPLICATION_CREDENTIALS",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Quiet,
    Warn,
}

impl Mode {
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("0" | "false" | "no" | "off") => Mode::Off,
            Some("1" | "true" | "yes" | "on") => Mode::Quiet,
            _ => Mode::Warn,
        }
    }
}

fn is_sensitive(name: &str) -> bool {
    if name.starts_with("GOOGLE_WORKSPACE_CLI_") {
        return !QUIET_GWS_VARS.contains(&name);
    }
    SENSITIVE_OTHER_VARS
        .iter()
        .any(|v| v.eq_ignore_ascii_case(name))
}

/// Variables from the file that are not already set. Shell values win.
fn select_new<F: Fn(&str) -> bool>(
    entries: Vec<(String, String)>,
    already_set: F,
) -> Vec<(String, String)> {
    entries
        .into_iter()
        .filter(|(k, _)| k != ENV_DOTENV && !already_set(k))
        .collect()
}

fn warning(mode: Mode, applied: &[(String, String)]) -> Option<String> {
    if mode != Mode::Warn {
        return None;
    }
    let names: Vec<&str> = applied
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| is_sensitive(k))
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(format!(
        "⚠️  Using {} from ./.env. Set {ENV_DOTENV}=1 to silence this, or {ENV_DOTENV}=0 to ignore .env.",
        names.join(", ")
    ))
}

fn read_entries(path: &Path) -> Result<Option<Vec<(String, String)>>, String> {
    let iter = match dotenvy::from_path_iter(path) {
        Ok(iter) => iter,
        Err(e) if e.not_found() => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    iter.collect::<Result<Vec<_>, _>>()
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Loads `./.env` into the process environment. Returns a message for stderr, if any.
pub fn load() -> Option<String> {
    let mode = Mode::from_env_value(std::env::var(ENV_DOTENV).ok().as_deref());
    load_from(Path::new(DOTENV_FILE), mode)
}

fn load_from(path: &Path, mode: Mode) -> Option<String> {
    if mode == Mode::Off {
        return None;
    }
    let entries = match read_entries(path) {
        Ok(Some(entries)) => entries,
        Ok(None) => return None,
        Err(e) => return Some(format!("⚠️  Ignoring ./.env: {e}")),
    };
    let applied = select_new(entries, |k| std::env::var_os(k).is_some());
    for (k, v) in &applied {
        std::env::set_var(k, v);
    }
    warning(mode, &applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn mode_parsing() {
        assert_eq!(Mode::from_env_value(None), Mode::Warn);
        assert_eq!(Mode::from_env_value(Some("")), Mode::Warn);
        assert_eq!(Mode::from_env_value(Some("0")), Mode::Off);
        assert_eq!(Mode::from_env_value(Some("false")), Mode::Off);
        assert_eq!(Mode::from_env_value(Some("1")), Mode::Quiet);
        assert_eq!(Mode::from_env_value(Some(" TRUE ")), Mode::Quiet);
        assert_eq!(Mode::from_env_value(Some("bogus")), Mode::Warn);
    }

    #[test]
    fn sensitive_classification() {
        assert!(is_sensitive("GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL"));
        assert!(is_sensitive("GOOGLE_WORKSPACE_CLI_TOKEN"));
        assert!(is_sensitive("GOOGLE_WORKSPACE_CLI_SOMETHING_NEW"));
        assert!(is_sensitive("https_proxy"));
        assert!(is_sensitive("SSL_CERT_FILE"));
        assert!(is_sensitive("GOOGLE_APPLICATION_CREDENTIALS"));
        assert!(!is_sensitive("GOOGLE_WORKSPACE_CLI_LOG"));
        assert!(!is_sensitive("GOOGLE_WORKSPACE_PROJECT_ID"));
        assert!(!is_sensitive("DATABASE_URL"));
    }

    #[test]
    fn shell_values_win_and_dotenv_flag_is_not_taken_from_file() {
        let entries = pairs(&[("A", "1"), ("B", "2"), (ENV_DOTENV, "1")]);
        let applied = select_new(entries, |k| k == "A");
        assert_eq!(applied, pairs(&[("B", "2")]));
    }

    #[test]
    fn warns_only_in_warn_mode_and_only_for_sensitive_vars() {
        let applied = pairs(&[
            ("GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL", "https://x"),
            ("GOOGLE_WORKSPACE_CLI_LOG", "debug"),
            ("HTTPS_PROXY", "http://p"),
        ]);
        let msg = warning(Mode::Warn, &applied).unwrap();
        assert!(msg.contains("GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL, HTTPS_PROXY"));
        assert!(!msg.contains("GOOGLE_WORKSPACE_CLI_LOG,"));
        assert!(!msg.contains("https://x"), "values must not be printed");
        assert_eq!(warning(Mode::Quiet, &applied), None);
        assert_eq!(
            warning(Mode::Warn, &pairs(&[("GOOGLE_WORKSPACE_CLI_LOG", "x")])),
            None
        );
    }

    #[test]
    fn read_entries_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_entries(&dir.path().join(".env")).unwrap(), None);
    }

    #[test]
    fn read_entries_parses_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "# c\nFOO=bar\nexport BAZ=\"q x\"\n").unwrap();
        assert_eq!(
            read_entries(&path).unwrap(),
            Some(pairs(&[("FOO", "bar"), ("BAZ", "q x")]))
        );
    }

    #[test]
    fn read_entries_reports_parse_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "NOT VALID LINE\n").unwrap();
        assert!(read_entries(&path).is_err());
    }

    #[test]
    fn load_from_child_ignores_parent_dotenv() {
        let parent = tempfile::tempdir().unwrap();
        std::fs::write(
            parent.path().join(".env"),
            "GWS_TEST_PARENT_DOTENV_VAR=leaked\n",
        )
        .unwrap();
        let child = parent.path().join("child");
        std::fs::create_dir(&child).unwrap();

        assert_eq!(load_from(&child.join(".env"), Mode::Warn), None);
        assert!(std::env::var_os("GWS_TEST_PARENT_DOTENV_VAR").is_none());
    }

    #[test]
    fn load_from_off_mode_reads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "GWS_TEST_OFF_MODE_VAR=x\n").unwrap();
        assert_eq!(load_from(&path, Mode::Off), None);
        assert!(std::env::var_os("GWS_TEST_OFF_MODE_VAR").is_none());
    }

    #[test]
    fn load_from_sets_vars_and_warns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "GOOGLE_WORKSPACE_CLI_TEST_DOTENV_SENSITIVE=x\n").unwrap();
        let msg = load_from(&path, Mode::Warn).unwrap();
        assert!(msg.contains("GOOGLE_WORKSPACE_CLI_TEST_DOTENV_SENSITIVE"));
        assert_eq!(
            std::env::var("GOOGLE_WORKSPACE_CLI_TEST_DOTENV_SENSITIVE").unwrap(),
            "x"
        );
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_TEST_DOTENV_SENSITIVE");
    }
}

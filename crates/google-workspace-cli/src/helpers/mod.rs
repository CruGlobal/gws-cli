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

use crate::error::GwsError;
use clap::{ArgMatches, Command};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
pub mod calendar;
pub mod chat;
pub mod docs;
pub mod drive;
pub mod events;
pub mod gmail;
pub mod modelarmor;
pub mod script;
pub mod sheets;
pub mod workflows;

/// Returns a future that completes when a shutdown signal is received.
///
/// On Unix this listens for both SIGINT (Ctrl+C) and SIGTERM; on other
/// platforms only SIGINT is handled. Used by long-running pull loops
/// (`gmail::watch`, `events::subscribe`) to exit cleanly under container
/// orchestrators (Kubernetes, Docker, systemd) that send SIGTERM.
///
/// The signal handler is registered once in a background task on first call
/// so it remains active for the lifetime of the process — no gap between
/// loop iterations.
pub(crate) async fn shutdown_signal() {
    use std::sync::OnceLock;
    use tokio::sync::Notify;

    static NOTIFY: OnceLock<std::sync::Arc<Notify>> = OnceLock::new();

    let notify = NOTIFY.get_or_init(|| {
        let n = std::sync::Arc::new(Notify::new());
        let n2 = n.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                match signal(SignalKind::terminate()) {
                    Ok(mut sigterm) => {
                        tokio::select! {
                            res = tokio::signal::ctrl_c() => {
                                res.expect("failed to listen for SIGINT");
                            }
                            Some(_) = sigterm.recv() => {}
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "warning: could not register SIGTERM handler: {e}. \
                             Listening for Ctrl+C only."
                        );
                        tokio::signal::ctrl_c()
                            .await
                            .expect("failed to listen for SIGINT");
                    }
                }
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c()
                    .await
                    .expect("failed to listen for SIGINT");
            }
            n2.notify_waiters();
        });
        n
    });

    notify.notified().await;
}

/// A trait for service-specific CLI helpers that inject custom commands.
pub trait Helper: Send + Sync {
    /// Injects subcommands into the service command.
    fn inject_commands(&self, cmd: Command, doc: &crate::discovery::RestDescription) -> Command;

    /// Attempts to handle a command. Returns Ok(Some(())) if handled,
    /// Ok(None) if not handled (should fall back to dynamic dispatch),
    /// or Err if handled but failed.
    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>>;

    /// If true, only helper commands are shown (discovery-generated commands are suppressed).
    fn helper_only(&self) -> bool {
        false
    }

    /// `+` commands that are safe under `--dry-run`: they honor it or make no changes.
    /// Unlisted commands are rejected when `--dry-run` is set.
    fn dry_run_safe_commands(&self) -> &'static [&'static str] {
        &[]
    }
}

/// Rejects `--dry-run` for helper commands that would ignore it and make real changes.
pub fn check_dry_run_support(helper: &dyn Helper, matches: &ArgMatches) -> Result<(), GwsError> {
    let Some((name, sub)) = matches.subcommand() else {
        return Ok(());
    };
    if !name.starts_with('+') {
        return Ok(());
    }
    let flag = |m: &ArgMatches| matches!(m.try_get_one::<bool>("dry-run"), Ok(Some(true)));
    if (flag(matches) || flag(sub)) && !helper.dry_run_safe_commands().contains(&name) {
        return Err(GwsError::Validation(format!(
            "`{name}` does not support --dry-run yet"
        )));
    }
    Ok(())
}

/// Build a `GwsError::Api` from an HTTP error response body, parsing the
/// Google JSON error format when possible. Modeled after the executor's
/// `handle_error_response`, extracting message, reason, and enable URL.
pub(crate) fn build_api_error(status: u16, body: &str, context: &str) -> GwsError {
    let err_json: Option<Value> = serde_json::from_str(body).ok();
    let err_obj = err_json.as_ref().and_then(|v| v.get("error"));
    let message = err_obj
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or(body)
        .to_string();
    let reason = err_obj
        .and_then(|e| e.get("errors"))
        .and_then(|e| e.as_array())
        .and_then(|arr| arr.first())
        .and_then(|e| e.get("reason"))
        .and_then(|r| r.as_str())
        .or_else(|| {
            err_obj
                .and_then(|e| e.get("reason"))
                .and_then(|r| r.as_str())
        })
        .unwrap_or("unknown")
        .to_string();
    let enable_url = if reason == "accessNotConfigured" {
        crate::executor::extract_enable_url(&message)
    } else {
        None
    };
    GwsError::Api {
        code: status,
        message: format!("{context}: {message}"),
        reason,
        enable_url,
    }
}

pub fn get_helper(service: &str) -> Option<Box<dyn Helper>> {
    match service {
        "gmail" => Some(Box::new(gmail::GmailHelper)),
        "sheets" => Some(Box::new(sheets::SheetsHelper)),
        "docs" => Some(Box::new(docs::DocsHelper)),
        "chat" => Some(Box::new(chat::ChatHelper)),
        "drive" => Some(Box::new(drive::DriveHelper)),
        "calendar" => Some(Box::new(calendar::CalendarHelper)),
        "script" | "apps-script" => Some(Box::new(script::ScriptHelper)),
        "workspaceevents" => Some(Box::new(events::EventsHelper)),
        "modelarmor" => Some(Box::new(modelarmor::ModelArmorHelper)),
        "workflow" => Some(Box::new(workflows::WorkflowHelper)),
        _ => None,
    }
}

#[cfg(test)]
mod dry_run_guard_tests {
    use super::*;

    const SERVICES: [&str; 10] = [
        "gmail",
        "sheets",
        "docs",
        "chat",
        "drive",
        "calendar",
        "script",
        "workspaceevents",
        "modelarmor",
        "workflow",
    ];

    fn matches_for(helper: &dyn Helper, args: &[&str]) -> ArgMatches {
        let root = Command::new("gws").arg(
            clap::Arg::new("dry-run")
                .long("dry-run")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        );
        let doc = crate::discovery::RestDescription::default();
        let names: Vec<String> = helper
            .inject_commands(Command::new("probe"), &doc)
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        let root = names
            .into_iter()
            .fold(root, |cmd, name| cmd.subcommand(Command::new(name)));
        root.try_get_matches_from(std::iter::once("gws").chain(args.iter().copied()))
            .unwrap()
    }

    fn check(service: &str, args: &[&str]) -> Result<(), GwsError> {
        let helper = get_helper(service).unwrap();
        let matches = matches_for(helper.as_ref(), args);
        check_dry_run_support(helper.as_ref(), &matches)
    }

    #[test]
    fn rejects_mutating_helpers_under_dry_run() {
        for (service, cmd) in [
            ("workflow", "+file-announce"),
            ("workflow", "+email-to-task"),
            ("gmail", "+watch"),
            ("modelarmor", "+create-template"),
        ] {
            match check(service, &[cmd, "--dry-run"]).unwrap_err() {
                GwsError::Validation(msg) => {
                    assert_eq!(msg, format!("`{cmd}` does not support --dry-run yet"))
                }
                other => panic!("expected Validation, got {other:?}"),
            }
            assert!(check(service, &["--dry-run", cmd]).is_err());
        }
    }

    #[test]
    fn allows_mutating_helpers_without_dry_run() {
        assert!(check("workflow", &["+file-announce"]).is_ok());
        assert!(check("gmail", &["+watch"]).is_ok());
    }

    #[test]
    fn allows_safe_helpers_under_dry_run() {
        for (service, cmd) in [
            ("workflow", "+standup-report"),
            ("gmail", "+triage"),
            ("gmail", "+send"),
            ("calendar", "+agenda"),
            ("modelarmor", "+sanitize-prompt"),
            ("workspaceevents", "+subscribe"),
        ] {
            assert!(check(service, &[cmd, "--dry-run"]).is_ok(), "{cmd}");
        }
    }

    #[test]
    fn dry_run_safe_commands_exist() {
        let doc = crate::discovery::RestDescription::default();
        for service in SERVICES {
            let helper = get_helper(service).unwrap();
            let cmd = helper.inject_commands(Command::new("probe"), &doc);
            for name in helper.dry_run_safe_commands() {
                assert!(
                    cmd.find_subcommand(name).is_some(),
                    "{service}: unknown command {name}"
                );
            }
        }
    }
}

#[cfg(test)]
mod build_api_error_tests {
    use super::*;

    #[test]
    fn test_build_api_error_parses_google_json_format() {
        let body = r#"{"error":{"code":403,"message":"Insufficient Permission","errors":[{"reason":"insufficientPermissions","domain":"global","message":"Insufficient Permission"}]}}"#;
        let err = build_api_error(403, body, "Test context");
        match err {
            GwsError::Api {
                code,
                message,
                reason,
                enable_url,
            } => {
                assert_eq!(code, 403);
                assert!(message.contains("Test context"));
                assert!(message.contains("Insufficient Permission"));
                assert_eq!(reason, "insufficientPermissions");
                assert!(enable_url.is_none());
            }
            _ => panic!("Expected GwsError::Api"),
        }
    }

    #[test]
    fn test_build_api_error_falls_back_to_raw_body() {
        let err = build_api_error(500, "Internal Server Error", "Test context");
        match err {
            GwsError::Api {
                code,
                message,
                reason,
                ..
            } => {
                assert_eq!(code, 500);
                assert!(message.contains("Internal Server Error"));
                assert_eq!(reason, "unknown");
            }
            _ => panic!("Expected GwsError::Api"),
        }
    }

    #[test]
    fn test_build_api_error_extracts_top_level_reason() {
        let body = r#"{"error":{"code":404,"message":"Not Found","reason":"notFound"}}"#;
        let err = build_api_error(404, body, "ctx");
        match err {
            GwsError::Api { reason, .. } => assert_eq!(reason, "notFound"),
            _ => panic!("Expected GwsError::Api"),
        }
    }

    #[test]
    fn test_build_api_error_access_not_configured_extracts_url() {
        let body = r#"{"error":{"code":403,"message":"People API has not been used in project 123 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/people.googleapis.com/overview?project=123 then retry.","errors":[{"reason":"accessNotConfigured"}]}}"#;
        let err = build_api_error(403, body, "ctx");
        match err {
            GwsError::Api {
                reason, enable_url, ..
            } => {
                assert_eq!(reason, "accessNotConfigured");
                assert!(enable_url.is_some());
                assert!(enable_url
                    .unwrap()
                    .contains("console.developers.google.com"));
            }
            _ => panic!("Expected GwsError::Api"),
        }
    }
}

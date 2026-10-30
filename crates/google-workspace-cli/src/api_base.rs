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

//! Base URLs for Google APIs that helpers call with hand-built URLs instead of
//! going through the discovery-driven executor.

use crate::discovery::RestDescription;
use crate::error::GwsError;

/// A Google API that helper code calls directly.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Api {
    Calendar,
    Chat,
    Drive,
    Gmail,
    People,
    PubSub,
    Tasks,
    WorkspaceEvents,
}

impl Api {
    /// Discovery service name, version, and Google's own base URL.
    fn spec(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Api::Calendar => ("calendar", "v3", "https://www.googleapis.com/calendar/v3/"),
            Api::Chat => ("chat", "v1", "https://chat.googleapis.com/"),
            Api::Drive => ("drive", "v3", "https://www.googleapis.com/drive/v3/"),
            Api::Gmail => ("gmail", "v1", "https://gmail.googleapis.com/"),
            Api::People => ("people", "v1", "https://people.googleapis.com/"),
            Api::PubSub => ("pubsub", "v1", "https://pubsub.googleapis.com/"),
            Api::Tasks => ("tasks", "v1", "https://tasks.googleapis.com/"),
            Api::WorkspaceEvents => (
                "workspaceevents",
                "v1",
                "https://workspaceevents.googleapis.com/",
            ),
        }
    }
}

/// The base URL that method paths are appended to, ending in `/`. Matches the
/// executor: with `GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL` set, it comes from
/// the API's discovery doc, so helpers follow the proxy like regular commands.
/// Otherwise it is Google's fixed base and no discovery fetch happens.
pub(crate) async fn api_base(api: Api) -> Result<String, GwsError> {
    let (service, version, google_base) = api.spec();
    if crate::discovery::base_url_override().is_none() {
        return Ok(google_base.to_string());
    }
    let doc = crate::discovery::fetch_discovery_document(service, version)
        .await
        .map_err(|e| {
            GwsError::Discovery(format!(
                "Failed to load {service}/{version} discovery document: {e}"
            ))
        })?;
    Ok(base_from_doc(&doc))
}

/// `api_base(api)` followed by `path`, a method path relative to that base.
pub(crate) async fn api_url(api: Api, path: &str) -> Result<String, GwsError> {
    Ok(format!("{}{path}", api_base(api).await?))
}

fn base_from_doc(doc: &RestDescription) -> String {
    doc.base_url
        .clone()
        .unwrap_or_else(|| format!("{}{}", doc.root_url, doc.service_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::EnvVarGuard;
    use serial_test::serial;

    fn doc(json: serde_json::Value) -> RestDescription {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn base_from_doc_joins_root_and_service_path() {
        let d = doc(serde_json::json!({
            "name": "calendar", "version": "v3",
            "rootUrl": "http://127.0.0.1:8080/", "servicePath": "calendar/v3/"
        }));
        assert_eq!(base_from_doc(&d), "http://127.0.0.1:8080/calendar/v3/");
    }

    #[test]
    fn base_from_doc_prefers_base_url() {
        let d = doc(serde_json::json!({
            "name": "gmail", "version": "v1",
            "rootUrl": "http://127.0.0.1:8080/", "servicePath": "",
            "baseUrl": "http://proxy.test/gmail/"
        }));
        assert_eq!(base_from_doc(&d), "http://proxy.test/gmail/");
    }

    #[tokio::test]
    #[serial]
    async fn api_base_without_override_is_google() {
        let _guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL");
        assert_eq!(
            api_base(Api::Gmail).await.unwrap(),
            "https://gmail.googleapis.com/"
        );
        assert_eq!(
            api_base(Api::Calendar).await.unwrap(),
            "https://www.googleapis.com/calendar/v3/"
        );
    }

    #[tokio::test]
    #[serial]
    async fn api_base_with_override_uses_discovery_doc() {
        let dir = tempfile::tempdir().unwrap();
        let _config = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", dir.path());
        let (server, requests) = crate::test_util::serve_json(|_| {
            Some(serde_json::json!({
                "name": "tasks", "version": "v1",
                "rootUrl": "http://proxy.test/", "servicePath": ""
            }))
        })
        .await;
        let _guard = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL", &server);
        assert_eq!(api_base(Api::Tasks).await.unwrap(), "http://proxy.test/");
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /discovery/v1/apis/tasks/v1/rest "));
    }
}

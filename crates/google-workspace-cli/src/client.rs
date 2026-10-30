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

//! HTTP client — re-exports from `google_workspace` library crate, plus a
//! guard that keeps API traffic on the discovery proxy when one is set.

pub use google_workspace::client::*;

use crate::error::GwsError;
use std::sync::Arc;

/// Builds an HTTP client. When `GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL` is
/// set, the client refuses to connect to Google API hosts, so a request that
/// skips the proxy fails instead of going to Google directly.
pub fn build_client() -> Result<reqwest::Client, GwsError> {
    if crate::discovery::base_url_override().is_none() {
        return google_workspace::client::build_client();
    }
    client_builder()
        .dns_resolver(Arc::new(GoogleHostGuard))
        .build()
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to build HTTP client: {e}")))
}

/// Whether the guard lets `host` through: anything outside googleapis.com,
/// OAuth (auth is not routed through the proxy), and Model Armor's regional
/// endpoints, which discovery can't describe.
fn guard_allows(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let is_google_api = host == "googleapis.com" || host.ends_with(".googleapis.com");
    !is_google_api || host == "oauth2.googleapis.com" || host.ends_with(".rep.googleapis.com")
}

struct GoogleHostGuard;

impl reqwest::dns::Resolve for GoogleHostGuard {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            if !guard_allows(&host) {
                let message = format!(
                    "refusing to connect to {host}: GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL is set, \
                     so Google API requests must go through it"
                );
                // reqwest's error Display hides the resolver's message.
                eprintln!("Error: {message}");
                return Err(Box::new(HostBlocked(message)) as _);
            }
            let addrs: Vec<std::net::SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn guard_blocks_google_api_hosts() {
        assert!(!guard_allows("www.googleapis.com"));
        assert!(!guard_allows("gmail.googleapis.com"));
        assert!(!guard_allows("GMAIL.googleapis.com."));
        assert!(!guard_allows("googleapis.com"));
    }

    #[test]
    fn guard_allows_oauth_model_armor_and_other_hosts() {
        assert!(guard_allows("oauth2.googleapis.com"));
        assert!(guard_allows("modelarmor.us-central1.rep.googleapis.com"));
        assert!(guard_allows("127.0.0.1"));
        assert!(guard_allows("proxy.example.com"));
        assert!(guard_allows("notgoogleapis.com"));
    }

    #[tokio::test]
    #[serial]
    async fn guarded_client_refuses_google_without_retrying() {
        let _guard = crate::test_util::EnvVarGuard::set(
            "GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL",
            "http://127.0.0.1:9",
        );
        let client = build_client().unwrap();
        let started = std::time::Instant::now();
        let err = send_with_retry(|| client.get("https://www.googleapis.com/calendar/v3/x"))
            .await
            .unwrap_err();
        assert!(err.is_connect());
        // Retrying would sleep 1s + 2s + 4s first.
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn cross_origin_redirect_drops_authorization() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (target, requests) =
            crate::test_util::serve_json(|_| Some(serde_json::json!({}))).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            let _ = stream.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {target}/landed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = build_client().unwrap();
        let resp = client
            .get(format!("{origin}/start"))
            .bearer_auth("secret-token")
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /landed "));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization"));
    }
}

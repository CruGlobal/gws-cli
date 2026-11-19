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

//! Browser login for `gws auth login`: OAuth 2.0 for installed apps with a
//! loopback redirect (RFC 8252), a `state` check and PKCE (RFC 7636, S256).

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::error::GwsError;

const AUTH_URI: &str = "https://accounts.google.com/o/oauth2/auth";
const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST_LINE: u64 = 8192;

const SUCCESS_PAGE: &str = "<html><body><h1>Success!</h1>\
    <p>You may now close this window and return to the terminal.</p></body></html>";
const DENIED_PAGE: &str = "<html><body><h1>Login cancelled</h1>\
    <p>You can close this window.</p></body></html>";
const IGNORED_PAGE: &str = "<html><body><p>Not a valid login response.</p></body></html>";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

/// Run the browser login and return `(access_token, refresh_token)`.
pub(crate) async fn login(
    client_id: &str,
    client_secret: &str,
    scopes: &[String],
) -> Result<(String, String), GwsError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| GwsError::Auth(format!("Failed to start local server: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| GwsError::Auth(format!("Failed to inspect local server: {e}")))?
        .port();
    let redirect_uri = format!("http://localhost:{port}");

    let state = random_token();
    let verifier = random_token();
    let auth_url = build_auth_url(
        client_id,
        &redirect_uri,
        scopes,
        &state,
        &pkce_challenge(&verifier),
    );

    eprintln!("Open this URL in your browser to authenticate:\n");
    eprintln!("  {auth_url}\n");

    let code = wait_for_code(&listener, &state, LOGIN_TIMEOUT, REQUEST_READ_TIMEOUT).await?;
    let tokens = exchange_code(
        TOKEN_URI,
        client_id,
        client_secret,
        &code,
        &redirect_uri,
        &verifier,
    )
    .await?;

    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        GwsError::Auth(
            "OAuth flow completed but no refresh token was returned. \
             Ensure the OAuth consent screen includes 'offline' access."
                .to_string(),
        )
    })?;
    Ok((tokens.access_token, refresh_token))
}

/// 32 random bytes, base64url: 43 chars, a valid PKCE verifier and `state`.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn build_auth_url(
    client_id: &str,
    redirect_uri: &str,
    scopes: &[String],
    state: &str,
    code_challenge: &str,
) -> String {
    let mut url = reqwest::Url::parse(AUTH_URI).expect("AUTH_URI is a valid URL");
    url.query_pairs_mut()
        .append_pair("scope", &scopes.join(" "))
        .append_pair("access_type", "offline")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("prompt", "select_account consent")
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256");
    url.into()
}

#[derive(Debug, PartialEq)]
enum Callback {
    Code(String),
    Denied(String),
    Ignored,
}

/// Classify one request to the loopback server. Anything without our exact
/// `state` is ignored, so a forged redirect can neither log us in nor abort.
fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::Ignored;
    };
    if !target.starts_with('/') {
        return Callback::Ignored;
    }
    let Ok(url) = reqwest::Url::parse(&format!("http://localhost{target}")) else {
        return Callback::Ignored;
    };
    if url.path() != "/" {
        return Callback::Ignored;
    }

    let (mut state, mut code, mut error) = (None, None, None);
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "state" => state = Some(value.into_owned()),
            "code" => code = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Callback::Ignored;
    }
    match (error, code) {
        (Some(error), _) => Callback::Denied(error),
        (None, Some(code)) if !code.is_empty() => Callback::Code(code),
        _ => Callback::Ignored,
    }
}

async fn wait_for_code(
    listener: &TcpListener,
    expected_state: &str,
    login_timeout: Duration,
    read_timeout: Duration,
) -> Result<String, GwsError> {
    let accept_loop = async {
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| GwsError::Auth(format!("Failed to accept connection: {e}")))?;
            let Some(line) = read_request_line(&mut stream, read_timeout).await else {
                continue;
            };
            match parse_callback(&line, expected_state) {
                Callback::Code(code) => {
                    respond(&mut stream, "200 OK", SUCCESS_PAGE).await;
                    return Ok(code);
                }
                Callback::Denied(error) => {
                    respond(&mut stream, "200 OK", DENIED_PAGE).await;
                    return Err(GwsError::Auth(format!(
                        "Login was not authorized: {}",
                        crate::output::sanitize_for_terminal(&error)
                    )));
                }
                Callback::Ignored => respond(&mut stream, "400 Bad Request", IGNORED_PAGE).await,
            }
        }
    };
    tokio::time::timeout(login_timeout, accept_loop)
        .await
        .map_err(|_| {
            GwsError::Auth(format!(
                "Timed out after {}s waiting for the browser login to finish",
                login_timeout.as_secs()
            ))
        })?
}

async fn read_request_line(stream: &mut TcpStream, read_timeout: Duration) -> Option<String> {
    let mut reader = BufReader::new(stream.take(MAX_REQUEST_LINE));
    let mut line = String::new();
    match tokio::time::timeout(read_timeout, reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => Some(line),
        _ => None,
    }
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn exchange_code(
    token_uri: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, GwsError> {
    let client = crate::client::shared_client()?;
    let response = client
        .post(token_uri)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("code_verifier", code_verifier),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| GwsError::Auth(format!("Failed to send token request: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = crate::auth::response_text_or_placeholder(response.text().await);
        return Err(GwsError::Auth(format!(
            "Token exchange failed with status {status}: {body}"
        )));
    }

    response
        .json()
        .await
        .map_err(|e| GwsError::Auth(format!("Failed to parse token response: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = "expected-state";

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_token_is_a_valid_unique_verifier() {
        let a = random_token();
        let b = random_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn auth_url_carries_state_and_pkce() {
        let scopes = vec!["openid".to_string(), "https://x/y z".to_string()];
        let url = build_auth_url("cid", "http://localhost:1234", &scopes, "st", "ch");
        let parsed = reqwest::Url::parse(&url).unwrap();
        let params: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params["scope"], "openid https://x/y z");
        assert_eq!(params["redirect_uri"], "http://localhost:1234");
        assert_eq!(params["client_id"], "cid");
        assert_eq!(params["access_type"], "offline");
        assert_eq!(params["prompt"], "select_account consent");
        assert_eq!(params["state"], "st");
        assert_eq!(params["code_challenge"], "ch");
        assert_eq!(params["code_challenge_method"], "S256");
    }

    #[test]
    fn callback_with_matching_state_returns_decoded_code() {
        let line = format!("GET /?state={STATE}&code=4%2F0Abc&scope=openid HTTP/1.1\r\n");
        assert_eq!(
            parse_callback(&line, STATE),
            Callback::Code("4/0Abc".into())
        );
    }

    #[test]
    fn callback_without_matching_state_is_ignored() {
        for line in [
            "GET /?code=4/injected HTTP/1.1",
            "GET /?state=wrong&code=4/injected HTTP/1.1",
            "GET /?state=wrong&error=access_denied HTTP/1.1",
        ] {
            assert_eq!(parse_callback(line, STATE), Callback::Ignored, "{line}");
        }
    }

    #[test]
    fn non_callback_requests_are_ignored() {
        for line in [
            format!("GET /favicon.ico?state={STATE}&code=c HTTP/1.1"),
            format!("POST /?state={STATE}&code=c HTTP/1.1"),
            format!("GET http://evil/?state={STATE}&code=c HTTP/1.1"),
            format!("GET /?state={STATE} HTTP/1.1"),
            format!("GET /?state={STATE}&code= HTTP/1.1"),
            String::new(),
        ] {
            assert_eq!(parse_callback(&line, STATE), Callback::Ignored, "{line}");
        }
    }

    #[test]
    fn callback_error_with_matching_state_is_denied() {
        let line = format!("GET /?state={STATE}&error=access_denied HTTP/1.1");
        assert_eq!(
            parse_callback(&line, STATE),
            Callback::Denied("access_denied".into())
        );
    }

    async fn send(addr: std::net::SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn wait_for_code_skips_stray_and_forged_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::spawn(async move {
            // Idle connection: must not block the real callback.
            let _idle = TcpStream::connect(addr).await.unwrap();
            let favicon = send(addr, "GET /favicon.ico HTTP/1.1\r\n\r\n").await;
            let forged = send(addr, "GET /?state=wrong&code=evil HTTP/1.1\r\n\r\n").await;
            let real = send(
                addr,
                &format!("GET /?state={STATE}&code=good HTTP/1.1\r\n\r\n"),
            )
            .await;
            (favicon, forged, real)
        });

        let code = wait_for_code(
            &listener,
            STATE,
            Duration::from_secs(10),
            Duration::from_millis(200),
        )
        .await
        .unwrap();
        let (favicon, forged, real) = client.await.unwrap();

        assert_eq!(code, "good");
        assert!(favicon.starts_with("HTTP/1.1 400"), "{favicon}");
        assert!(forged.starts_with("HTTP/1.1 400"), "{forged}");
        assert!(real.starts_with("HTTP/1.1 200"), "{real}");
        assert!(real.contains("Success!"));
    }

    #[tokio::test]
    async fn wait_for_code_reports_denied_login() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            send(
                addr,
                &format!("GET /?state={STATE}&error=access_denied HTTP/1.1\r\n\r\n"),
            )
            .await
        });
        let err = wait_for_code(
            &listener,
            STATE,
            Duration::from_secs(10),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("access_denied"), "{err}");
    }

    #[tokio::test]
    async fn wait_for_code_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let err = wait_for_code(
            &listener,
            STATE,
            Duration::from_millis(100),
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("Timed out"), "{err}");
    }

    #[tokio::test]
    async fn exchange_code_sends_pkce_verifier() {
        let (base, requests) = crate::test_util::serve_json(|_| {
            Some(serde_json::json!({
                "access_token": "at",
                "refresh_token": "rt",
                "expires_in": 3600,
                "token_type": "Bearer"
            }))
        })
        .await;

        let tokens = exchange_code(
            &format!("{base}/token"),
            "cid",
            "secret",
            "4/code",
            "http://localhost:1234",
            "the-verifier",
        )
        .await
        .unwrap();

        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        let request = requests.lock().unwrap().join("\n");
        assert!(request.contains("code_verifier=the-verifier"), "{request}");
        assert!(request.contains("code=4%2Fcode"), "{request}");
        assert!(
            request.contains("grant_type=authorization_code"),
            "{request}"
        );
    }
}

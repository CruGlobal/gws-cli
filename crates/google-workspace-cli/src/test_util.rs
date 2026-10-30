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

//! Shared test helpers.

/// RAII guard that saves the current value of an environment variable and
/// restores it when dropped, so env-var-dependent tests don't leak state.
pub(crate) struct EnvVarGuard {
    name: String,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    pub(crate) fn set(name: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(name);
        std::env::set_var(name, value);
        Self {
            name: name.to_string(),
            original,
        }
    }

    pub(crate) fn remove(name: &str) -> Self {
        let original = std::env::var_os(name);
        std::env::remove_var(name);
        Self {
            name: name.to_string(),
            original,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(v) => std::env::set_var(&self.name, v),
            None => std::env::remove_var(&self.name),
        }
    }
}

/// Starts a local HTTP server that answers every request with
/// `respond(path)` as a JSON body (404 when it returns `None`) and records
/// each raw request. Returns the server's base URL (`http://127.0.0.1:PORT`).
pub(crate) async fn serve_json(
    respond: impl Fn(&str) -> Option<serde_json::Value> + Send + 'static,
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = std::sync::Arc::clone(&requests);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0_u8; 8192];
            let bytes_read = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..bytes_read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("")
                .to_string();
            let (status, body) = match respond(&path) {
                Some(json) => ("200 OK", json.to_string()),
                None => ("404 Not Found", "{}".to_string()),
            };
            recorded.lock().unwrap().push(request.to_string());
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    (format!("http://{addr}"), requests)
}

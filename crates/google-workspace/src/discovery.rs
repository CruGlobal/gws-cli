#![allow(dead_code)]
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

//! Discovery Document Parsing and Management
//!
//! Handles fetching, caching, and parsing Google API Discovery Documents.
//! These JSON schemas define the shapes of API requests and responses, forming
//! the foundation of the dynamically generated CLI commands.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level Discovery REST Description document.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestDescription {
    pub name: String,
    pub version: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub root_url: String,
    #[serde(default)]
    pub service_path: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub schemas: HashMap<String, JsonSchema>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    pub auth: Option<AuthDescription>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuthDescription {
    pub oauth2: Option<OAuth2Description>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OAuth2Description {
    pub scopes: Option<HashMap<String, ScopeDescription>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ScopeDescription {
    pub description: Option<String>,
}

/// A resource in the Discovery Document, which can contain methods and nested sub-resources.
#[derive(Debug, Deserialize, Default)]
pub struct RestResource {
    #[serde(default)]
    pub methods: HashMap<String, RestMethod>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
}

/// A single API method.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestMethod {
    pub id: Option<String>,
    pub description: Option<String>,
    pub http_method: String,
    pub path: String,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    #[serde(default)]
    pub parameter_order: Vec<String>,
    pub request: Option<SchemaRef>,
    pub response: Option<SchemaRef>,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub flat_path: Option<String>,
    #[serde(default)]
    pub supports_media_download: bool,
    #[serde(default)]
    pub supports_media_upload: bool,
    pub media_upload: Option<MediaUpload>,
}

/// Media upload metadata from the Discovery Document.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUpload {
    pub protocols: Option<MediaUploadProtocols>,
    pub accept: Option<Vec<String>>,
}

/// Upload protocol details.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocols {
    pub simple: Option<MediaUploadProtocol>,
}

/// A single upload protocol entry.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocol {
    pub path: String,
    pub multipart: Option<bool>,
}

/// A reference to a schema (e.g., `{ "$ref": "File" }`).
#[derive(Debug, Deserialize, Default)]
pub struct SchemaRef {
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    #[serde(rename = "parameterName")]
    pub parameter_name: Option<String>,
}

/// A parameter definition for a method.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MethodParameter {
    #[serde(rename = "type")]
    pub param_type: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub format: Option<String>,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub enum_descriptions: Option<Vec<String>>,
    #[serde(default)]
    pub repeated: bool,
    pub minimum: Option<String>,
    pub maximum: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
}

/// JSON Schema definition for request/response bodies.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchema {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub schema_type: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub required: Vec<String>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

/// A property within a JSON Schema.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchemaProperty {
    #[serde(rename = "type")]
    pub prop_type: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub format: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(default)]
    pub read_only: bool,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

/// Builds the primary discovery-document URL.
///
/// When `base_url` is `Some`, the document is fetched from a proxy at
/// `{base}/discovery/v1/apis/{service}/{version}/rest` (trailing slashes on the
/// base are trimmed). When `None`, the standard Google endpoint is used.
fn primary_discovery_url(service: &str, version: &str, base_url: Option<&str>) -> String {
    let service = crate::validate::encode_path_segment(service);
    let version = crate::validate::encode_path_segment(version);
    match base_url {
        Some(base) => format!(
            "{}/discovery/v1/apis/{service}/{version}/rest",
            base.trim_end_matches('/')
        ),
        None => format!("https://www.googleapis.com/discovery/v1/apis/{service}/{version}/rest"),
    }
}

/// Returns the per-service `$discovery/rest` fallback URL to try when the
/// primary discovery fetch fails.
///
/// Returns `None` when a `base_url` override is set: the proxy is expected to
/// serve every document (or return a meaningful error), so we must not fall back
/// to per-service Google hosts, which would bypass the proxy.
fn discovery_fallback_url(service: &str, base_url: Option<&str>) -> Option<String> {
    match base_url {
        Some(_) => None,
        // Pattern used by newer APIs (Forms, Keep, Meet, etc.).
        None => Some(format!("https://{service}.googleapis.com/$discovery/rest")),
    }
}

/// Builds the on-disk cache filename for a discovery document.
///
/// Without an override the filename is `{service}_{version}.json` (unchanged,
/// backward compatible). With a `base_url` override the filename is namespaced
/// with a short stable hash of the (trailing-slash-trimmed) base URL —
/// `{service}_{version}_{hash}.json` — so proxied and direct documents never
/// share a cache entry.
fn discovery_cache_filename(service: &str, version: &str, base_url: Option<&str>) -> String {
    match base_url {
        Some(base) => {
            let hash = short_hash(base.trim_end_matches('/'));
            format!("{service}_{version}_{hash}.json")
        }
        None => format!("{service}_{version}.json"),
    }
}

/// Returns a short (8 hex char), stable, dependency-free hash of `input`.
///
/// Uses the 64-bit FNV-1a algorithm so the value is deterministic across
/// platforms and Rust versions (unlike `DefaultHasher`), keeping cache
/// filenames stable between runs.
fn short_hash(input: &str) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")[..8].to_string()
}

/// Fetches and caches a Google Discovery Document.
///
/// When `cache_dir` is `Some`, the document is cached on disk with a 24-hour
/// TTL. Pass `None` to skip caching entirely.
///
/// When `base_url` is `Some`, discovery is fetched from that base (e.g. a proxy)
/// instead of `www.googleapis.com`, and the per-service `$discovery/rest`
/// fallback is skipped — the proxy is expected to serve every document or return
/// a meaningful error. The cache filename is namespaced by the base URL so
/// proxied and direct documents never mix.
pub async fn fetch_discovery_document(
    service: &str,
    version: &str,
    cache_dir: Option<&std::path::Path>,
    base_url: Option<&str>,
) -> anyhow::Result<RestDescription> {
    // Validate service and version to prevent path traversal in cache filenames
    // and injection in discovery URLs.
    let service =
        crate::validate::validate_api_identifier(service).map_err(|e| anyhow::anyhow!("{e}"))?;
    let version =
        crate::validate::validate_api_identifier(version).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Check cache (24hr TTL)
    if let Some(dir) = cache_dir {
        tokio::fs::create_dir_all(dir).await?;
        let cache_file = dir.join(discovery_cache_filename(service, version, base_url));

        if let Ok(metadata) = tokio::fs::metadata(&cache_file).await {
            if let Ok(modified) = metadata.modified() {
                if modified.elapsed().unwrap_or_default() < std::time::Duration::from_secs(86400) {
                    let data = tokio::fs::read_to_string(&cache_file).await?;
                    let doc: RestDescription = serde_json::from_str(&data)?;
                    tracing::debug!(service = %service, version = %version, "Discovery cache hit");
                    return Ok(doc);
                }
            }
        }
    }

    let url = primary_discovery_url(service, version, base_url);

    tracing::debug!(service = %service, version = %version, "Fetching discovery document");
    let client = crate::client::build_client()?;
    let resp = client.get(&url).send().await?;

    let body = if resp.status().is_success() {
        resp.text().await?
    } else if let Some(alt_url) = discovery_fallback_url(service, base_url) {
        let alt_resp = client
            .get(&alt_url)
            .query(&[("version", version)])
            .send()
            .await?;
        if !alt_resp.status().is_success() {
            anyhow::bail!(
                "Failed to fetch Discovery Document for {service}/{version}: HTTP {} (tried both standard and $discovery URLs)",
                alt_resp.status()
            );
        }
        alt_resp.text().await?
    } else {
        // A base-URL override is set: the proxy serves every document, so we do
        // not fall back to per-service Google hosts (which would bypass it).
        anyhow::bail!(
            "Failed to fetch Discovery Document for {service}/{version} from {url}: HTTP {}",
            resp.status()
        );
    };

    // Write to cache
    if let Some(dir) = cache_dir {
        let cache_file = dir.join(discovery_cache_filename(service, version, base_url));
        if let Err(e) = tokio::fs::write(&cache_file, &body).await {
            tracing::warn!(error = %e, "Failed to write discovery cache");
        }
    }

    let doc: RestDescription = serde_json::from_str(&body)?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_rest_description() {
        let json = r#"{
            "name": "drive",
            "version": "v3",
            "rootUrl": "https://www.googleapis.com/",
            "servicePath": "drive/v3/",
            "resources": {
                "files": {
                    "methods": {
                        "list": {
                            "httpMethod": "GET",
                            "path": "files",
                            "response": { "$ref": "FileList" }
                        }
                    }
                }
            },
            "schemas": {
                "FileList": {
                    "id": "FileList",
                    "type": "object",
                    "properties": {
                        "files": {
                            "type": "array",
                            "items": { "$ref": "File" }
                        }
                    }
                }
            }
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.name, "drive");
        assert_eq!(doc.version, "v3");
        assert_eq!(doc.root_url, "https://www.googleapis.com/");
        assert_eq!(doc.service_path, "drive/v3/");

        // precise resource checking
        let files = doc.resources.get("files").expect("files resource missing");
        let list = files.methods.get("list").expect("list method missing");
        assert_eq!(list.http_method, "GET");
        assert_eq!(list.path, "files");

        // schema checking
        let file_list = doc
            .schemas
            .get("FileList")
            .expect("FileList schema missing");
        assert_eq!(file_list.id.as_deref(), Some("FileList"));
    }

    #[test]
    fn test_deserialize_defaults() {
        let json = r#"{
            "name": "admin",
            "version": "directory_v1",
            "rootUrl": "https://admin.googleapis.com/"
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.service_path, ""); // default empty string
        assert!(doc.resources.is_empty());
        assert!(doc.schemas.is_empty());
    }

    #[test]
    fn test_primary_discovery_url_default() {
        assert_eq!(
            primary_discovery_url("drive", "v3", None),
            "https://www.googleapis.com/discovery/v1/apis/drive/v3/rest"
        );
    }

    #[test]
    fn test_primary_discovery_url_override() {
        assert_eq!(
            primary_discovery_url("drive", "v3", Some("https://proxy.example.com")),
            "https://proxy.example.com/discovery/v1/apis/drive/v3/rest"
        );
    }

    #[test]
    fn test_primary_discovery_url_override_trims_trailing_slash() {
        // A single or multiple trailing slashes on the base are stripped.
        assert_eq!(
            primary_discovery_url("drive", "v3", Some("https://proxy.example.com/")),
            "https://proxy.example.com/discovery/v1/apis/drive/v3/rest"
        );
        assert_eq!(
            primary_discovery_url("drive", "v3", Some("https://proxy.example.com/base///")),
            "https://proxy.example.com/base/discovery/v1/apis/drive/v3/rest"
        );
    }

    #[test]
    fn test_fallback_url_used_without_override() {
        assert_eq!(
            discovery_fallback_url("forms", None).as_deref(),
            Some("https://forms.googleapis.com/$discovery/rest")
        );
    }

    #[test]
    fn test_fallback_skipped_with_override() {
        // Override set => no per-service Google fallback (proxy serves all).
        assert_eq!(
            discovery_fallback_url("forms", Some("https://proxy.example.com")),
            None
        );
    }

    #[test]
    fn test_cache_filename_default_unchanged() {
        // Backward compatible: no override => exact legacy filename.
        assert_eq!(
            discovery_cache_filename("drive", "v3", None),
            "drive_v3.json"
        );
    }

    #[test]
    fn test_cache_filename_override_is_namespaced() {
        let name = discovery_cache_filename("drive", "v3", Some("https://proxy.example.com"));
        // {service}_{version}_{hash8}.json
        assert!(name.starts_with("drive_v3_"));
        assert!(name.ends_with(".json"));
        let hash = name
            .strip_prefix("drive_v3_")
            .and_then(|s| s.strip_suffix(".json"))
            .unwrap();
        assert_eq!(hash.len(), 8);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        // Differs from the un-namespaced default so proxied/direct never collide.
        assert_ne!(name, "drive_v3.json");
    }

    #[test]
    fn test_cache_filename_trailing_slash_matches_bare() {
        // Trailing slashes are trimmed before hashing, so these resolve to the
        // same cache entry.
        assert_eq!(
            discovery_cache_filename("drive", "v3", Some("https://proxy.example.com")),
            discovery_cache_filename("drive", "v3", Some("https://proxy.example.com/"))
        );
    }

    #[test]
    fn test_cache_filename_distinct_bases_distinct_files() {
        assert_ne!(
            discovery_cache_filename("drive", "v3", Some("https://proxy-a.example.com")),
            discovery_cache_filename("drive", "v3", Some("https://proxy-b.example.com"))
        );
    }

    #[test]
    fn test_short_hash_is_stable_and_short() {
        // Deterministic across runs (FNV-1a), fixed known value.
        assert_eq!(short_hash("https://proxy.example.com"), "b7266c31");
        assert_eq!(short_hash("https://proxy.example.com").len(), 8);
    }
}

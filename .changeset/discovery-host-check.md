---
"gws-cli": patch
---

Check the API host from a discovery document's `rootUrl`/`baseUrl` before sending a request. Without `GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL` it must be https on `googleapis.com`; with it, it must match the override's scheme, host and port. Foreign hosts (including from a tampered cache file) now fail with a discovery error and no token is sent.

---
"gws-cli": patch
---

Security hardening (#129): `gcloud` child processes no longer inherit `GOOGLE_WORKSPACE_CLI_*` env vars, and `gws auth status` no longer runs `gcloud` (its `enabled_apis` fields are gone). `gws auth logout` only deletes files inside the gws config dir; a `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` outside it is kept, noted on stderr and listed under `kept`. The `GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL` value is shown without userinfo, query or fragment in warnings and errors.

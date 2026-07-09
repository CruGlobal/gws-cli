---
"gws-cli": minor
---

Add `GOOGLE_WORKSPACE_CLI_AUTH=none` to skip client-side credential acquisition and send requests with no `Authorization` header, for deployments behind a credential-injecting network proxy (e.g. gapx).

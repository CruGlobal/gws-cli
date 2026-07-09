---
"gws-cli": patch
---

Fix no-auth mode (`GOOGLE_WORKSPACE_CLI_AUTH=none`) masking proxy 401/403 denials as "No credentials provided". The proxy's real error message is now surfaced verbatim; the "run `gws auth login`" hint still fires for the genuine forgot-to-log-in case.

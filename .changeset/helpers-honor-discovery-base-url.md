---
"gws-cli": patch
---

Helper commands (`+agenda`, `+triage`, `+watch`, workflows, etc.) now route through `GOOGLE_WORKSPACE_CLI_DISCOVERY_BASE_URL` like regular commands. With the override set, direct connections to `*.googleapis.com` are refused (OAuth and Model Armor regional endpoints excepted), and a warning is printed when it is set without `GOOGLE_WORKSPACE_CLI_AUTH=none`.

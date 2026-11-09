---
"gws-cli": minor
---

`.env` is now read from the current directory only (parent directories are no longer searched). gws warns on stderr when `.env` sets a variable that changes where requests go or which credentials are used. New `GOOGLE_WORKSPACE_CLI_DOTENV`: `0` disables `.env` loading, `1` loads it without the warning.

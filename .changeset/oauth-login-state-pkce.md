---
"gws-cli": patch
---

`gws auth login` now uses one built-in browser login flow with a `state` check and PKCE (S256), replacing yup-oauth2's flow and the separate proxy flow. The local callback server ignores stray or forged requests instead of taking the first connection, reports a cancelled login, times out after 5 minutes, and the login URL is always printed to stderr.

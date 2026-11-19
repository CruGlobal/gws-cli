---
"gws-cli": patch
---

`gws auth status` now sends the access token to Google's tokeninfo endpoint in the POST body instead of the URL query string, so it no longer shows up in proxy or server logs.

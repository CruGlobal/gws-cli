---
"gws-cli": patch
---

Validate Model Armor location before building the regional endpoint URL, so a crafted template name or `--location` can't redirect requests (and the auth token) to another host.

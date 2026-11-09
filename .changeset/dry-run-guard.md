---
"gws-cli": patch
---

`--dry-run` now fails fast with a validation error on helper commands that would ignore it and make real changes (`gmail +watch`, `workflow +file-announce`, `workflow +email-to-task`, `modelarmor +create-template`).

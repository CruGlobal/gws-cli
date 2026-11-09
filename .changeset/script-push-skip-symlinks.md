---
"gws-cli": patch
---

`script +push` now skips symlinks (with a warning) and hidden directories, and refuses files that resolve outside the project directory, so files outside the project can't be uploaded as Apps Script source.

---
"gws-cli": patch
---

Update `url` to 2.5.9 to fix RUSTSEC-2026-0592 (stack overflow in `Url::origin()` on nested `blob:` URLs).

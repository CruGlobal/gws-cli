---
"gws-cli": patch
---

`gws auth export` (masked) now shows only the last 4 characters of each secret, hides a service account's `private_key` and `private_key_id`, and refuses to print credentials that are not valid JSON instead of printing them unmasked.

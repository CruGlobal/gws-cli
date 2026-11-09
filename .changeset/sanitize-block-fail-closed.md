---
"gws-cli": patch
---

Model Armor `block` mode now fails closed: if the sanitizer call fails, output is blocked with a non-zero exit instead of passing through unchecked. `gmail +watch` drops the message in that case.

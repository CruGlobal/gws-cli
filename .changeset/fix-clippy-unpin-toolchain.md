---
"gws-cli": patch
---

Fix clippy findings across the workspace and unpin the Lint job from Rust 1.93.1 so it tracks stable again. Clippy now runs with `--all-targets` to cover test code.

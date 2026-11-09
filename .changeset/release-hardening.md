---
"gws-cli": patch
---

Harden release pipeline: update yanked async-executor and deny yanked crates, refresh only workspace versions in Cargo.lock at release (`cargo update -w`), use a read-only default token in the release workflow with write perms only on the tag-only release job, and remove the unused `npm/` installer.

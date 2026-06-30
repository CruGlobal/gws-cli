---
"gws-cli": patch
---

Re-enable the Security Audit (`cargo audit`) and Cargo Deny gates by removing their `continue-on-error`. The RUSTSEC advisories that prompted the temporary bypass (e.g. RUSTSEC-2026-0097 against rand 0.9.2) were already resolved on main by dependency bumps, so both gates now pass with no ignores required. Also refreshes transitive deps via `cargo update`.

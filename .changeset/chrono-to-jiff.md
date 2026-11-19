---
"gws-cli": patch
---

Replace unmaintained `chrono`/`chrono-tz`/`iana-time-zone` with `jiff` (RUSTSEC-2026-0543, RUSTSEC-2026-0555). `calendar +agenda --today/--tomorrow` and the workflow standup/review day ranges now end at the next local midnight on DST-change days (previously off by an hour). The machine-local timezone fallback now honors the `TZ` env var.

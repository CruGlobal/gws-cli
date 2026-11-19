---
"gws-cli": patch
---

When `credentials.enc` cannot be decrypted, gws now stops with an error instead of deleting the file and silently falling back to other credentials (plaintext file or ADC), which could switch accounts. Run `gws auth login` to sign in again or `gws auth logout` to remove the file.

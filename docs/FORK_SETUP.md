# Fork setup & maintenance

This repository is a community-maintained fork of
[`googleworkspace/cli`](https://github.com/googleworkspace/cli), maintained by
[Cru](https://github.com/CruGlobal). This document captures the fork-specific
operational setup. See [`AGENTS.md`](../AGENTS.md) for build/test/contribution
conventions.

## Relationship to upstream

- `origin` → `git@github.com:CruGlobal/gws-cli.git`
- `upstream` → `git@github.com:googleworkspace/cli.git` (read-only; abandoned April 2026)

To pull a fix from upstream or an upstream PR branch:

```bash
git fetch upstream
git cherry-pick <sha>        # or: git merge upstream/main
```

## Required repository secret

Automated releases and skill-sync PRs need a token that can push tags/commits in
a way that **re-triggers** workflows (the default `GITHUB_TOKEN` cannot — tags it
pushes do not start the `Release` workflow).

Create a **fine-grained Personal Access Token** scoped to this repo with:

- **Contents: Read and write**
- **Pull requests: Read and write**

Add it as the repository secret **`RELEASE_TOKEN`** (Settings → Secrets and
variables → Actions). Used by `release-changesets.yml` and `generate-skills.yml`.

> A GitHub App token (via `actions/create-github-app-token`) works too and avoids
> tying releases to one person's PAT — switch to it if this becomes a bus-factor
> concern.

## CI smoketest credentials (`GOOGLE_CREDENTIALS_JSON`)

The **API Smoketest** job in `ci.yml` runs live Drive/Gmail/Calendar/Slides/pagination
checks. They are **skipped** when the `GOOGLE_CREDENTIALS_JSON` secret is absent
(offline `--help`/schema checks still run). To enable them:

1. **Dedicated test account.** Create a normal Workspace user in a *test* domain
   (no admin role needed — the checks only touch `me`/`primary` resources). With
   Gmail, Calendar, and Drive enabled for its OU.
2. **OAuth client.** `gws auth setup` creates the client in a GCP project and
   enables the APIs. Note which **project** it uses.
3. **Grant the bot `roles/serviceusage.serviceUsageConsumer` on that GCP
   project** — otherwise every call 403s with *"Caller does not have permission
   to use project …"* (user tokens bill quota to the OAuth client's project, and
   the bot isn't a member of it):
   ```
   gcloud projects add-iam-policy-binding <PROJECT> \
     --member="user:<bot>@<test-domain>" \
     --role="roles/serviceusage.serviceUsageConsumer"
   ```
4. **Log in read-only** as the bot: `gws auth login --readonly` (readonly scopes
   cover every smoketest call). Consent as the bot in the browser.
5. **Seed ≥2 Drive files** in the bot account — the pagination check pages at
   `pageSize:1` and asserts ≥2 pages, so a fresh empty Drive fails it. (Needs a
   one-time write-scoped login, e.g. `gws auth login --scopes .../auth/drive`,
   then `gws drive files create --json '{"name":"…","mimeType":"application/vnd.google-apps.folder"}'`.)
6. **Set the secret** (base64 of the exported `authorized_user` JSON — the CI job
   decodes it):
   ```
   gws auth export --unmasked | base64 | gh secret set GOOGLE_CREDENTIALS_JSON -R <org>/gws-cli
   ```

> **Token durability:** create the OAuth client in a GCP project **in the test
> domain's org with an Internal consent screen** so the refresh token doesn't
> expire. An *External + Testing* client issues tokens that die after 7 days,
> breaking CI weekly. Rotate by repeating steps 4 and 6.

## Branch protection

If you require status checks on `main`, only require checks that actually run in
this fork (e.g. `CI`, `Policy`). Do **not** require the upstream-only checks
`codecov/patch` or `cla/google` — they no longer run here and would block all
merges.

## Release flow (automated)

1. Every PR includes a changeset (`.changeset/*.md`, package name `"gws-cli"`).
   The `Policy` workflow enforces this for Rust/Cargo changes.
2. On merge to `main`, `release-changesets.yml` opens (or updates) a
   **"chore: release versions"** PR that bumps the version and updates the
   changelog.
3. Merging that PR pushes a `vX.Y.Z` tag (via `RELEASE_TOKEN`).
4. The tag triggers `release.yml`, which cross-compiles the 7 platform binaries,
   attaches them (as `gws-<target>.{tar.gz,zip}` + `.sha256`) to a GitHub
   Release, and signs them with build provenance attestations.

This fork does **not** publish to npm, crates.io, or Homebrew. Distribution is
via GitHub Release binaries, `cargo install --git`, or the Nix flake.

## Skills

`skills/` is generated from Google's Discovery Service by `generate-skills.yml`
(daily cron), which opens a sync PR when the API surface changes. Skills are not
published to any external registry.

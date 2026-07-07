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

## Claude PR automation

Claude Code reviews PRs, adapts to major dependency bumps, and gates the weekly
release (see [PR automation](#pr-automation-claude-code) below). This needs:

1. **Install the Claude GitHub App** — from a maintainer's terminal, run
   `claude /install-github-app` and select this repo.
2. **Add the secret `CLAUDE_CODE_OAUTH_TOKEN`** — a Claude Pro/Max subscription
   token (Settings → Secrets and variables → Actions). Usage bills to that
   subscription. An `ANTHROPIC_API_KEY` secret works instead if you'd rather bill
   the API; swap the `claude_code_oauth_token:` input for `anthropic_api_key:`.

> The Claude workflows pin `anthropics/claude-code-action@v1`. Keep it at
> **≥ v1.0.94** (pre-June-2026 tags had an env-exfiltration CVE). Repo convention
> is SHA-pinning — pin to the exact release SHA once confirmed.

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

## PR automation (Claude Code)

How each kind of PR reaches `main` (all merges gate on the required `CI Gate`
check — nothing merges on red CI):

| PR | Workflow | Behavior |
| --- | --- | --- |
| Dependabot patch/minor | `dependabot-automerge.yml` | Auto-merge, no review |
| Dependabot **major** | `claude-dependabot-major.yml` | Claude reads the changelog, adapts our code to breaking changes, verifies (`build`/`test`/`clippy`), pushes the fix, and enables auto-merge if confident; otherwise holds for a human |
| Skill-sync (`chore/sync-skills`) | `generate-skills.yml` | Auto-merge, no review |
| Release (`chore: release versions`) | `claude-release.yml` | Weekly (Mon 14:00 UTC): Claude assesses risk and auto-merges low-risk releases, else labels `needs-release-review` |
| Human-authored | `claude-review.yml` | Claude posts inline review comments; a human merges |

Security notes:

- `claude-review.yml` uses `pull_request` (safe on forks — the token is absent
  there, so it no-ops rather than exposing secrets to untrusted code).
- `claude-dependabot-major.yml` uses `pull_request_target` because Dependabot
  `pull_request` runs get a read-only token and no secrets. To adapt, Claude
  compiles the upgraded crate — which runs that crate's build scripts in a job
  holding `CLAUDE_CODE_OAUTH_TOKEN`. This is an accepted trade-off; `RELEASE_TOKEN`
  is deliberately kept out of that job, and every merge is still gated on
  `CI Gate`. Rotate `CLAUDE_CODE_OAUTH_TOKEN` if a bad dependency is suspected.
- `claude-release.yml` splits the model (risk assessment) and the privileged merge
  (`RELEASE_TOKEN`) into separate jobs, so the release token never sits in the job
  that runs Claude. The merge uses `RELEASE_TOKEN` so the push re-triggers the
  release chain (a `GITHUB_TOKEN` merge would not).

## Skills

`skills/` is generated from Google's Discovery Service by `generate-skills.yml`
(daily cron), which opens a sync PR when the API surface changes. Skills are not
published to any external registry.

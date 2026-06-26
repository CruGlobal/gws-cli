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

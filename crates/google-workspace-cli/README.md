# google-workspace-cli

**One CLI for all of Google Workspace — built for humans and AI agents.**

`gws` dynamically generates its command surface at runtime by reading Google's [Discovery Service](https://developers.google.com/discovery). Drive, Gmail, Calendar, and every Workspace API — zero boilerplate, structured JSON output, 40+ agent skills included.

## Install

Download the pre-built binary for your OS and architecture from the **[GitHub Releases](https://github.com/CruGlobal/gws-cli/releases)** page.

Or build from source:

```bash
cargo install --git https://github.com/CruGlobal/gws-cli --locked   # cargo
nix run github:CruGlobal/gws-cli                                    # nix
```

> This fork is not published to npm, crates.io, or Homebrew.

## Quick Start

```bash
gws auth login
gws drive files list --params '{"pageSize": 5}'
gws gmail users.messages list --params '{"maxResults": 3}'
```

## Documentation

See the [full README](https://github.com/CruGlobal/gws-cli#readme) for authentication setup, helper commands, agent skills, and more.

## License

Apache-2.0 — see [LICENSE](https://github.com/CruGlobal/gws-cli/blob/main/LICENSE).

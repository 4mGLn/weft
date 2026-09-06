# Weft

Weft is a local-first coordination layer for software changes created by humans and AI agents.

It treats a Change—not a branch, worktree, commit, or agent session—as the durable unit of work. Exact revisions can be assigned, materialized, composed, reviewed, validated, reconciled, and integrated across Native Git and GitButler providers.

## Project status

Weft has completed the provider feasibility spike, provider-neutral domain and persistence kernel, Native Git adapter, GitButler adapter, stable local CLI, and provider-neutral agent protocol. The first deployable artifact is a local CLI archive. Hosted deployment remains out of scope.

## Install

Linux x86_64 (portable musl build), macOS Intel, and macOS Apple Silicon:

```bash
curl -fsSL https://github.com/4mGLn/weft/releases/latest/download/install.sh | sh
```

Windows x86_64:

```powershell
irm https://raw.githubusercontent.com/4mGLn/weft/main/install.ps1 | iex
```

The Unix bootstrapper is downloaded from the latest published GitHub Release,
then downloads the matching runtime archive and verifies its published SHA-256
digest before installing. To select an exact release, use that release's
`install.sh` asset and pass its version to `sh`, for example
`curl -fsSL https://github.com/4mGLn/weft/releases/download/vMAJOR.MINOR.PATCH/install.sh | WEFT_VERSION=vMAJOR.MINOR.PATCH sh`.
Unix installations default to `$HOME/.local`; set `PREFIX` to override it.
Windows installations default to `%LOCALAPPDATA%\\Weft`; set `WEFT_PREFIX` to
override it. A platform is installable once its matching release asset is
published.

## Start here

After installation, wire Weft to the agent tools already used by a repository:

```bash
cd your-project
weft setup
weft doctor
```

Users then keep working through their usual agent runtime or orchestrator. Weft
coordinates the agents' shared durable state underneath it; it does not replace or
schedule them. See [Runtime wiring](docs/RUNTIME_WIRING.md) for the supported
surfaces and boundaries.

- [Product goal](docs/GOAL.md)
- [Normative domain model](docs/DOMAIN.md)
- [Implementation roadmap](docs/ROADMAP.md)
- [Development guide](docs/DEVELOPMENT.md)
- [Deployment and release policy](docs/DEPLOYMENT.md)
- [Runtime wiring](docs/RUNTIME_WIRING.md)
- [Agent instructions](AGENTS.md)

## Development

```bash
make check
```

Provider evidence and the local CLI can be run with:

```bash
make phase0-native-git-spike
make phase0-gitbutler-spike
cargo run -p weft-cli -- --help
```

The domain kernel, canonical artifact store, SQLite store, provider adapters, and CLI are verified by `make check` and live under `crates/`.

For contributor workflow and verification requirements, follow [AGENTS.md](AGENTS.md).

## Architecture at a glance

```text
Agent / Human / Orchestrator
             │
             ▼
           Weft
 Change · Revision · Candidate
 Review · Conflict · Integration
             │
       ┌─────┴─────┐
       ▼           ▼
   Native Git   GitButler
```

Weft records and coordinates work. External systems run agents.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) before proposing changes. Report vulnerabilities according to [SECURITY.md](SECURITY.md).

The repository is public, but no software license has been selected yet. Public visibility alone does not grant permission to use, modify, or redistribute the work.

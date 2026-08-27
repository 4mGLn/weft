# Weft

Weft is a local-first coordination layer for software changes created by humans and AI agents.

It treats a Change—not a branch, worktree, commit, or agent session—as the durable unit of work. Exact revisions can be assigned, materialized, composed, reviewed, validated, reconciled, and integrated across Native Git and GitButler providers.

## Project status

Weft has completed the provider feasibility spike, provider-neutral domain and persistence kernel, Native Git adapter, GitButler adapter, stable local CLI, and provider-neutral agent protocol. The first deployable artifact is a local CLI archive built and smoke-tested on Ubuntu 24.04 x86_64. Hosted deployment remains out of scope.

## Start here

- [Product goal](GOAL.md)
- [Normative domain model](DOMAIN.md)
- [Implementation roadmap](ROADMAP.md)
- [Development guide](docs/DEVELOPMENT.md)
- [Deployment and release policy](docs/DEPLOYMENT.md)
- [Agent protocol](docs/AGENT_PROTOCOL.md)
- [Paseo integration](docs/PASEO.md)
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

Material changes should start from the [task record template](docs/agent-harness/task-template.md), use the [verification matrix](docs/agent-harness/verification-matrix.md), and record durable design decisions under `docs/decisions/`.

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

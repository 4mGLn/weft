# ADR-0013: Agent protocol and local runtime release

- **Status:** Accepted
- **Date:** 2026-08-27

## Context

The stable CLI makes Weft usable by humans and agent runtimes, but orchestration and deployment boundaries must be explicit before publishing a runtime artifact. Three existing EZIS repositories supplied useful patterns: concise routed agent context and specialist reviews, automated repository-rule checks, supervised workspace commands, change-aware CI, and artifact-first release verification. Their product-specific hooks, database/module branch rules, self-hosted runners, production promotion branches, and frontend deployment topology do not match Weft.

## Decision

The `weft` JSON CLI is the provider-neutral agent protocol v1. Orchestrators invoke it as a process, provide durable operation IDs and optimistic-concurrency inputs, and persist only Weft identifiers—not agent session IDs—as workflow authority. `docs/AGENT_PROTOCOL.md` owns the operation and error mapping. Paseo is the first documented orchestrator integration, but remains a launcher and supervisor; it does not become a Weft provider or durable state store.

The repository harness remains the single local/CI gate. Agent guidance stays concise and routed through `AGENTS.md`; deterministic rules are checked by scripts rather than copied into prompts. Project specialists stay bounded, read-only reviewers. No editor-specific pre-write hook becomes correctness evidence.

The first runtime release is a relocatable archive built and smoke-tested on Ubuntu 24.04 x86_64, containing the `weft` binary, install/uninstall helpers, contracts, and notices. Other distributions and glibc versions are not supported claims. It is a local CLI, not a daemon or hosted service. The state directory is caller-selected and retained across upgrade, rollback, and uninstall. Native Git requires Git 2.38 or newer. GitButler remains an optional, explicitly capability-gated development adapter requiring exactly `but 0.22.0`.

Tags matching semantic version `vMAJOR.MINOR.PATCH` build from the tagged source, run the full gate, smoke-test installation from the archive, emit SHA-256 checksums and a deterministic CycloneDX inventory, and publish through GitHub Releases. GitHub artifact attestations provide build provenance. Artifact signing, vulnerability scanning, additional operating systems/architectures, package-manager distribution, auto-update, services, and hosted deployment remain unsupported until separately evidenced.

## Consequences

- Any runtime can integrate through the same stable process contract and resume from durable state after its own session ends.
- Paseo workspaces map to Materializations; Paseo agents map to Assignment holders, never Change identity.
- Release claims are intentionally narrower than the Rust code's possible build targets.
- Public source visibility does not change the repository's `UNLICENSED` status.

## Required proof

- Agent-protocol command/error documentation and process-restart tests.
- A clean temporary install from the exact release archive, version/help checks, state initialization, restart readback, checksum verification, and uninstall-with-state-retention proof.
- CI and release workflows with least privilege, concurrency controls, immutable tag identity, and artifact attestation.
- Read-only domain, provider, and release review as applicable, followed by the full repository gate.

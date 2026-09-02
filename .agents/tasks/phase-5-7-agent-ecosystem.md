# Task Record: Phase 5–7 agent ecosystem

## Outcome and scope

- **Target result:** A provider-neutral agent protocol, Paseo mapping, multi-agent operating model, and verified local runtime release fitted to Weft's durability and deployment boundaries.
- **Reference evidence:** G-EZIS agent harness/specialists and release gates; monitoring-front rule hooks, affected builds, and promotion automation; EZIS Nexus routed context, rule checker, supervised harness, artifact matrices, and release runbook.
- **Adopted:** concise source routing, deterministic enforcement, specialist read-only review, one local/CI gate, explicit supervised commands, artifact smoke testing, least privilege, concurrency, checksum/provenance, install/rollback/uninstall guidance.
- **Rejected as non-fitting:** product-specific source hooks, DBMS/module branch isolation, frontend production branches, self-hosted runner assumptions, multi-edition/platform claims, embedded scheduling, and copied prompt/rule trees.
- **Affected invariants:** durable identity, exact targeting, assignment/lease authority, canonical resume, uncertain integration recovery, and scheduler separation.

## Acceptance criteria

1. Agent operations and error responses are documented over `weft.cli.v1` without provider/runtime identity leakage.
2. Session termination and replacement resume from durable canonical revision state, not a dirty workspace.
3. Paseo workspaces/agents/scripts map explicitly to Materializations/Assignments/Validations while process scheduling stays external.
4. Dependency-aware multi-agent review, validation, resolution, and integration ordering preserves exact candidates and reconciliation.
5. An Ubuntu 24.04 x86_64 archive has reproducible assembly, checksum, clean-prefix install, restart, state-retaining uninstall, and documented rollback boundaries.
6. CI and tag releases use least privilege, concurrency controls, exact tag/version identity, archive smoke proof, and build provenance.
7. Full repository gate and read-only release review pass; unsupported deployment claims remain explicit.

## Proof record

| Check | Command | Result |
| --- | --- | --- |
| Harness/document contracts | `make harness-check docs-check` | Passed |
| Runtime archive | `make package-release VERSION=v0.1.0` | Passed; binary version and target bound |
| Clean install/restart/uninstall | `make test-release ARCHIVE=dist/weft-0.1.0-x86_64-unknown-linux-gnu.tar.gz` | Passed; archive/SBOM checksums and retained SQLite state verified |
| Full repository gate | `CARGO_NET_OFFLINE=true make check` | Passed; 129 active tests, five ignored helpers, strict Clippy |
| Domain/release review | `domain_reviewer`, `release_reviewer` | Passed after expired-lease recovery and checksum-denial clarifications; no remaining finding |

## Residual risk and unavailable evidence

- No earlier public runtime exists for a real cross-version upgrade/rollback test.
- The CycloneDX inventory and its GitHub provenance attestation are supported. Artifact signing, vulnerability scanning, non-Ubuntu platforms, package managers, auto-update, remote providers, services, containers, and hosted control planes remain unsupported.
- The repository remains `UNLICENSED`; public visibility grants no additional rights.

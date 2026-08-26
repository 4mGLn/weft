# Task Record: Phase 1 domain kernel

## Outcome and scope

- **Result:** Executable Rust domain types enforcing linear revision-head CAS and canonical `tree-delta-v1` manifest invariants.
- **In scope:** Change/Revision identity, append semantics, base/artifact references, path operations, and focused tests.
- **Out of scope:** SQLite persistence, CAS blob I/O, providers, CLI, assignments, candidates, and integrations.
- **Affected invariants:** `DOMAIN.md` sections 1–2 and ADR-0002.

## Acceptance criteria

1. A root revision and one successor form a linear immutable sequence.
2. A stale expected head and duplicate revision ID fail without mutating the Change.
3. Canonical manifests reject empty, duplicate, unsorted, traversal, and content-less operations.
4. `make check` passes formatting, tests, Clippy, harness, and documentation gates.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Domain unit tests | `cargo test -p weft-domain --target x86_64-unknown-linux-gnu` | Passed | 8 tests on 2026-08-26 |
| Workspace gate | `make check` | Passed | Harness, links, format, tests, and strict Clippy on 2026-08-26 |

## Follow-up

Add SQLite migrations and transactional repositories that persist these invariants and prove multi-process stale-head behavior.

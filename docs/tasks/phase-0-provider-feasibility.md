# Task Record: Git and GitButler provider feasibility

## Outcome and scope

- **Result:** Evidence-backed provider capability matrix, executable spike, and implementation recommendations required by Roadmap Phase 0.
- **In scope:** canonical revisions, materializations, stacks/candidates, integration attempts, conflicts, and reconciliation across Native Git and GitButler.
- **Out of scope:** production domain storage, stable CLI, hosted service, or runtime deployment.
- **Affected invariants:** all provider-facing invariants in `DOMAIN.md`.

## Acceptance criteria

1. Every Phase 0 experiment in `ROADMAP.md` has reproducible commands and recorded results.
2. Unsupported and provider-specific behavior is explicit in a capability matrix.
3. An ADR recommends language, storage, canonical artifact representation, and adapter boundaries without weakening domain guarantees.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Harness baseline | `make check` | Pending | |
| Native Git spike | | Pending | |
| GitButler spike | | Pending | |
| Crash/reconciliation | | Pending | |

## Follow-up

This task becomes active after repository bootstrap. Results update `.agent/PROGRESS.md` and produce the next ADRs.

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

## Native Git evidence (2026-08-26)

`scripts/phase0-native-git-spike.sh` creates disposable repositories and proves:

- a binary-capable patch against an exact base reconstructs the recorded tree after
  the original branch reference is deleted;
- a rewritten provider commit does not mutate the prior canonical artifact;
- an ordered two-revision candidate reconstructs its exact composed tree;
- an integration plan detects a changed target before execution;
- a conflicting merge exposes unresolved paths; and
- a recorded provider ref diverges after an external commit and can therefore be
  reconciled rather than silently accepted.

Run it with `make phase0-native-git-spike`. The test is self-contained, disables
commit signing only in temporary repositories, and removes them when it exits.

Git version recorded by the successful run: 2.43.0.

## GitButler evidence (2026-08-26)

`scripts/phase0-gitbutler-spike.sh` uses the GitButler CLI (`but`) to create a
disposable GitButler project and proves:

- a virtual branch has a stable GitButler change ID;
- a `but amend` rewrites its provider commit ID while preserving that change ID,
  including the identity of a dependent stacked change;
- an explicitly anchored branch forms an ordered stack;
- a second virtual branch can coexist in the workspace as a separate stack, then
  be moved into the first stack; and
- `but land --whole-stack --yes` advances the configured local target.

Run it with `make phase0-gitbutler-spike`. It requires access to GitButler's
local project registry and removes the disposable repository on exit. The
successful run used `but 0.22.0`.

The extended spike proves conflict mapping and external-provider reconciliation
for the tested local workflow: it advances the
target outside GitButler, runs `but pull`, verifies the observed merge base, preserves
the Change ID, and asserts the rebased commit is marked conflicted.

## Deliverables

- [Technical report](../phase0/provider-feasibility-report.md)
- [Provider capability matrix](../phase0/provider-capability-matrix.md)
- [ADR-0002 implementation foundation](../decisions/0002-implementation-foundation.md)

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Harness baseline | `make check` | Passed | 2026-08-26 local run |
| Native Git spike | `make phase0-native-git-spike` | Passed | 2026-08-26 local run; Git 2.43.0 |
| GitButler virtual branches/stacks/landing | `make phase0-gitbutler-spike` | Passed | 2026-08-26 local run; `but 0.22.0` |
| GitButler conflict/reconciliation | `make phase0-gitbutler-spike` | Passed | External target advance retained Change ID and produced conflicted commit |
| Crash/reconciliation | Native Git and GitButler external-target divergence cases | Partial | Divergence/reconciliation passed; crash injection remains a Phase 1/adapter gate |

## Follow-up

Phase 0 is complete for architecture selection. Begin Phase 1 with the domain kernel,
transactional schema, canonical artifact contract, and required crash/compatibility
tests from ADR-0002. Unproven GitButler removal and uncertain-land recovery remain
explicit adapter gates rather than assumed capabilities.

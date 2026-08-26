# Project Progress

## Current phase

Phase 1 — Persistence and domain kernel.

## Completed

- Defined product goal, normative domain model, and implementation roadmap.
- Resolved revision identity, canonical content, exact composition, review targeting, and integration semantics.
- Established the evidence-driven development, review, CI, and specification-release harness.
- Added a passing Native Git Phase 0 spike for canonical reconstruction, provider
  rewrite survival, candidate composition, target guarding, conflict capture, and
  external-ref reconciliation.
- Added a passing GitButler Phase 0 spike for virtual-branch identity across
  rewrites, parallel and stacked changes, local whole-stack landing, conflicted
  commits, and external-target reconciliation.
- Completed the Phase 0 capability matrix, technical report, and implementation
  foundation decision (Rust, SQLite metadata, filesystem CAS, capability adapters).
- Started Phase 1 with a compiling Rust workspace and sealed domain types for
  linear revision-head CAS and canonical `tree-delta-v1` manifests; all eight
  focused tests and the strict workspace gate pass.

## Next checkpoint

Add SQLite migrations and transactional repositories for Change and ChangeRevision,
then prove stale-head behavior across independent connections/processes.

## Known gaps

- No runtime implementation, storage engine, or stable CLI.
- GitButler provider removal/reconnect and crash-uncertain landing remain unproven
  implementation gates.
- No runtime packaging or deployment support.
- No software license selected.

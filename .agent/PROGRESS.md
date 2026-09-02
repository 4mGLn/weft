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
- Implemented durable Phase 1 Change/Revision foundations: deterministic
  base-bound canonical artifacts, verified filesystem CAS blobs/manifests, and
  SQLite WAL persistence with transactional head compare-and-swap.
- Proved canonical artifact reopen and independent-connection competing-writer
  behavior; all fifteen focused tests and the strict workspace gate pass.
- Added SQLite-backed exclusive leases with expiry recovery and ordered audit
  events for Change creation, revision appends, and lease acquisition.

## Next checkpoint

Extend the transactional repository to relationships and immutable
CompositionCandidates.

## Known gaps

- No provider runtime implementation or stable CLI.
- GitButler provider removal/reconnect and crash-uncertain landing remain unproven
  implementation gates.
- No runtime packaging or deployment support.
- No software license selected.

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
- Added durable exact-revision dependencies with atomic cycle rejection and
  immutable CompositionCandidates with ordered inputs, dependency snapshots,
  deterministic digests, restart reload, and explicit stale detection.
- Added durable assignment-event history and exact-revision materialization
  records with guarded, auditable lifecycle transitions.
- Added durable exact-target review requests/submissions and validation results;
  these reject nonexistent targets and never accept mutable Change identities.
- Added durable integration attempts with operation-ID idempotency, fresh-candidate
  planning, expected-target guarding, required operation leases, terminal state
  validation, and receipts required for success.
- Added durable, ordered, duplicate-free Stack versions with optimistic updates
  and immutable historical reads.
- Added durable change relationships, exact-revision overlap signals, candidate
  stack-version provenance, exact-target validation and review-history reads,
  and revision/candidate staleness projection.
- Added lease renewal/release, conflict and reconciliation evidence, and
  operation-ID retry behavior that returns the durable integration attempt rather
  than duplicating provider work.
- Made integration start, conflict capture, and terminal receipt persistence
  transactionally guarded; integration start automatically emits complete domain
  evidence in the same transaction and is proven across independent connections.

## Next checkpoint

Close the remaining complete-audit, explicit-reuse, and interrupted-operation
recovery semantics before the Phase 1 acceptance review.

## Known gaps

- No provider runtime implementation or stable CLI.
- Phase 1's complete automatic audit contract is not yet implemented for all
  correctness-sensitive transitions; Phase 2 provider mutation remains gated on it.
- GitButler provider removal/reconnect and crash-uncertain landing remain unproven
  implementation gates.
- No runtime packaging or deployment support.
- No software license selected.

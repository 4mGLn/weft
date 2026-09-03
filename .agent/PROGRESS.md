# Project Progress

## Current phase

Phase 3 — GitButler provider.

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
- Completed Phase 1 acceptance evidence: every persisted mutation emits durable
  domain evidence, retrying an operation ID resumes its exact attempt, and a
  running integration survives restart for explicit reconciliation without
  duplicate effects.
- Completed Phase 2 Native Git provider acceptance: discovery, canonical capture,
  detached materialization, overlap detection, conservative composition, guarded
  target CAS, durable receipts/conflicts, and restart-safe reconciliation are
  proven through the reusable API.
- Added a version-gated GitButler provider adapter for the Phase 0-supported
  CLI/status schema: target inspection and reconciliation, virtual-branch
  creation and rewrite observation, anchored stacks, and whole-stack landing.
- Normalized GitButler virtual-branch provider references as logical change ID,
  exact commit ID, and conflict state; the supported status projection is tested
  against multiple branches and normal JSON whitespace.
- Added canonical GitButler branch export through the established Native Git
  artifact boundary. GitButler IDs remain supplemental provider references while
  the artifact retains only exact base-bound canonical content.

## Next checkpoint

Implement durable GitButler integration execution with an exact observed target,
verified receipt or explicit conflict/uncertain outcome, and restart-safe
reconciliation. Add a reproducible provider fixture that proves canonical export
and recovery against a live supported `but` CLI.

## Known gaps

- No provider runtime implementation or stable CLI.
- GitButler provider removal/reconnect, durable landing receipts, and
  crash-uncertain landing remain unproven implementation gates.
- No runtime packaging or deployment support.
- No software license selected.

# Task Record: Phase 1 Integration, Conflict, and Recovery Kernel

## Outcome and scope

- **User/operator result:** Weft can durably plan exact candidate integration, grant recoverable target-scoped execution authority, preserve uncertainty/conflicts, reconcile provider observations, and issue one immutable receipt only after verified success.
- **In scope:** IntegrationAttempt identity/version/state, candidate/target/provider/strategy intent, stable effect-operation ID, gate/capability evidence, referenced review/validation evidence, target observations, target-scoped execution leases, transition events, IntegrationConflict, ConflictResolution, ReconciliationObservation, IntegrationReceipt, global metadata-operation replay, schema v7 migration, restart/concurrency/drift proof.
- **Out of scope:** Calling Git/GitButler, provider subprocess deadlines/redaction/version gates, actual merge/land/apply behavior, provider-specific evidence verification, repository-policy engine, Change lifecycle aggregation, remote branch protection, CLI/API schemas, signing/attestation, distributed leases.
- **Affected domain invariants:** `DOMAIN.md` sections 5, 6, 8–10; GOAL success criteria 10–12; ADR-0002, ADR-0004, ADR-0007, and ADR-0008.
- **Provider/runtime scope:** Provider-neutral local SQLite WAL and filesystem CAS; provider observations/evidence are inputs from future capability adapters, not provider calls in this checkpoint.
- **Compatibility surface:** Domain state machine, effect-operation semantics, lease/observation/receipt fields, storage API, global operation idempotency, migration behavior.

## Acceptance criteria

1. Attempt identity and repository/candidate/target/provider/strategy/effect-operation intent are immutable; effect-operation IDs are globally unique per attempt and distinct from metadata transition IDs.
2. Planning requires a current authoritative candidate whose exact target base equals expected target, exact ordered inputs, explicit gate-policy and provider-capability evidence, current Approved/Passed referenced evidence within the candidate, and an observed target equal to expected.
3. Starting uses exact-version CAS, revalidates candidate/gates, rejects changed observed target, and atomically creates a non-expired execution lease scoped to exact `(repository, target_ref)`.
4. Only the current lease holder may renew or report execution outcomes; lease expiry/release/loss never authorizes blind re-execution or a success/failure claim.
5. Running may enter Reconciling for uncertain effect; uncertainty, divergence, and repeated reconciliation observations preserve exact intent and never emit a receipt. Exact Diverged evidence may explicitly terminally Supersede the attempt before a new candidate/attempt targets the observed revision.
6. Conflicted transition atomically records one immutable IntegrationConflict with exact attempt/candidate/inputs/provider evidence. ConflictResolution is separate, exact-target/validation-backed, and never rewrites or resumes the conflicted attempt.
7. Failed/Aborted after uncertain execution requires a NoEffectVerified reconciliation observation. Succeeded requires a matching ResultVerified observation or direct verified target observation and atomically emits exactly one immutable receipt.
8. Receipt candidate/attempt/prior/result/provider/effect-operation/evidence/provenance are immutable; no receipt exists for any non-Succeeded state, and Succeeded cannot exist without its matching receipt.
9. Exact metadata-operation retries return historical outcomes after later transitions; cross-kind/payload conflicts fail. Replanning uses new attempt and effect-operation identity.
10. Authoritative reads reconstruct contiguous state events, leases, observations, conflicts/resolutions, and receipts; source canonical content, candidate/evidence refs, operation provenance, and projection/event agreement are revalidated; drift fails closed.
11. Fresh and v1–v6 databases reach schema v7 under serialized concurrent opens without losing review/validation histories; focused domain/provider-neutral storage, domain-review, workspace, strict Clippy, harness, and documentation gates pass.

## Risks

- **Safety:** Treating timeout/lease expiry as failure could duplicate an external mutation. Uncertainty must funnel to reconciliation.
- **Authority:** Existing Change-operation leases cannot safely represent a multi-Change candidate target; integration authority is target-scoped and recoverable.
- **Provider trust:** Opaque evidence is durable but not yet adapter-verified. This checkpoint proves state semantics, not that a provider observation is truthful.
- **Policy:** Gate and capability attestations are explicit evidence; sufficiency remains a repository/provider policy decision, not an inferred approval rule.
- **Upgrade/rollback:** v7 is additive but older binaries reject it. Rollback uses a pre-migration backup.

## Evidence and plan

- Relevant sources: `GOAL.md`, `DOMAIN.md`, ROADMAP Phases 0–2, ADR-0002/0004/0007/0008, Phase 0 provider report/matrix, domain/storage kernels.
- Required proof: verification-matrix Integration row plus read-only domain review; provider execution remains explicitly unavailable.

1. Freeze state, lease, observation, conflict/resolution, reconciliation, and receipt types.
2. Add schema v7 versioned attempts and immutable recovery/evidence records.
3. Prove target CAS, lease loss, uncertainty, conflict, reconciliation, verified receipt, replay, migration, restart, and drift.
4. Run domain review, resolve findings, record ADR/evidence, and execute the full repository gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-domain`; `cargo test -p weft-storage-sqlite` | Passed | 41 domain tests; 54 active storage tests, 3 process helpers intentionally ignored in the direct crate run |
| Domain/contract | Integration state/authority/conflict/resolution/receipt tests and read-only domain review | Passed | Reviewer findings for unresolved-target exclusion, start-time freshness, historical replay, divergence liveness, and orphan terminal evidence were resolved and re-proved |
| Concurrency/recovery | Target CAS, unresolved target exclusion, lease expiry, uncertainty/divergence, historical replay, v6 migration, receipt drift | Passed | Integration tests include expiry/reconciliation/receipt, stale-plan rejection, exact conflict resolution, and Diverged-to-Superseded replacement; concurrent v6 upgrade is covered |
| Provider integration | Unavailable | No provider mutation in this checkpoint; Phase 0 behavior is context, not current proof |
| Static/harness | `CARGO_HOME=… CARGO_NET_OFFLINE=true make check` | Passed | Harness/docs/fmt; 106 active workspace tests; spawned-process proofs; warning-free strict Clippy |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0009 accepts target-scoped authority, mandatory reconciliation, separate conflict resolution, and verified immutable receipts; it rejects expiry-as-failure, blind retry, mutable conflict repair, and exit-status-only receipts.
- **Residual risks:** Provider adapter truth, crash injection around real mutation, GitButler uncertain landing, external policy, authorization, distributed authority, and signed receipts remain open.
- **Unavailable evidence:** Live Native Git/GitButler mutation, real target locks, remote policies, network filesystems, distributed writers, and cryptographic attestations are not claimed.
- **Follow-up:** Implement Phase 2 Native Git provider execution and reconciliation against this kernel, including bounded/redacted subprocess behavior and crash injection.

# Task Record: Phase 1 Stacks and CompositionCandidates

## Outcome and scope

- **User/operator result:** Weft can version mutable Stack topology and atomically resolve it—or an explicit Change order—into one immutable, digest-verified exact target for later review, validation, and integration.
- **In scope:** Stack identity/policy/order/version, full-snapshot events, exact-version definition replacement, `composition-candidate-v1` canonical bytes/digest, current-head resolution, canonical-content/repository checks, explicit and Stack-predecessor requirements, dependency/order validation, immutable candidate persistence, derived staleness, operation replay, schema v5 migration, restart/concurrency/drift proof.
- **Out of scope:** Review/ValidationResult entities, provider target observation, actual composition/application, conflicts, IntegrationAttempt, CLI/API schemas, authorization, signing, and distributed coordination.
- **Affected domain invariants:** `DOMAIN.md` sections 1–4, 8, and 10; GOAL success criteria 7–9; ADR-0002, ADR-0003, and ADR-0006.
- **Provider/runtime scope:** Provider-neutral local SQLite WAL and filesystem CAS; no provider mutation.
- **Compatibility surface:** Domain API, candidate digest bytes, SQLite schema/storage API, global operation idempotency, and migration behavior.

## Acceptance criteria

1. Stack definitions are non-empty, duplicate-free explicit predecessor chains with `order_only` or `predecessor_dependencies` policy; identity/provenance remain immutable.
2. Definition replacement uses exact Stack-version CAS, records the complete resulting definition, rejects stale/no-op/time reversal, and exact retries return historical outcomes after later changes.
3. Candidate creation uses one metadata snapshot, exact expected Stack version or explicit Change order, current exact heads, durable canonical content, and one repository matching the exact target base.
4. Every active dependency for a selected downstream input is current, included as an exact ID/version/pin resolution, and points to an upstream input earlier in order; missing/reversed/stale requirements fail atomically.
5. `predecessor_dependencies` adds exact direct-predecessor requirements without creating durable Dependency rows; `order_only` does not infer them.
6. `composition-candidate-v1` deterministically hashes all correctness-bearing fields and excludes candidate ID/creator/time; identical composition has the same digest, any exact input/topology/requirement change has a different digest.
7. Candidate identity, fields, ordered inputs, requirements, digest, provenance, and creation event are immutable; authoritative reads revalidate exact content, structure, bytes/digest, and fail closed on drift.
8. Candidate freshness separately reports advanced inputs, changed/removed dependencies, and changed Stack version without mutating the candidate; provider target freshness remains explicitly unknown.
9. Fresh and v1–v4 databases reach schema v5 under serialized concurrent opens without losing existing histories; focused, domain-review, workspace, strict Clippy, harness, and documentation gates pass.

## Risks

- **Data/security:** Candidate encoding is bounded and length-prefixed; identifiers and digests are validated. No provider source or secrets enter candidate metadata.
- **Concurrency/crash recovery:** Immediate transactions serialize Stack revisions, candidate resolution, operation registration, and immutable event publication.
- **Provider divergence/compatibility:** Candidate target base is exact but live target comparison is deferred to provider planning/reconciliation.
- **Performance/resource limits:** Resolution and freshness scan selected histories/dependencies; explicit limits and graph indexes must bound hostile or accidental expansion.
- **Upgrade/rollback:** v5 is additive but older binaries reject it. Rollback uses a pre-migration backup.

## Evidence and plan

- Relevant sources: `GOAL.md`, `DOMAIN.md`, `ROADMAP.md`, ADR-0002/0003/0006, `crates/weft-domain`, `crates/weft-artifact`, and `crates/weft-storage-sqlite`.
- Required proof: verification-matrix dependency/stack/candidate row plus read-only domain review.

1. Freeze Stack and candidate types plus canonical digest bytes.
2. Add schema v5 projections/snapshots and immutable candidates.
3. Resolve candidates atomically and prove ordering, dependencies, staleness, replay, migration, and drift behavior.
4. Run domain review, resolve findings, record ADR/evidence, and execute the full repository gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-domain -p weft-storage-sqlite --target x86_64-unknown-linux-gnu --offline` | Passed | 30 domain and 42 active storage tests; 3 process helpers intentionally ignored at top level and exercised by parent tests |
| Domain/contract | Stack/Candidate state, digest, ordering, provenance, replay, and drift tests; read-only domain review | Passed | Initial four findings resolved; re-review reported no actionable findings |
| Concurrency/recovery | Immediate-transaction resolution, Stack stale CAS, historical replay, restart, finalized snapshots, concurrent populated v4 upgrade | Passed | Focused storage suite and full gate |
| Provider integration | Not applicable | No provider mutation in scope |
| Static/harness | `CARGO_HOME=<isolated-cache> CARGO_NET_OFFLINE=true make check`; `git diff --check` | Passed | Harness/docs/fmt, 83 active workspace tests, spawned-process helpers, strict Clippy; clean diff whitespace |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0007 accepts versioned full-snapshot Stacks, one-snapshot exact candidate resolution, and identity-independent canonical digests; mutable review targets, split-snapshot resolution, inferred durable Stack dependencies, and persisted stale flags are rejected.
- **Residual risks:** Live target freshness, review/validation, actual composition, graph scale, backup tooling, and distributed coordination remain open.
- **Unavailable evidence:** Provider target observation, Native Git/GitButler composition, network filesystems, distributed writers, and signed candidate attestations are not claimed.
- **Follow-up:** Continue Phase 1 with exact-target ReviewRequest and ValidationResult persistence.

# Task Record: Durable dependencies and composition candidates

## Outcome and scope

- **User/operator result:** Weft persists exact-revision dependency contracts and immutable, ordered composition candidates across restart.
- **In scope:** Dependency cycle detection, exact pin resolution, candidate ordering, deterministic digest, stale detection, SQLite schema v3, and audit events.
- **Out of scope:** Mutable stacks, materializations, assignments, reviews, validations, integrations, and provider runtime operations.
- **Affected domain invariants:** Dependencies are explicit, durable, directed, acyclic, and exact-pinned; candidates are immutable exact snapshots whose ordering is validated; later revisions make affected candidates stale without mutation.
- **Provider/runtime scope:** Provider-neutral storage only.
- **Compatibility surface:** API | schema | storage | artifact

## Acceptance criteria

1. Adding a dependency atomically rejects cycles and pins an upstream revision belonging to its declared Change.
2. Candidate creation persists an ordered, duplicate-free exact input snapshot and all resolved dependency pins.
3. Candidate loading after restart preserves the snapshot and digest; upstream/input advancement marks it stale without changing it.

## Risks

- Data/security: Persisted identifiers are decoded through bounded domain constructors; candidate digest detects row tampering.
- Concurrency/crash recovery: Dependency and candidate writes use immediate SQLite transactions; the CAS owns canonical content independently.
- Provider divergence/compatibility: Target-base freshness remains a provider/integration concern.
- Performance/resource limits: Candidate validation is linear in its inputs plus declared dependencies; graph traversal is recursive SQL and should be benchmarked for large graphs before public scaling claims.
- Upgrade/rollback: Schema v3 is forward-version guarded; no downgrade path exists yet.

## Evidence and plan

- Relevant paths, symbols, decisions, and tests: `crates/weft-domain/src/storage.rs`, `crates/weft-domain/src/change.rs`, `DOMAIN.md` sections 2 and 8, `docs/agent-harness/verification-matrix.md`.
- Reproduction or baseline: Phase 1 durable revision/CAS/lease storage was already passing on schema v2.
- Official version-sensitive evidence: Not applicable; this is local provider-neutral domain behavior.
- Required decision/documentation updates: Progress ledger updated; public API remains pre-release.

1. Add dependency and candidate schema/API — proof: focused storage tests.
2. Validate exact pins, cycles, ordering, and stale snapshots — proof: negative-path tests.
3. Run strict repository checks — proof: commands recorded below.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-domain` | Passed | 18 tests, including candidate restart, stale, cycle, missing-pin, and ordering cases. |
| Domain/contract | `cargo test -p weft-domain` | Passed | Exact snapshots, immutable reload, deterministic order-sensitive digest. |
| Concurrency/recovery | `cargo test -p weft-domain` | Passed | Candidate restart round-trip; existing independent-writer CAS test remains green. |
| Provider integration | Not run | Unavailable by scope | Provider runtime is Phase 2+. |
| Static/harness | `cargo clippy --workspace --all-targets -- -D warnings` | Passed | Strict lint clean. |
| Package/deployment | Not run | Unavailable by scope | Runtime packaging is later work. |

## Decision and follow-up

- **Decision and alternatives rejected:** Candidates snapshot dependency rows instead of resolving heads on read; current heads are consulted only by explicit stale detection. Rejected mutable candidate inputs because they would invalidate exact review/integration targeting.
- **Residual risks:** Stack versioning, target-base reconciliation, actor/time metadata, and audit event completeness remain unimplemented.
- **Unavailable evidence:** Large dependency graph performance and provider materialization/integration behavior.
- **Follow-up, owner, resumption condition:** Primary agent continues Phase 1 with durable assignments/materializations, then reviews, validations, operation/idempotency, and integration records.

# Task Record: Phase 1 assignments and leases

## Outcome and scope

- **User/operator result:** Responsibility survives handoff and restart, while one correctness-sensitive Change operation can be held by only one non-expired lease at a time.
- **In scope:** Assignment tenures and release, typed subjects/roles, operation-scoped leases, acquire/renew/release/reclaim, optimistic versions, immutable histories, global operation IDs, schema v2 migration, and multi-process recovery.
- **Out of scope:** Authorization policy, hosted/distributed lease authority, automatic agent scheduling, wall-clock injection at an untrusted API boundary, materializations, and provider operations.
- **Affected domain invariants:** `DOMAIN.md` sections 1, 3, 7, 8, and 10; ADR-0002 and ADR-0004.
- **Provider/runtime scope:** Local same-host SQLite WAL; no provider calls.
- **Compatibility surface:** Domain API, SQLite schema/storage API, operation idempotency, audit history.

## Acceptance criteria

1. Typed Assignment identity, subject kind, role, provenance, and version reject invalid values and stale or repeated release transitions.
2. Different assignments overlap; exact active duplicates fail; release and reopen preserve both projection and immutable events.
3. A lease is exclusive per Change/operation scope, and every acquire/renew/release uses exact scope-version compare-and-swap.
4. Active competitors fail, renewal strictly extends expiry, release clears authority, and equality at expiry permits reclaim under a new identity linked to its predecessor.
5. Expired authority can be reclaimed after the original process exits; no assignment or lease history is erased.
6. Exact operation retries return recorded outcomes while cross-kind or payload-conflicting reuse fails atomically.
7. Schema v1 upgrades to v2 without losing Change/audit history; focused, domain-review, workspace, strict Clippy, harness, and documentation gates pass.

## Risks

- **Data/security:** The persistence layer validates all durable enum/identifier values. Authorization of actors and trusted sourcing of timestamps belong to the future API/CLI boundary.
- **Concurrency/crash recovery:** Short immediate transactions serialize projection/event changes. Expiry is evaluated once per operation; process death requires no lock cleanup.
- **Provider divergence/compatibility:** No provider state is read or mutated.
- **Performance/resource limits:** Histories are revalidated linearly; indexing supports active assignment and scope lookup.
- **Upgrade/rollback:** v2 is additive but older binaries reject it. Rollback uses a pre-migration backup.

## Evidence and plan

- Relevant sources: `DOMAIN.md`, ADR-0002, ADR-0004, `crates/weft-domain`, and `crates/weft-storage-sqlite`.
- Required proof: verification-matrix Assignment/Lease row plus ADR-0002 two-process contention.

1. Seal domain values and transitions — proof: state-machine success, stale, expiry-boundary, and invalid-transition tests.
2. Add schema v2 and transactional repositories — proof: v1 migration/reopen and immutable projection/event constraints.
3. Prove local concurrency/recovery — proof: competing connections and process-exit expiry/reclaim.
4. Consolidate idempotency — proof: global operation registry, exact replay, and conflict tests.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-domain`; `cargo test -p weft-storage-sqlite` | Passed | 12 domain tests; 17 active storage tests and three process helpers invoked by parent tests |
| Domain/contract | Assignment/Lease state-machine and persistence round-trip | Passed | Stale release, exact expiry, renew/release, overlapping assignment history |
| Concurrency/recovery | Competing connection/process and abrupt-exit reclaim | Passed | Active child-process contention rejected; a child committed a lease then exited without destructors, and the parent reopened/reclaimed it at exact expiry |
| Provider integration | Not applicable | No provider mutation in scope |
| Static/harness | `make check` | Passed | Harness, documentation links, formatting, 40 active workspace tests, all four spawned-process helpers, and strict Clippy |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0004 separates overlapping responsibility from exclusive, expiring operational authority and establishes a global operation registry. Domain review findings on historical status, event/projection drift, process contention, and abrupt-exit recovery were resolved; final re-review found no remaining actionable issue.
- **Residual risks:** Trusted timestamp sourcing, authorization, busy-error taxonomy, clock rollback policy, backup/checkpoint tooling, and distributed coordination remain open.
- **Unavailable evidence:** Network filesystem, distributed clock, Windows process-recovery, and hosted coordination semantics are not claimed.
- **Follow-up:** Continue Phase 1 with durable Materialization state and lifecycle proof.

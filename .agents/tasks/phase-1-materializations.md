# Task Record: Phase 1 Materializations

## Outcome and scope

- **User/operator result:** Weft durably identifies where one exact revision is realized and can explain every verified provider-state observation across retries, concurrent writers, and restart.
- **In scope:** Typed Materialization identity and placement, exact revision binding, clean/dirty/diverged/suspended/released/invalidated states, version compare-and-swap, provider-reference history and evidence, immutable events, schema v3 migration, active-placement uniqueness, and fail-closed reconstruction.
- **Out of scope:** Provider subprocesses, actual worktree/virtual-branch creation, dirty-content capture, reconciliation execution, authorization, CLI/API surface, and distributed coordination.
- **Affected domain invariants:** `DOMAIN.md` sections 1, 8, 9, and 10; ADR-0002, ADR-0003, and ADR-0005.
- **Provider/runtime scope:** Provider-neutral persistence on local same-host SQLite WAL and the filesystem CAS; evidence is opaque adapter output.
- **Compatibility surface:** Domain API, SQLite schema/storage API, canonical revision availability, operation idempotency, and audit history.

## Acceptance criteria

1. Materialization identity, Change, exact revision, workspace, provider, creator, and creation time remain immutable through every state/reference observation.
2. Creation requires an existing exact revision with durable canonical content, initial clean state, matching provenance, and non-empty provider evidence.
3. Every transition compares the exact version, advances once, records state/reference/evidence atomically, and rejects stale, no-op, time-reversing, or terminal transitions.
4. Exact operation retries return their recorded historical outcome; cross-kind or payload/evidence-conflicting reuse fails without mutation.
5. At most one active Materialization per Change/workspace/provider exists; released and invalidated history remains durable and terminal.
6. Reads reconstruct immutable events and fail closed on missing canonical content, missing history, or projection/event drift.
7. Fresh, v1, and v2 databases reach schema v3 under concurrent opens without losing Change/Revision/Assignment/Lease history.

## Risks

- **Data/security:** Provider evidence is required and durable but intentionally opaque. Future adapters must validate and redact it before persistence; it must never contain repository source or credentials.
- **Concurrency/crash recovery:** Short immediate transactions serialize version checks, projection updates, operation registration, and event append. This checkpoint does not cover crashes around external provider mutations.
- **Provider divergence/compatibility:** Native Git and GitButler evidence formats and reconciliation are deferred to provider phases; no claim of provider mutation is made here.
- **Performance/resource limits:** Lifecycle reconstruction and canonical Change verification are linear in history size; limits and snapshots are deferred.
- **Upgrade/rollback:** v3 is additive but older binaries reject it. Rollback uses a pre-migration backup.

## Evidence and plan

- Relevant sources: `GOAL.md`, `DOMAIN.md`, `ROADMAP.md`, ADR-0002/0003/0005, `crates/weft-domain`, `crates/weft-artifact`, and `crates/weft-storage-sqlite`.
- Required proof: verification-matrix Materialization and domain-transition rows plus read-only domain review.

1. Seal identity, placement, state, version, and terminal behavior in provider-neutral domain types.
2. Add schema v3 projections/events and transactional create/transition/read repositories.
3. Prove canonical-content enforcement, exact replay, stale concurrency, restart, migration, and fail-closed drift handling.
4. Run domain review, resolve findings, and execute the full repository gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-domain`; `cargo test -p weft-storage-sqlite` | Passed | 17 domain tests; 25 active storage tests and three ignored helpers invoked by parent tests |
| Domain/contract | Materialization state-machine and exact-revision persistence tests | Passed | Identity preservation, terminal states, stale/no-op/time denial, reference-only observation, exact replay, and restart |
| Content/recovery | Canonical-content and event/projection checks | Passed | Missing manifest blocks read/transition; missing or drifted event history fails closed |
| Concurrency/migration | Independent writer and concurrent v2 upgrade tests | Passed | Stale version rejected; repeated concurrent v2-to-v3 opens preserve coordination history |
| Provider integration | Not applicable | No provider mutation in scope; opaque provider evidence is persisted |
| Static/harness | `make check` | Passed | Harness, Markdown links, formatting, 53 active workspace tests, all four spawned-process helpers, and strict Clippy |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0005 keeps exact revision binding immutable, models state as versioned verified observations, and requires durable provider evidence. Domain review's authoritative-read evidence-validation finding was resolved with stored-event parsing and corruption proof.
- **Residual risks:** Provider evidence schemas, redaction, adapter trust, crash-uncertain provider mutations, storage compaction, backup/checkpoint tooling, and distributed coordination remain open.
- **Unavailable evidence:** Native Git/GitButler end-to-end materialization, network filesystems, Windows, and crash injection between provider mutation and metadata commit are not claimed.
- **Follow-up:** Continue Phase 1 with explicit Relationship/dependency persistence and immutable CompositionCandidate inputs.

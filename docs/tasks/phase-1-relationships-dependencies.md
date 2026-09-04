# Task Record: Phase 1 relationships and dependencies

## Outcome and scope

- **User/operator result:** Weft distinguishes contextual Change relationships from directed exact-revision dependencies and reports stale inputs without silently retargeting them.
- **In scope:** Symmetric task-decomposition/related-to records, directed dependency identity, exact downstream/upstream revision pins, derived freshness, explicit repin/removal, optimistic versions, immutable events, active-edge cycle rejection, schema v4 migration, replay, concurrency, and fail-closed reconstruction.
- **Out of scope:** Stack order, CompositionCandidate persistence, Change lifecycle/rejection readiness, review/validation policy, provider calls, CLI/API schemas, authorization, and distributed coordination.
- **Affected domain invariants:** `DOMAIN.md` sections 1, 2, 3, 8, and 10; GOAL success criteria 6–8; ADR-0002.
- **Provider/runtime scope:** Provider-neutral local SQLite WAL; exact revisions remain backed by the filesystem CAS.
- **Compatibility surface:** Domain API, SQLite schema/storage API, global operation idempotency, immutable relationship history, and migration behavior.

## Acceptance criteria

1. Task-decomposition and related-to records use canonical unordered distinct endpoints, infer no dependency/order/transitivity, reject active duplicates, and remove only through exact-version CAS.
2. Dependency identity and downstream/upstream Changes are immutable; creation rejects self-edges and requires exact revisions owned by the corresponding Changes.
3. Active directed dependencies remain acyclic, including concurrent attempts that would complete a cycle, and duplicate active edges fail atomically.
4. Freshness derives from both current Change heads and distinguishes downstream, upstream, or both advancing without mutating exact pins.
5. Repin atomically replaces the exact downstream/upstream pin pair, requires a changed pin and exact version, and preserves prior history; removal is terminal.
6. Exact operation retries return historical outcomes while cross-kind or payload-conflicting reuse fails without partial mutation.
7. Reads reconstruct immutable events and fail closed on invalid values, missing history, non-contiguous versions, or projection/event drift.
8. Fresh, v1, v2, and v3 databases reach schema v4 under serialized concurrent opens without losing existing histories; focused, domain-review, workspace, strict Clippy, harness, and documentation gates pass.

## Risks

- **Data/security:** Identifiers and exact revision ownership are validated at domain and database boundaries; no provider evidence or source content is stored in relationship events.
- **Concurrency/crash recovery:** Immediate transactions serialize graph checks, projection changes, operation registration, and event append. SQLite is the sole local graph authority.
- **Provider divergence/compatibility:** Relationships are provider-neutral and canonical artifacts remain independently verified.
- **Performance/resource limits:** Cycle and history checks are linear in reachable graph/history size; graph limits and materialized closure tables are deferred until measurement.
- **Upgrade/rollback:** v4 is additive but older binaries reject it. Rollback uses a pre-migration backup.

## Evidence and plan

- Relevant sources: `GOAL.md`, `DOMAIN.md`, `ROADMAP.md`, ADR-0002, `crates/weft-domain`, and `crates/weft-storage-sqlite`.
- Required proof: verification-matrix dependency row plus read-only domain review.

1. Seal symmetric relationship and directed dependency types, versions, pins, freshness, repin, and terminal removal.
2. Add schema v4 projections/events with exact revision ownership and atomic active-graph cycle guards.
3. Prove replay, stale writers, cycles, freshness, restart, migration, and fail-closed history validation.
4. Run domain review, resolve findings, record the ADR/evidence, and execute the full repository gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-domain`; `cargo test -p weft-storage-sqlite` | Passed | 23 domain tests; 37 active storage tests and three ignored helpers invoked by parent tests |
| Domain/contract | Relationship and dependency state-machine tests | Passed | Canonical endpoints, exact pins/freshness, repin, stale/time/no-op denial, terminal removal |
| Concurrency/recovery | Active cycle, stale writer, restart, and migration tests | Passed | Opposite independent writers permit exactly one edge; stale repin has no event; reopen preserves histories; concurrent v3 upgrade succeeds |
| Provider integration | Not applicable | No provider mutation in scope |
| Static/harness | `make check` | Passed | Harness, Markdown links, formatting, 71 active workspace tests, all four spawned-process helpers, and strict Clippy |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0006 separates symmetric context from directed exact-pin dependencies, derives freshness, and serializes active cycle checks. Domain review's public audit-read validation finding was resolved by routing listings through authoritative lifecycle and canonical-content validation; final re-review found no remaining actionable issue.
- **Residual risks:** Stack/candidate resolution, lifecycle readiness, graph scale, backup tooling, and distributed coordination remain open.
- **Unavailable evidence:** Network filesystems, distributed writers, provider adapters, candidate resolution, and rejected-upstream readiness are not claimed.
- **Follow-up:** Continue Phase 1 with durable Stack and immutable CompositionCandidate resolution.

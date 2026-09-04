# Task Record: Phase 1 durable revisions

## Outcome and scope

- **User/operator result:** A revision is accepted only when its canonical artifact is locally durable and verifiable; competing local writers receive a stale-head outcome rather than creating a fork.
- **In scope:** Canonical artifact encoding and digesting, filesystem content-addressed blobs/manifests, SQLite Change/Revision persistence, strict identifier/base/path validation, and storage-level tests.
- **Out of scope:** Providers, materializations, candidates, leases, reviews, integrations, CLI, runtime packaging, and GitButler CI.
- **Affected domain invariants:** Immutable canonical content, exact base, linear revision head, durable identity, atomic stale-head rejection, and audit-ready storage foundations.
- **Provider/runtime scope:** Provider-neutral local storage only; no provider mutation.
- **Compatibility surface:** API | artifact | storage

## Acceptance criteria

1. Canonical artifact bytes deterministically bind an exact base and ordered tree operations; their SHA-256 digest is computed and checked on reopen.
2. Every upsert blob is present in the local CAS and hash-verified before a revision can be persisted.
3. SQLite persists Change and ChangeRevision state, and two independent repository connections cannot append different successors from one expected head.
4. Reserved/internal paths and control-character identifiers/base values are rejected.
5. `make check` passes with focused reconstruction and competing-writer proof.

## Risks

- Data/security: untrusted artifact paths or forged digests could write outside intended content or make history unreconstructable.
- Concurrency/crash recovery: interrupted writes must never publish a partial CAS object or a revision whose head did not change atomically.
- Provider divergence/compatibility: provider adapters remain absent, so provider object validation is intentionally not claimed.
- Performance/resource limits: CAS verification reads blobs; limits and garbage collection remain a later storage concern.
- Upgrade/rollback: newer database schemas are rejected explicitly; migrations and a data-preserving downgrade path remain unimplemented.

## Evidence and plan

- Relevant paths: `crates/weft-domain/src/{artifact,change,storage}.rs`, ADR-0002, `DOMAIN.md` sections 1 and 8.
- Baseline: previous kernel held only a digest-shaped string and in-memory head.

1. Define deterministic artifact encoding and CAS — artifact round-trip, tamper, binary/mode/symlink/delete tests.
2. Add transactional SQLite repository — reopen and two-connection stale-head tests.
3. Update verification/docs — make check.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Canonical artifact and CAS | storage artifact-reopen test | Passed | Reopens a SQLite repository and verifies binary, executable, symlink, and deletion artifact operations through the filesystem CAS. |
| Competing writers | storage competing-writer test | Passed | Two separately opened SQLite connections race from the same persisted head; exactly one appends and one receives StaleHead. |
| Static/harness | make check | Passed | 2026-09-02: harness, documentation links, format, 14 tests, and strict Clippy. |

## Decision and follow-up

- **Decision and alternatives rejected:** Use SQLite bundled for a reproducible local test/build rather than relying on host SQLite; keep blob payloads outside SQLite as ADR-0002 requires.
- **Residual risks:** No candidate, provider, or crash-injection implementation exists yet. CAS objects are immutable and verified on read, but garbage collection, filesystem permission hardening, and fault injection are deferred.
- **Unavailable evidence:** GitButler adapter/reconnect and uncertain landing tests remain Phase 3 work. A local GitButler 0.22.0 Phase 0 rerun is currently blocked by a but.sqlite database-lock failure during setup; it must not be reported as passing CI evidence.
- **Follow-up:** Add lease/audit schema and crash-injection tests after durable revision storage is proven.

# Task Record: Phase 1 SQLite Change store

## Outcome and scope

- **User/operator result:** A Change and its immutable linear revisions survive process restart, and stale concurrent writers cannot overwrite the head.
- **In scope:** Initial SQLite schema, migration/reopen behavior, Change creation, revision append CAS, revision provenance, exact operation replay, and append-only audit events.
- **Out of scope:** Artifact codec/CAS implementation, other Phase 1 entities, provider mutation, CLI/API schemas, packaging, and remote or network filesystems.
- **Affected domain invariants:** `DOMAIN.md` sections 1, 8, and 10.
- **Provider/runtime scope:** Local file-backed SQLite 3.45.1 through `rusqlite` 0.39.0; no provider calls.
- **Compatibility surface:** Domain API, schema, storage.

## Acceptance criteria

1. Migration and reopen rehydrate exact revision identity, ancestry, base, canonical artifact reference, creator, creation time, and current head through domain constructors and verified artifact bytes.
2. Revision append uses one immediate transaction and rejects a stale expected head without leaving a revision or audit row.
3. Separate connections and a separate process observe the same CAS boundary.
4. An exact operation-ID replay returns its recorded success without duplication; conflicting reuse fails atomically.
5. Every successful mutation writes an immutable audit event with actor, operation, expected state, and resulting state.
6. Foreign keys, WAL mode, schema versioning, formatting, tests, strict Clippy, harness, and documentation checks pass.

## Risks

- **Data/security:** Durable strings are revalidated through sealed domain constructors on read; filesystem permissions and untrusted database handling remain future hardening work.
- **Concurrency/crash recovery:** SQLite serializes immediate writers; the bounded busy timeout currently surfaces as a database error rather than a stable public error contract.
- **Provider divergence/compatibility:** No provider state is read or mutated in this slice.
- **Performance/resource limits:** Revision rehydration is linear and intentionally favors invariant validation over projection optimization.
- **Upgrade/rollback:** Only schema version 1 exists; newer versions fail closed. Downgrade and backup policy remain release work.

## Evidence and plan

- Relevant sources: `DOMAIN.md`, ADR-0002, `crates/weft-domain`, `crates/weft-storage-sqlite`.
- Required proof: verification matrix domain transition row plus ADR-0002 migration and two-process requirements.

1. Define schema and transaction boundary — proof: migration/reopen test and foreign-key/WAL assertions.
2. Persist and rehydrate Changes — proof: exact round-trip and immutable audit assertions.
3. Exercise concurrency/retry failures — proof: independent-connection stale writer, child-process writer, rollback, and exact replay tests.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-storage-sqlite` | Passed | 9 normal tests; one ignored helper is executed by the spawned-process test |
| Domain/contract | Migration/reopen round-trip | Passed | Two revisions and three exact audit events rehydrated |
| Concurrency/recovery | Concurrent migration, independent connection, and child-process tests | Passed | Migration serialized; losing mutations left no revision or audit row |
| Provider integration | Not applicable | No provider mutation in scope |
| Static/harness | `make check` | Passed | Artifact-integrated workspace: harness, documentation links, formatting, 28 active tests, both spawned-process helpers, and strict Clippy |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** Implemented ADR-0002 directly: SQLite WAL with short immediate transactions and domain revalidation. In-memory tests were rejected because they cannot prove file-backed WAL or process behavior. Domain review required complete immutable request matching for operation replay and same-Change foreign keys plus shape checks for audit history; both findings were resolved and re-reviewed with no remaining actionable findings.
- **Residual risks:** Busy/locked error taxonomy, crash injection during commit, backup/checkpoint policy, and future schema downgrade behavior remain open.
- **Unavailable evidence:** Network filesystem semantics are unsupported and untested by design.
- **Follow-up:** Completed by [Phase 1 canonical artifacts](phase-1-canonical-artifacts.md). Continue with Assignment and Lease persistence.

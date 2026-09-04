# Task Record: Phase 1 canonical artifacts

## Outcome and scope

- **User/operator result:** Every persisted revision has provider-independent bytes that can reconstruct its exact file tree after the originating workspace disappears.
- **In scope:** `tree-delta-v1` encoding, SHA-256 filesystem CAS, atomic concurrent writes, verified reads, manifest/blob durability, exact-base binding, safe reconstruction, and SQLite append/load verification.
- **Out of scope:** Capturing provider trees, proving a supplied base directory's provider object identity, CAS garbage collection/repair, remote storage, and provider mutation.
- **Affected domain invariants:** `DOMAIN.md` sections 1, 5, 8, and 9; ADR-0002 and ADR-0003.
- **Provider/runtime scope:** Provider-neutral local filesystems; Unix-specific executable and symbolic-link reconstruction is proven.
- **Compatibility surface:** Domain API, canonical artifact, storage API, filesystem layout.

## Acceptance criteria

1. Equal exact base and sorted operations encode to identical versioned bytes and a pinned SHA-256 digest; malformed, non-canonical, oversized, unknown-version, and trailing data fail closed.
2. CAS writes never replace an existing digest path, synchronize bytes before publication, tolerate concurrent identical writers, and verify every read.
3. A manifest is durable only after every referenced blob exists and passes digest verification.
4. Reconstruction preserves binary bytes, regular/executable modes, symbolic links, rename-as-delete-plus-upsert, deletion, and unchanged base content after the originating workspace is deleted.
5. Wrong base identity or structurally incompatible base content fails before creating the destination.
6. SQLite refuses revision append and load unless the manifest, blobs, and exact recorded base verify.
7. Domain review, focused tests, strict Clippy, documentation/harness checks, and the complete workspace gate pass.

## Risks

- **Data/security:** Reads reject symlink/non-file CAS objects and verify hashes; hostile concurrent filesystem mutation remains outside the local trusted-state boundary.
- **Concurrency/crash recovery:** Temporary bytes are synchronized before atomic hard-link publication. A crash may leave an unreferenced temporary file but cannot publish a partial final object.
- **Provider divergence/compatibility:** The caller must verify the supplied base materialization's provider object identity; reconstruction checks identity equality but cannot derive it from arbitrary directory bytes.
- **Performance/resource limits:** Objects are currently read into memory with a 512 MiB per-object bound. Streaming and aggregate quotas remain future work.
- **Upgrade/rollback:** The wire format and golden digest are frozen as v1. Breaking changes require a new version; no in-place rewrite is permitted.

## Evidence and plan

- Relevant sources: `DOMAIN.md`, ADR-0002, `crates/weft-domain`, `crates/weft-artifact`, and `crates/weft-storage-sqlite`.
- Required proof: verification matrix revision/canonical-content row and ADR-0002 artifact cases.

1. Freeze codec and object identity — proof: golden digest, strict decoder, corruption/limit tests.
2. Store manifests and blobs durably — proof: concurrent writers and missing-reference denial.
3. Reconstruct exact trees — proof: provider-workspace removal fixture covering all required file semantics and failure boundaries.
4. Bind metadata to bytes — proof: SQLite append/load denial for absent content and base mismatch.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-artifact` | Passed | 11 active artifact/CAS/codec/reconstruction tests; one ignored helper is executed by the cross-process test |
| Domain/contract | Golden digest and provider-removal reconstruction | Passed | Exact base plus binary, executable, symlink, rename, deletion, unchanged content |
| Concurrency/recovery | Concurrent CAS writers and repeated concurrent migration race | Passed | One verified digest; migration race passed 20 repeated runs |
| Persistence boundary | SQLite artifact append/load checks | Passed | Missing content and base mismatch rejected without metadata mutation; missing manifest fails load |
| Provider integration | Not applicable | Provider workspace is removed; no provider mutation occurs |
| Static/harness | `make check` | Passed | Harness, documentation links, formatting, 28 active workspace tests, both spawned-process helpers, and strict Clippy |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0003 freezes a compact binary format and no-replace filesystem CAS. Canonical JSON, provider-owned objects, silent overwrite, and digest-only metadata trust were rejected.
- **Residual risks:** Verified-base acquisition, streaming, garbage collection, orphan cleanup, repair, permissions, backups, quotas, and non-Unix symlink support remain open.
- **Unavailable evidence:** Windows symbolic-link reconstruction and non-local filesystem atomicity are not claimed.
- **Follow-up:** Add durable Assignment and Lease state with competing-writer, expiry/reclaim, and process-recovery proof.

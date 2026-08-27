# ADR-0003: Canonical `tree-delta-v1` bytes and filesystem CAS

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

ADR-0002 selected a provider-independent `tree-delta-v1` artifact and filesystem content-addressed storage but did not freeze its byte encoding or reconstruction boundary. A revision digest must remain identical across processes and providers, survive loss of its originating workspace, retain binary and file-mode semantics, and fail closed when bytes are missing or corrupt.

## Decision

`tree-delta-v1` uses a deterministic length-prefixed binary manifest:

1. Magic bytes `WEFT-ARTIFACT\0`.
2. UTF-8 artifact version string `tree-delta-v1`.
3. Exact Repository ID and base object ID.
4. Big-endian unsigned 32-bit operation count.
5. Strictly path-sorted operations. Each operation has a one-byte tag, a length-prefixed canonical repository-relative UTF-8 path, and for upserts a length-prefixed lowercase SHA-256 blob digest.

Operation tags are `0` delete, `1` regular-file upsert, `2` executable-file upsert, and `3` symbolic-link upsert. All strings use a big-endian unsigned 32-bit byte length followed by exact UTF-8 bytes. Decoders reject unknown tags or versions, excessive counts/fields, invalid domain values, truncation, and trailing bytes. The compatibility fixture in `weft-artifact` pins a known manifest digest.

Objects use lowercase `sha256:<64-hex>` identity and live below `objects/sha256/<first-two-hex>/<remaining-hex>`. Writes use a synchronized temporary regular file followed by an atomic same-directory hard link, so an existing object is never replaced. Reads enforce a size bound and recompute the digest. Manifests are stored only after every referenced blob is present and verified.

Reconstruction requires a caller-supplied materialization already verified to match the manifest's exact base identity. Weft snapshots that directory without following symbolic links, applies deletes before upserts, rejects structural mismatches, and writes into a newly created destination. Empty directories are not canonical objects. Unix executable bits and symbolic-link target bytes are preserved; non-Unix symbolic-link materialization remains explicitly unsupported until proven.

SQLite revision append and load verify the manifest, every referenced blob, and exact base match before returning durable revision state.

## Alternatives

- Canonical JSON: rejected because serializer settings, escaping, number handling, and map ordering create a larger compatibility surface without benefiting the binary file content.
- Git objects as the CAS: rejected because Git remains a provider and its object availability cannot define Weft durability.
- Rename as a distinct operation: rejected in v1; delete plus upsert is deterministic and retains exact resulting content.
- Overwriting an existing digest path with `rename`: rejected because corruption or a race must be detected, never silently replaced.
- Trusting only SQLite's digest string: rejected because a syntactically valid reference does not prove durable reconstructable bytes.

## Consequences

- Manifest bytes and SHA-256 identity are now a compatibility contract; incompatible evolution requires a new artifact version.
- The exact base content is not duplicated inside each delta. Provider/base snapshot code must prove that the supplied base materialization matches its recorded identity.
- CAS garbage collection, quotas beyond the per-object bound, backup, repair, and orphan temporary-file cleanup remain future storage work.
- Local state tampering is detected on read but filesystem access control is outside this format.

## Required proof

- Golden digest and encode/decode/encode equality.
- Concurrent identical writes produce one verified object identity.
- Missing, oversized, non-regular, and corrupt objects fail closed.
- A manifest cannot be committed before every referenced blob is durable.
- Binary, executable, symbolic-link, rename, and deletion reconstruction succeeds after the originating provider workspace is removed.
- Wrong base identity, structurally incompatible base content, malformed manifests, and trailing bytes fail without producing a destination.

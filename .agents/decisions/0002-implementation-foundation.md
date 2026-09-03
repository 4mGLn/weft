# ADR-0002: Rust, SQLite, CAS, and capability-based provider adapters

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Phase 0 reproduced canonical reconstruction, rewrite behavior, parallel and stacked work, target guarding, conflict capture, landing, and external-state reconciliation across Native Git and GitButler. The domain requires strong typed invariants, a portable local executable, transactional multi-process metadata, durable content outside provider state, and explicit provider capability differences.

## Decision

Use:

1. A Rust workspace for the domain library, provider adapters, and CLI.
2. SQLite metadata storage in WAL mode for same-host local use, with short transactions, explicit busy/retry policy, foreign keys, migrations, and controlled checkpoints.
3. A filesystem content-addressed store next to the database for potentially large canonical blobs; SQLite stores manifests, digests, references, and operation metadata.
4. A versioned `tree-delta-v1` canonical artifact containing exact base identity, sorted canonical repository-relative path operations, file modes, lowercase `sha256:<64-hex>` blob digests, and artifact version.
5. Capability-based provider ports. Native Git uses version-gated Git plumbing; GitButler uses validated version-gated CLI JSON. Raw commands and JSON never become public domain types.
6. Canonical bytes are a versioned binary contract: UTF-8 strings are length-prefixed
   with unsigned 64-bit big-endian lengths; the tree manifest starts with
   `weft/tree-delta-v1\0`, and the base-bound artifact wrapper starts with
   `weft/canonical-artifact-v1\0`. The SHA-256 digest names the complete wrapper,
   not a provider object or an unbound manifest.

The first implementation is a single local process/CLI with safe multi-process database access. A hosted service, network database, and distributed lease authority remain out of scope.

## Alternatives

- Go: viable for a portable CLI, but Rust provides a stronger fit for invariant-heavy domain types and future direct integration with the Rust Git/GitButler ecosystem.
- Embedded Git library as the only Native Git path: rejected for v1 because behavior parity and edge coverage would add risk before the domain kernel exists.
- Git commits or GitButler objects as canonical content: rejected because provider deletion or rewrite would violate revision durability.
- Store all blobs inside SQLite: rejected because large canonical artifacts would couple database write amplification and checkpoint behavior to repository content size.
- Event log without transactional projections: rejected because v1 needs simple atomic invariants and recovery queries; durable events remain an audit trail within transactional storage.

## Consequences

- Phase 1 can enforce revision-head CAS, acyclic dependencies, candidate immutability, idempotent operations, and audit events transactionally.
- Deployment is a local binary plus state directory initially; packaging details still require a release ADR.
- WAL requires local filesystem semantics and one writer at a time. Weft must surface contention and unsupported storage locations.
- Filesystem CAS writes are published only after the complete object is synced and
  atomically linked into its digest address. A crash may leave an unreferenced
  temporary file, but never a partial object at a canonical address.
- Provider subprocesses require deadlines, cancellation, bounded output, redaction, and reconciliation after uncertain termination.

## Required proof

- Migration and reopen round-trip.
- Two-process stale-head and lease contention tests.
- Canonical artifact digest/reconstruction tests including binary, executable, symlink, rename, and deletion cases.
- Provider version/capability and unknown-JSON rejection tests.
- Crash injection between provider mutation, reconciliation, and receipt completion.

Related evidence: [Phase 0 report](../phase0/provider-feasibility-report.md) and [capability matrix](../phase0/provider-capability-matrix.md).

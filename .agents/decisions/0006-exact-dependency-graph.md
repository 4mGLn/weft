# ADR-0006: Exact dependency graph and contextual relationships

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Weft must distinguish task context, revision ancestry, required upstream work, and stack order. Treating all of them as one graph would infer invalid ordering and readiness. Dependency declarations also need to survive revision advancement without silently changing what downstream work actually consumed, while concurrent local writers must not create a cycle.

## Decision

Task-decomposition and related-to are symmetric contextual records. Their distinct Change endpoints are stored in canonical identity order, and their kind/pair implies no direction, ancestry, order, readiness, dependency, or transitive closure. Identity, kind, endpoints, and creation provenance are immutable. Exact-version removal is terminal and retains immutable history. One active record may exist for each kind/pair.

A Dependency is a separate directed record from one downstream Change to one distinct upstream Change. Identity, direction, and creation provenance are immutable. Its versioned projection pins both the exact downstream revision that declared or consumed the requirement and the exact upstream revision required. Exact revision ownership and canonical content are verified at creation, repin, read, and removal boundaries.

Freshness is derived by comparing both pins with their current Change heads. Head advancement never mutates the Dependency. Explicit repin atomically replaces the exact pair, must change at least one pin, and appends the prior/resulting version and pins. Removal is terminal and frees the directed pair while preserving history.

Only active directed edges participate in cycle detection. Creation runs the reachability check inside the same SQLite immediate transaction that inserts the edge, operation record, and immutable event. A schema trigger independently rejects cycles. This serializes competing local graph writers so opposite edges cannot both commit. One active dependency may exist for each directed Change pair.

All mutations use the global operation registry. Exact retries return the recorded outcome—including a historical repin result after later events—while cross-kind or payload-conflicting reuse fails. Authoritative reads rebuild projections from contiguous immutable events and fail closed on drift.

## Alternatives

- One generic directed relationship graph: rejected because contextual relevance, decomposition, dependency, ancestry, and stack order have different invariants.
- Store only upstream Change identity: rejected because later head movement would silently reinterpret consumed input.
- Pin only the upstream revision: rejected because the downstream revision that declared or consumed the requirement would remain ambiguous.
- Persist a mutable stale boolean: rejected because freshness is derived from exact immutable pins and current heads and would otherwise drift.
- Delete and recreate on repin: rejected because it loses one dependency contract's revision history and complicates exact retry outcomes.
- Check cycles outside the write transaction: rejected because two local writers could both validate against the same old graph and commit a cycle.
- Materialize a transitive closure table now: deferred until graph scale proves recursive reachability insufficient.

## Consequences and migration

Schema v4 adds contextual relationship and directed dependency projections, immutable events, active uniqueness, exact revision ownership, append-only guards, and cycle enforcement. Fresh databases and v1/v2/v3 upgrades apply the additive migration under the serialized migration lock. Older binaries reject schema v4; rollback restores a pre-upgrade backup rather than rewriting history.

Stack and CompositionCandidate persistence are defined by ADR-0007. Candidate creation copies exact dependency resolutions rather than targeting mutable dependency projections. Upstream rejection/blocking remains deferred until Change lifecycle state exists.

## Required proof

- Symmetric endpoint canonicalization, active duplicate denial, versioned removal, replay, restart, and event/projection validation.
- Exact downstream/upstream revision ownership and canonical content on every dependency boundary.
- Derived downstream/upstream/both staleness without pin mutation.
- Explicit repin, stale-writer denial, historical replay, terminal removal, and immutable events.
- Concurrent opposite-edge creation permits exactly one commit and leaves an acyclic graph.
- v3-to-v4 migration preserves Materialization history and concurrent upgrade rechecks under the migration lock.

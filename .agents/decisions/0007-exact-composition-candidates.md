# ADR-0007: Versioned Stacks and exact CompositionCandidates

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Review, validation, and integration need one immutable target even while Changes, Dependencies, and operator-managed ordering continue to evolve. A mutable Stack cannot be that target. Resolving each input or requirement in separate reads would also permit a candidate assembled from states that never existed together.

## Decision

A Stack has immutable identity and provenance plus a versioned, non-empty, duplicate-free ordered definition. Every member records its direct predecessor explicitly. `order_only` records topology without inferring readiness. `predecessor_dependencies` adds one exact direct-predecessor requirement for every member after the first when a candidate is resolved; it never creates or mutates a durable Dependency. Definition replacement uses exact-version compare-and-swap, rejects no-op and time-reversing changes, and records the complete resulting snapshot.

Candidate creation accepts either an exact expected Stack version or an explicit ordered Change list. One SQLite immediate transaction resolves current exact heads, verifies their canonical artifacts and common repository, and evaluates every active Dependency whose downstream is selected. Dependency pins must match both current exact heads and the required upstream must appear earlier. Missing, reversed, or stale requirements abort the operation. Stack-predecessor requirements are added from the exact selected snapshot.

A CompositionCandidate is immutable. It records the exact target base, optional Stack ID/version/policy, ordered Change/revision inputs, and canonically ordered resolved requirements. `composition-candidate-v1` is SHA-256 over a domain-separated, big-endian length-prefixed binary encoding of those correctness fields. Candidate identity and creation provenance are excluded, so identical correctness state has the same digest. Any exact input, order, target, Stack snapshot, policy, or requirement change changes the encoded input.

Authoritative reads reconstruct Stack versions from finalized full snapshots, reconstruct candidates through domain constructors, re-encode and rehash them, verify every canonical revision, and prove that each recorded Dependency version/pin or Stack snapshot existed. Later source changes remain valid history and are reported through derived freshness rather than mutating the candidate. Live provider-target freshness is deferred to integration planning and reconciliation.

Stack and candidate mutations use the global operation registry. Exact retries return the recorded historical outcome even after later head, Dependency, or Stack changes; conflicting operation reuse fails.

## Alternatives

- Review or integrate a mutable Stack: rejected because order, policy, heads, and dependencies can change between approval and execution.
- Resolve inputs outside one metadata transaction: rejected because the resulting combination might never have existed atomically.
- Persist only ordered revisions: rejected because the exact target, Stack policy/version, and requirement provenance affect correctness.
- Create durable Dependencies for Stack predecessors: rejected because operator composition topology and declared cross-Change requirements have different lifecycle and intent.
- Include candidate ID or creator/time in the digest: rejected because those fields do not change composition correctness and would prevent content equivalence.
- Persist a mutable stale flag: rejected because freshness is derived from immutable candidate evidence and current projections.
- Treat a provider target observation as candidate freshness: deferred because provider state requires capability-specific observation and reconciliation.

## Consequences and migration

Schema v5 adds Stack projections, finalized full-snapshot events, immutable candidates, exact ordered inputs, and resolved requirements. Fresh databases and v1–v4 upgrades apply the additive migration under the serialized migration lock. A populated concurrent v4 upgrade preserves Relationship and Dependency history. Older binaries reject schema v5; rollback restores a pre-migration backup rather than rewriting history.

Candidate resolution currently scans active Dependencies and validates selected histories. Scale limits and indexed query planning remain a later performance checkpoint. ReviewRequest and ValidationResult persistence can now target exact candidate identity and digest.

## Required proof

- Stack non-empty/duplicate/predecessor validation, version CAS, no-op/stale denial, exact replay, restart, finalized snapshots, and projection-drift rejection.
- Candidate exact-head/canonical-content/repository validation in one immediate transaction.
- Active Dependency inclusion, exact historical source/pin verification, stale-pin denial, missing-upstream denial, and upstream-first ordering.
- Complete predecessor-policy requirements without durable Dependency creation.
- Deterministic identity-independent digest and sensitivity to exact correctness fields.
- Immutable persistence, restart reconstruction, source/digest drift rejection, and historical operation replay.
- Derived input, Dependency, and Stack freshness without candidate mutation.
- Fresh and v1–v4 migration, including populated concurrent v4 preservation.

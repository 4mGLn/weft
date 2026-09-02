# ADR-0005: Exact-revision Materialization lifecycle

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Weft needs durable knowledge of where one exact Change revision is realized without treating a provider branch, worktree, or virtual branch as domain identity. Provider references and working state can change externally, concurrent observers can race, and a crash must not leave an unexplained success projection.

## Decision

A Materialization has immutable identity, Change, exact revision, workspace, provider, creator, and creation time. It begins clean only at a trusted provider-adapter boundary that supplies non-empty evidence for the exact canonical revision. Its current provider reference and state are versioned projections over immutable creation and transition events.

Every transition supplies one atomic `ProviderObservation`: state, provider reference, and opaque non-empty evidence. It compares the exact expected Materialization version, advances by one, and records actor, time, and globally unique operation ID in the same SQLite transaction. Provider adapters own evidence structure and verification; the provider-neutral kernel preserves it without interpreting provider-specific syntax. Exact retries return the historical outcome, while payload-conflicting reuse fails.

States are observations, not a workflow: any non-terminal observation may follow another, including a provider-reference change with the same state. A no-op is rejected. Released and invalidated Materializations are terminal. At most one non-terminal Materialization for one Change occupies a workspace/provider placement.

The exact revision binding never changes. Dirty capture appends a new revision through Change-head compare-and-swap and later creates a distinct Materialization. Reads verify the bound revision's canonical content and reconstruct lifecycle state from events; missing content or projection/event drift fails closed.

## Alternatives

- Use the provider reference as identity: rejected because provider rewrites and deletion would destroy domain continuity.
- Retarget a dirty Materialization to a new revision: rejected because reviews, dependencies, and history require exact immutable revision bindings.
- Encode a strict clean/dirty workflow: rejected because the states report provider facts and reconciliation may legitimately move between any active observations.
- Store only a mutable status row: rejected because restart, audit, idempotency, and reconciliation require an immutable explanation.
- Interpret one universal evidence schema: deferred because Native Git and GitButler expose different state identities; adapters must preserve capability differences.

## Consequences and migration

Schema v3 adds Materialization projections, immutable events, provider evidence, exact-revision foreign keys, active-placement uniqueness, and transition guards. Fresh databases and v1/v2 upgrades apply the additive migration under the existing serialized migration lock. Older binaries reject schema v3; rollback restores a pre-upgrade backup rather than deleting history.

This checkpoint persists verified observations but does not yet call a provider. Native Git and GitButler adapters must later prove how they derive evidence, materialize canonical content, detect divergence, and recover uncertain mutations before this boundary can claim end-to-end provider reconciliation.

## Required proof

- Exact identity and revision binding survive state/reference changes and restart.
- Canonical content is required on create, read, and transition.
- Independent stale writers fail; exact retries replay and conflicting retries fail.
- Released and invalidated states are terminal, and active placement is unique.
- Event/provider evidence is immutable and projection drift fails closed.
- Concurrent v2-to-v3 migration preserves existing Change, Assignment, and Lease history.

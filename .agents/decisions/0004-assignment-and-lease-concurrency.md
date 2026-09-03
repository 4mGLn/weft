# ADR-0004: Durable assignments and versioned operation leases

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Weft must preserve responsibility across sessions while preventing mutually unsafe operations from running concurrently. A mutable owner field loses handoff history, while a process lock cannot survive crashes. Lease expiry and retries also need deterministic multi-process behavior without introducing a hosted or distributed coordinator.

## Decision

Assignments are durable tenures identified independently from their subject. Assignment and release each append an immutable event; an active projection exists only to support transactional checks. Different subjects and roles may overlap. Release uses an exact assignment version.

Leases are exclusive per `(change_id, operation_key)` scope. The scope owns a monotonic optimistic-concurrency version and at most one current lease projection. Each acquisition creates an immutable lease identity and records holder, acquisition timestamp, initial expiry, and the expired predecessor when reclaiming. Renew and release operate only on the exact current lease and expected scope version. Expiry is active for `observed_at < expires_at`; equality is expired. Renewal must extend expiry, and reclaim creates a new identity rather than mutating the expired tenure.

All Assignment, Lease, Change, and Revision mutations register their operation ID in one global immutable registry. Exact retries return the recorded outcome. Cross-kind or payload-conflicting reuse fails without mutation. SQLite transactions update projections, operation records, and immutable events atomically.

## Alternatives

- One mutable Change owner: rejected because overlapping roles and handoff history would be lost.
- Assignment implies exclusivity: rejected because responsibility and operational authority are different domain concepts.
- OS/process locks: rejected because they are not durable, observable history and cannot safely recover after process failure.
- Renew an expired lease or reuse its identity: rejected because it obscures the loss-of-authority interval and predecessor history.
- Wall-clock cleanup that deletes expired rows: rejected because expiry is a derived status and history must remain.
- Per-table operation uniqueness: rejected because the same operation ID could otherwise describe different domain effects.

## Consequences and migration

Schema v2 migrates existing v1 audit operation IDs into the global registry before enabling Assignment and Lease state. Upgrade is additive and preserves existing Change history. Older binaries reject schema v2; rollback requires restoring a pre-upgrade database backup rather than rewriting coordination history.

The persistence API accepts an explicit observation/event timestamp from its trusted application boundary for deterministic transitions. The future CLI/API boundary must source it from the local Weft process rather than user JSON. Distributed clock authority and network filesystems remain out of scope.

## Required proof

- v1-to-v2 migration and reopen preserve Change history.
- Overlapping Assignment creation, exact-version release, stale denial, and immutable event replay.
- Competing SQLite writers cannot acquire one active scope.
- Renewal extends an active lease; release clears authority.
- Exact expiry permits reclaim with a new identity and predecessor after process restart.
- Repeated operation IDs replay exact outcomes and reject cross-kind or payload-conflicting reuse.

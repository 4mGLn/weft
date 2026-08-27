# ADR-0009: Integration authority, reconciliation, and verified receipts

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Provider integration changes a shared repository target and can time out after the provider has accepted the effect. Treating timeout, process loss, or lease expiry as failure permits duplicate integration. Treating a mutable branch or provider reference as the integration identity also loses the exact candidate and target intent that reviewers approved.

## Decision

An IntegrationAttempt has immutable identity and intent: exact CompositionCandidate identity/digest/ordered inputs, repository and target ref, expected target revision, provider, strategy, and a stable provider-effect operation ID. Metadata transition operation IDs remain distinct and globally replayable. Planning verifies the authoritative current candidate, its target base, policy/capability evidence, and any current Approved/Passed evidence within that candidate.

Starting performs exact attempt-version and target-revision compare-and-swap checks and grants an expiring recoverable execution lease scoped to `(repository_id, target_ref)`. This scope is independent of Change-operation leases because one candidate can contain several Changes. Only the live exact holder can renew or report a direct outcome. A competing live target lease is rejected atomically.

Timeout, authority loss, and lease expiry never imply failure and never authorize blind execution. They move the attempt to Reconciling. Reconciliation appends provider observations as StillUncertain, NoEffectVerified, ResultVerified, or Diverged. Only exact NoEffectVerified evidence permits terminal Failed/Aborted after uncertainty. Exact ResultVerified evidence permits Succeeded and a receipt. Exact Diverged evidence permits an explicit receipt-free Superseded transition, after which a new candidate/attempt may target the observed revision. Direct Running success requires live authority and verified target evidence.

Conflict is a terminal attempt result and atomically creates one immutable IntegrationConflict copying candidate inputs and provider evidence. ConflictResolution is a separate immutable record citing a current exact target and current Passed validations; it never rewrites or resumes the conflicted attempt.

Succeeded atomically creates one immutable IntegrationReceipt copying attempt/candidate, repository/ref, prior/result revisions, provider, stable effect-operation ID, verification evidence, and provenance. No other state has a receipt. Authoritative reads reconstruct the append-only event sequence and compare generated conflicts and receipts with their immutable rows. Historical attempts remain readable after later Change-head movement; their immutable source content and evidence relationships must still reconstruct.

## Alternatives

- Reuse Change-scoped operation leases: rejected because a multi-Change candidate has target-level exclusivity.
- Mark timeout or lease expiry Failed: rejected because the provider effect may already exist.
- Retry with a new provider operation ID immediately: rejected because it can duplicate the effect.
- Store only the current attempt state: rejected because recovery and audit require exact transition evidence.
- Mutate a conflicted attempt after manual repair: rejected because it erases the original terminal outcome.
- Issue a receipt from provider command exit status alone: rejected because target verification is required.

## Consequences and migration

Schema v7 adds immutable integration intent and finalized inputs/evidence references, append-only versioned events with lease and observation snapshots, immutable conflicts/inputs, receipts, conflict resolutions, and resolution-validation references. Fresh and v1–v6 databases upgrade additively under the serialized migration lock. Older binaries reject schema v7; rollback restores a pre-migration backup.

Provider evidence is opaque at this boundary. Native Git and GitButler adapters must later supply truthful observations, stable effect IDs, bounded subprocess behavior, and reconciliation implementations. This checkpoint does not claim a live provider mutation.

## Required proof

- Exact candidate/target binding, planning target observation, version CAS, and stable effect-operation uniqueness.
- Target-scoped live-lease exclusion, exact holder renewal/outcome authority, and exact-boundary expiry.
- Expiry-to-reconciliation without success/failure, repeated observations, NoEffectVerified termination, ResultVerified success, and explicit Diverged-to-Superseded replanning.
- Atomic conflict and receipt creation, separate exact validated resolution, and no receipt outside Succeeded.
- Exact metadata-operation replay and conflicting payload denial.
- Event reconstruction, historical readability, operation/source/collection validation, and conflict/receipt drift failure.
- Fresh and v1–v6 migration, strict static checks, and read-only domain review.

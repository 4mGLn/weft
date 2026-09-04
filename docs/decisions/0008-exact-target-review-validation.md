# ADR-0008: Exact-target review and validation evidence

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Review approvals and validation results become unsafe when attached to a mutable Change, Stack, branch, or provider reference. Weft needs durable evidence that remains historically intelligible after revision heads, candidate inputs, Dependencies, or Stack definitions advance. It must also distinguish factual target freshness from an explicit policy decision to reuse evidence.

## Decision

Review and validation use one provider-neutral ExactTarget. It is exactly one Change/revision pair or one CompositionCandidate identity and copies the source repository/context plus canonical artifact or candidate digest. Creation and every authoritative read reconstruct the source, verify exact revision ownership and canonical content or candidate provenance, compare all copied fields, and reject evidence dated before the source existed.

A ReviewRequest has immutable identity, one ExactTarget, requester/time provenance, a canonical non-empty duplicate-free reviewer set, and a recorded reuse policy. The only v1 policy is `new_submission_required`: review evidence never transfers to another target. Reviewer membership is finalized with the request.

Each ReviewSubmission has immutable identity and appends one requested reviewer's outcome, optional non-empty comments, and time to the request history. Outcomes are Approved, ChangesRequested, Rejected, and Blocked. Repeated submissions by one reviewer remain separate history; this checkpoint does not infer quorum, readiness, or an effective aggregate outcome.

A ValidationResult has immutable identity, one ExactTarget, type, environment, Passed/Failed/Blocked/Error outcome, execution ID, validator/time provenance, and scope. Scope is `exact_target` or a validator-declared reusable scope with a non-empty rationale. The declaration is durable metadata, not an automatic application decision.

Factual freshness is derived. A revision target is current only while it remains its Change head. A candidate target is current only while its exact inputs, Dependency sources, and Stack snapshot remain current under ADR-0007. A declared validation scope never changes this result. Applying evidence to another target, if supported later, must create a separate explicit reuse decision.

Requests, submissions, results, and reviewer membership are immutable. Mutations use the global operation registry; exact retries return the recorded object and conflicting reuse fails. SQLite immediate transactions atomically publish operation and evidence rows. Authoritative reads validate operation actor/time/kind and fail closed on missing children, malformed scope, target/source drift, or canonical-content loss.

## Alternatives

- Attach approvals or checks to a mutable Change/branch: rejected because later content would inherit unrelated evidence.
- Store only a revision or candidate ID: rejected because copied context/digest makes target intent auditable and drift detectable.
- Automatically reuse approval for identical content: rejected in v1 because review can cover provenance, dependency topology, and target context beyond bytes.
- Treat declared reusable validation scope as current on another target: rejected because declaration and application are separate decisions.
- Replace a reviewer's prior submission: rejected because it erases the sequence that explains the current decision.
- Persist a mutable stale flag: rejected because freshness derives from exact immutable source and current state.
- Implement quorum/readiness aggregation now: deferred until Change lifecycle policy is defined.

## Consequences and migration

Schema v6 adds immutable ReviewRequest, finalized reviewer, ReviewSubmission, and ValidationResult tables with exact revision-or-candidate shape constraints, requested-reviewer foreign keys, operation provenance triggers, and append-only guards. Fresh databases and v1–v5 upgrades apply the additive migration under the serialized migration lock. A populated concurrent v5 upgrade preserves candidate history. Older binaries reject schema v6; rollback restores a pre-migration backup.

The initial storage API can construct verified revision or candidate targets, create/read/replay review and validation evidence, list submission history, and derive freshness. Review aggregation, authorization, validator execution, external-system mappings, and explicit cross-target reuse application remain later work.

## Required proof

- Exactly-one target shape, exact revision ownership/canonical content, candidate provenance, copied context/digest, and temporal ordering.
- Non-empty canonical reviewer set, finalized membership, requested-reviewer enforcement, repeated submission history, and restart reconstruction.
- `new_submission_required` with no implicit approval transfer.
- Exact and declared-reusable validation scopes, with stale targets remaining factually stale.
- Revision-head and candidate-input/Dependency/Stack freshness without evidence mutation.
- Exact operation replay, cross-kind/payload conflict, immutable rows, outsider denial, and source-drift failure.
- Fresh and v1–v5 migration, including populated concurrent v5 preservation.

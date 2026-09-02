# Weft Domain Model

This document defines the normative entities, invariants, lifecycle, and concurrency behavior of Weft. Product intent lives in [GOAL.md](GOAL.md); implementation sequencing lives in [ROADMAP.md](ROADMAP.md).

## 1. Core entities

### Repository

A source-control repository known to Weft. Its identity must remain stable across local path changes and must include enough provider information to address exact source states.

### Change

A durable logical unit of software work with an immutable `change_id`. The identifier survives agent, workspace, provider, base, and content changes.

A Change owns histories of revisions, assignments, materializations, relationships, reviews, validations, conflicts, and integrations. Mutable summaries such as current state are projections over that durable state and history.

### ChangeRevision

A ChangeRevision is one exact, immutable version of a Change:

```text
ChangeRevision
├── revision_id
├── change_id
├── parent_revision_id
├── base
├── canonical_content
├── content_digest
├── provider_refs
├── created_at
├── created_by
└── metadata
```

`base` identifies an exact reproducible repository state. `canonical_content` is a durable, provider-independent artifact sufficient to reconstruct and verify the revision; it may be a patch, tree/object snapshot, or content-addressed artifact with guaranteed local availability. A provider-native reference may supplement canonical content but may never be its only durable representation.

A dirty working directory is not an implicit base or source of truth.

#### Linear revision invariant

The initial model has exactly one current head per Change. Every non-root revision has one parent, which must be the current head at creation time.

Creating a revision requires `expected_head_revision_id`. The operation atomically compares it with the current head and appends the new revision only when they match. A stale writer receives a concurrency error and must refresh, rebase its work, and retry explicitly. Revision forks and multi-parent revision merges are out of scope for the initial model.

Provider rebases or rewrites create new revisions; they never mutate existing ones.

### Workspace and Materialization

A Workspace is an execution environment such as an existing checkout, Git worktree, GitButler workspace, or isolated runtime.

A Materialization realizes one ChangeRevision in a workspace/provider environment:

```text
Materialization
├── materialization_id
├── change_revision_id
├── workspace_id
├── provider
├── provider_ref
├── state
├── created_at
└── released_at
```

One revision may have multiple materializations, and one workspace may contain materializations for multiple Changes where the provider supports it. Materialization identity, Change, exact revision, workspace, provider, creation time, and creator are immutable. Only the observed provider reference and state advance during its lifetime. At most one non-terminal Materialization for a Change may occupy one workspace/provider placement; terminal history remains addressable.

A Materialization starts `clean` only after its provider has verified that the recorded reference realizes the exact durable canonical revision. Subsequent provider observations use the exact Materialization version as compare-and-swap input and atomically append the resulting state, provider reference, non-empty provider evidence, actor, time, and operation ID. An exact operation retry returns its recorded outcome; a stale version or conflicting operation reuse fails without mutation. The immutable event history is authoritative and mutable projections must fail closed when they drift from it.

State distinguishes `clean`, `dirty`, `diverged`, `suspended`, `released`, and `invalidated`. These are provider observations rather than a prescribed workflow, so any non-terminal state may be followed by another verified non-terminal observation. A provider reference may advance while the state remains unchanged, but a transition that changes neither is invalid. `released` and `invalidated` are terminal and free the active placement. Capturing dirty work creates a new ChangeRevision through the head compare-and-swap operation and, when realized, a distinct Materialization; it never retargets or updates the revision bound to the existing Materialization.

### Assignment and lease

Responsibility is recorded as durable Assignment events rather than one mutable owner field. Subjects may be humans, agents, sessions, or integrations, with roles such as owner, implementer, reviewer, resolver, integrator, and observer.

An Assignment has immutable identity, Change, subject, role, assignment provenance, and an optimistic-concurrency version. Assignment and release are immutable events. Releasing requires the exact active version and retains the complete tenure. Assignments for different subject/role pairs may overlap; a second identical active subject/role assignment is rejected rather than silently duplicating responsibility.

A Lease grants temporary exclusive authority for one `(change_id, operation_key)` scope. Each acquisition has an immutable `lease_id`, holder, acquisition time, expiry, and optional predecessor lease. The scope has one monotonic version and at most one current lease projection. Acquire, renew, and release compare-and-swap the exact expected scope version and append immutable events in the same transaction.

The expiry boundary is exact: after acquisition, a lease is active only while `observed_at < expires_at`. Historical queries before acquisition report not-yet-acquired; an explicit release takes effect at its recorded timestamp, not retroactively. Renewal is allowed only while active and must extend expiry. Explicit release clears the current projection. Acquisition after expiry creates a new lease identity linked to the expired predecessor; it never revives or rewrites the old lease. Assignment history remains after a lease ends or work is handed off.

## 2. Relationship semantics

Weft keeps these relationships distinct:

- **Task decomposition:** a symmetric pair records that two Changes contribute within the same larger task decomposition; no ancestry, dependency, order, or transitive closure is implied.
- **Revision ancestry:** the linear `parent_revision_id` relation within one Change.
- **Dependency:** downstream Change B requires an exact accepted or declared revision of Change A for validation or integration.
- **Stack:** an explicit ordered collection of Change identities defining intended review and integration topology.
- **Related-to:** a symmetric pair records contextual relevance without ordering or inferred transitivity.

Task-decomposition and related-to records have immutable identity, kind, canonical unordered Change endpoints, and creation provenance. Removal uses the exact record version and appends immutable history; it never deletes or reinterprets the pair. A duplicate active kind/pair is rejected. These contextual records never imply readiness or composition inputs.

Dependency graphs are explicit, durable, directed, and acyclic. Adding an edge that creates a cycle fails atomically.

A dependency contract has immutable identity and direction from one downstream Change to one distinct upstream Change. Its current projection pins both the exact downstream `revision_id` that declared or consumed the requirement and the exact upstream `revision_id` it requires. Creation, explicit repin, and removal compare-and-swap the exact dependency version and atomically append actor, time, operation ID, expected version, and resulting pins. Repin replaces both exact pins as one operation and must change at least one; removal is terminal. At most one active dependency exists for one directed Change pair.

Freshness is derived rather than stored: a dependency is current only while both pins equal their Changes' current heads. If either Change advances, the dependency reports which side is stale while retaining its exact historical pins; Weft never silently retargets it. It remains stale until downstream work is revalidated or revised and the dependency is explicitly repinned, or the dependency is removed. Rejected upstream work blocks or stales dependents when Change lifecycle is available. Immutable candidate creation copies the exact dependency resolution it consumed. Change and revision deletion cannot leave dangling edges.

A Stack has immutable identity and creation provenance plus a versioned definition containing a policy and an ordered, non-empty, duplicate-free list of Change identities with explicit predecessor positions. Definition replacement compares the exact Stack version, advances it by one, and atomically records the complete resulting order, policy, actor, time, and operation ID. A no-op or stale replacement fails. Stack events are authoritative and mutable membership projections fail closed when they drift.

The initial policies are `order_only` and `predecessor_dependencies`. Order-only records composition topology without creating readiness edges. Predecessor-dependencies makes every member after the first require its direct predecessor when a candidate is resolved; it does not create or mutate a durable Dependency. Explicit Dependencies remain separate and must agree with the Stack order. A dependency does not imply Stack membership. Mutable Stack order or policy is never itself a review, validation, or integration input.

### CompositionCandidate

A CompositionCandidate is an immutable snapshot of exact work intended for review, validation, or integration:

```text
CompositionCandidate
├── candidate_id
├── repository_id
├── target_base
├── ordered_inputs[] { change_id, revision_id }
├── resolved_dependencies[]
├── stack_id / stack_version (optional)
├── content_digest
├── created_at
└── created_by
```

Candidate creation selects either an exact expected Stack version or an explicit ordered, duplicate-free Change list. In one metadata snapshot it resolves every input to its current exact head, verifies durable canonical content and one repository, resolves every active Dependency whose downstream is selected, requires each required upstream to be present earlier, rejects stale dependency pins, and adds Stack-predecessor requirements according to policy. Missing upstream inputs, reversed dependency order, a changed expected Stack version, or a Change without a head fails atomically.

The candidate records complete exact inputs, resolved Dependency IDs/versions/pins, implicit Stack-predecessor requirements, and the exact Stack ID/version/policy where applicable. Its `composition-candidate-v1` digest is SHA-256 over a length-prefixed binary encoding of repository/target base, optional Stack snapshot, ordered inputs, and canonically ordered resolved requirements. Candidate ID and creation provenance are excluded so identical correctness inputs have identical content digests. Stored bytes/fields are re-encoded and rehashed on read.

Later Change revisions, Dependency edits/removal, or Stack definition changes do not mutate an existing candidate; they make its corresponding input, dependency, or Stack snapshot stale and require a new candidate. Staleness is derived without rewriting candidate history. The target base remains exact; comparison with a live provider target is deferred to planning/reconciliation rather than silently updating the candidate.

For a single Change, a candidate may contain one input. This gives review, validation, and integration one exact targeting model without forcing all Changes into stacks.

## 3. Change lifecycle and readiness

The primary lifecycle is independent of any agent session:

```text
Draft → Ready → Active → InReview → Approved → Integrating → Integrated
```

Terminal alternatives are Rejected, Abandoned, and Superseded. A requested revision returns the Change to Active through creation of a new ChangeRevision.

Assignments, lease ownership, dependency readiness, validation status, conflict status, and general blocking are orthogonal projections rather than destructive lifecycle transitions. In particular, `Blocked` is recoverable and is not a terminal lifecycle state.

Transitions that affect correctness are validated and recorded atomically. A ChangeRevision remains immutable regardless of Change lifecycle.

## 4. Review and validation

Review and validation never target an unqualified mutable Change.

An ExactTarget identifies exactly one ChangeRevision or CompositionCandidate and copies its repository/base context plus canonical artifact or candidate digest. A ReviewRequest binds one target, requester, a non-empty duplicate-free reviewer set, creation provenance, and an explicit reuse policy. The v1 review policy is `new_submission_required`: an approval never transfers to another target. A ReviewSubmission has immutable identity and identifies the request, repeats its exact target, names one requested reviewer, records Approved, ChangesRequested, Rejected, or Blocked, optional comments, and submission time. Repeated submissions append history rather than overwriting it; readiness aggregation is a separate policy.

When a Change advances, or a new candidate resolves different inputs or target state, approvals for the previous target become stale. Review reuse is allowed only through an explicit recorded policy or new review submission; it is never inferred.

A ValidationResult has immutable identity and similarly records an ExactTarget, validation type, environment, Passed, Failed, Blocked, or Error outcome, execution ID, validator, timestamp, and scope. Tests, build, lint, type checking, security scans, and custom checks are validations. Scope is either `exact_target` or a non-empty validator-declared reusable scope with rationale. Target freshness remains factual: a result becomes stale when its exact revision is no longer the Change head or its candidate inputs/requirements/Stack become stale. A declared reusable scope does not silently make a stale result current; any later consumer that applies it to another target must record that explicit reuse decision.

Review requests, submissions, and validation results are immutable audit records. Creation validates exact revision ownership or candidate identity, copied context and digest, canonical content, actor/time provenance, and global operation replay. Reads revalidate those sources and fail closed on drift. Later target advancement never rewrites historical review or validation evidence.

## 5. Overlap, conflict, and resolution

Weft uses separate concepts:

- **Overlap:** a risk signal that exact revisions touch intersecting files, hunks, ranges, generated artifacts, or shared resources. It does not prove incompatibility.
- **IntegrationConflict:** a provider operation could not combine exact inputs as requested, such as a merge conflict, rebase conflict, patch failure, or GitButler conflicted commit.
- **ValidationFailure:** an exact revision or candidate fails a machine or external condition.
- **Semantic incompatibility:** may require tests, static analysis, or judgment; Weft does not claim generic automatic detection.

An IntegrationConflict has immutable identity and records the exact attempt, candidate and ordered revisions, provider state/evidence, creator, and time. The conflicted attempt remains terminal. A ConflictResolution is a separate immutable record that identifies the conflict, resolver, resulting exact revision or candidate, supporting ValidationResults, provider evidence, and time. Resolution never rewrites the conflict or resumes the failed attempt; integration replans with a new candidate or attempt.

## 6. Integration

Integration combines an immutable CompositionCandidate into an accepted target repository state.

### IntegrationAttempt

```text
IntegrationAttempt
├── integration_id
├── repository_id
├── candidate_id
├── target_ref
├── expected_target_revision
├── ordered_inputs[]
├── provider
├── strategy
├── operation_id
├── actor
├── state
├── review_refs[]
├── validation_refs[]
├── conflict_refs[]
├── started_at / finished_at
├── result_revision
└── provider_receipt
```

Allowed states are Planned, Running, Reconciling, Conflicted, Failed, Succeeded, Aborted, and Superseded. An attempt has immutable identity, repository/candidate/target/provider/strategy/effect-operation intent and a versioned projection reconstructed from append-only events. Each transition uses a separate global idempotency operation ID. The stable effect-operation ID is reused by an adapter while attempting or reconciling the same external mutation and never identifies an unrelated attempt.

Planning verifies candidate freshness, dependency and stack order, referenced approvals and validations, an explicit gate-policy attestation, provider capability evidence, and a provider observation whose target equals `expected_target_revision`. The candidate target base must equal that expected repository revision. Starting revalidates all of those facts and requires a recoverable execution lease scoped to `(repository_id, target_ref)` plus another provider observation equal to the expected target. A changed target causes a stale-target failure before Running rather than implicit replanning.

The provider may use merge, rebase, patch application, commit creation, or a provider-native strategy, but the selected strategy, exact inputs, capability/gate evidence, expected target, and stable effect-operation ID are recorded before execution. An execution lease authorizes one holder until its exact expiry; expiry never proves that a provider mutation did not occur and never permits blind re-execution.

Only Succeeded means the exact target was observed and verified at `result_revision`. The success transition atomically emits exactly one immutable IntegrationReceipt linking candidate, attempt, prior target, resulting target, provider, effect-operation ID, verification evidence, actor, and time. Conflicted or failed attempts retain all evidence. A conflict transition atomically creates its IntegrationConflict. If a crash, lease loss, timeout, cancellation, or provider response leaves the effect uncertain, the attempt enters Reconciling; it must not be reported as Failed, Aborted, or Succeeded merely from missing response.

Each reconciliation observation is immutable and records expected/observed target, provider evidence, actor/time, and one outcome: StillUncertain, NoEffectVerified, ResultVerified, or Diverged. StillUncertain and Diverged retain Reconciling. NoEffectVerified permits a terminal Failed or Aborted outcome without a receipt. ResultVerified may succeed only when the receipt records the same verified result. Exact Diverged evidence permits an explicit terminal Superseded transition without a receipt; only then may a new candidate and attempt be planned against the observed target. Reconciliation never silently changes expected target, candidate, strategy, or effect-operation identity.

External-effect retries use the same durable effect-operation ID and are allowed only while authority and provider reconciliation semantics make them safe. Metadata-transition retries use their recorded global operation IDs and cannot duplicate events, conflicts, resolutions, observations, or receipts. Replanning after changed inputs or target creates a new attempt with new identity and effect-operation ID.

## 7. Orchestration and agent protocol

Weft records readiness, assignments, leases, progress, requested actions, handoffs, and outcomes. It does not launch or supervise agent processes. Paseo, CI, other runtimes, shell automation, or humans perform execution.

The provider-neutral agent contract supports discovering Changes, acquiring a lease, inspecting the exact head, creating a materialization, producing a revision, reporting progress, requesting review or validation, handing off, releasing, and resuming. Agents never infer durable Change state solely from a dirty filesystem.

## 8. Persistence, concurrency, and recovery

Durable storage covers Changes, revisions, canonical artifacts, candidates, assignments, leases, materializations, relationships, reviews, validations, overlaps, conflicts, integration attempts and receipts, operations, and reconciliation events.

Correctness-sensitive transitions are atomic. Mutations carry the expected entity version; stale versions fail rather than overwrite. Lease decisions use one explicit observation timestamp throughout the transaction so the expiry boundary cannot change mid-transition. Locks may protect repository/provider and materialization operations, but must be scoped and recoverable rather than relying exclusively on a long-lived process.

Retryable operations are idempotent where practical. Non-idempotent operations use durable operation IDs. Reusing an operation ID returns its recorded outcome or resumes reconciliation instead of duplicating state.

Crash recovery distinguishes operations that completed, did not begin, partially applied, or require reconciliation. Expired leases never erase assignment or operation history.

## 9. Provider reconciliation and capabilities

Provider state can change outside Weft. Reconciliation compares known state with actual provider state and records divergence before creating a new revision, changing materialization state, or completing an operation. Provider state never silently overwrites Weft history.

Providers expose capabilities rather than pretending their semantics are identical. Capabilities may include inspecting revisions, creating materializations, applying canonical content, computing diffs, detecting overlaps, planning or attempting integration, capturing conflicts, publishing, and reconciling external state. Callers discover capabilities and receive an explicit unsupported result where needed.

Native Git and GitButler provider mappings must preserve domain identifiers and canonical artifacts even when provider objects are rewritten or removed.

The Native Git v1 mapping binds captured revisions to exact commit and tree observations while retaining Weft-owned Repository, Change, Revision, Candidate, and operation identities. Ordered Native Git composition requires exact commit chaining, not tree equivalence, and may use durable prior tree observations after intermediate provider commits disappear. Canonical blobs are staged through filter-independent plumbing and every materialized or composed tree is verified against its exact captured observation.

A Native Git integration plan binds one exact candidate base commit to one expected local target commit. Success requires exact ref compare-and-swap followed by verification of the controlled one-parent result commit, candidate tree, and stable effect-operation marker. After ambiguous execution, observing the expected target alone is StillUncertain rather than proof of no effect; exact no-effect evidence must establish that the mutation did not occur despite possible external ref advance/reset.

The GitButler v1 mapping is version- and schema-gated. A GitButler `changeId` is a replaceable `ProviderRef`, never a Weft Change or Revision identity. GitButler status stacks are normalized from provider tip-first order into exact base-to-tip commit inputs; every input must have the preceding exact commit as its first parent. A canonical export accepts only the currently observed `changeId`/commit pair and that commit's exact first parent, then stores provider-independent `tree-delta-v1` content through the Native Git object boundary. Missing provider metadata is divergence evidence, not proof of release or permission to infer reconnection.

GitButler local landing is supported only for an initialized SHA-1 project whose configured target is its repository-local `gb-local` remote, with a clean, conflict-free, fully represented stack that can fast-forward from the exact merge base to the exact candidate tip. The sealed plan retains the Weft repository identity, canonical provider locator, complete ordered inputs, target, result commit/tree, and stable effect-operation ID. Success or crash recovery requires the target to equal that exact planned tip and tree. A different target is Diverged; the unchanged expected target after an ambiguous command is StillUncertain. Canonical import, empty or published branch segments, provider reconnect, remote landing, and unvalidated JSON extensions return explicit unsupported errors.

## 10. Audit history

Important events include creation of Changes, revisions and candidates; assignment and lease changes; relationship edits; review and validation submissions; overlaps and conflicts; integration planning and completion; provider divergence; and reconciliation.

Every event records the actor, time, expected prior state, resulting state, affected domain identifiers, operation ID where relevant, and provider evidence. History must explain who acted, what exact state they used, what changed, and why the current state exists.

## 11. CLI and API contract

The reusable domain API is independent of the CLI. The CLI exposes Change lifecycle, revision capture, assignment and handoff, relationships, stacks and candidates, materializations, review, validation, conflicts, integration, history, and reconciliation.

Structured commands support a stable JSON schema, documented field semantics and exit codes, noninteractive operation, explicit confirmation for destructive actions, expected-version inputs, and operation IDs for retryable operations.

Representative commands include:

```bash
weft status --json
weft change create
weft change show <change-id> --json
weft change revise <change-id> --expected-head <revision-id>
weft change acquire <change-id>
weft change handoff <change-id>
weft dependency add <upstream>@<revision> <downstream>
weft stack create
weft candidate create --stack <stack-id>
weft materialization create <revision-id>
weft review request --candidate <candidate-id>
weft review submit <review-id>
weft validation record --candidate <candidate-id>
weft conflict list
weft integrate plan <candidate-id>
weft integrate run <integration-id> --operation-id <operation-id> --yes
weft reconcile
weft history
```

Exact syntax may evolve, but the domain operations and machine-readable guarantees are required.

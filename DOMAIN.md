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

One revision may have multiple materializations, and one workspace may contain materializations for multiple Changes where the provider supports it. Materialization lifetime never changes Change identity or revision content.

Materialization state must distinguish at least clean, dirty, diverged, suspended, released, and invalidated. Capturing dirty work creates a new ChangeRevision through the head compare-and-swap operation; it does not update the referenced revision.

### Assignment and lease

Responsibility is recorded as durable Assignment events rather than one mutable owner field. Subjects may be humans, agents, sessions, or integrations, with roles such as owner, implementer, reviewer, resolver, integrator, and observer.

Assignments may overlap. A Lease grants temporary exclusive authority for operations that cannot safely run concurrently. Leases expire, can be renewed, are observable, and can be safely recovered after process failure. Assignment history remains after a lease ends or work is handed off.

## 2. Relationship semantics

Weft keeps these relationships distinct:

- **Task decomposition:** Changes contribute to the same larger task; no ancestry or ordering is implied.
- **Revision ancestry:** the linear `parent_revision_id` relation within one Change.
- **Dependency:** downstream Change B requires an exact accepted or declared revision of Change A for validation or integration.
- **Stack:** an explicit ordered collection of Change identities defining intended review and integration topology.
- **Related-to:** contextual relevance without ordering.

Dependency graphs are explicit, durable, directed, and acyclic. Adding an edge that creates a cycle fails atomically.

A dependency contract pins the exact upstream `revision_id` and the downstream revision or candidate that consumed it. If the upstream Change advances, the dependency becomes stale; Weft never silently retargets it. It remains stale until the downstream work is revalidated, revised, explicitly repinned, or the dependency is removed. Rejected upstream work blocks or stales dependents. Deletion cannot leave dangling edges.

A Stack contains an ordered, duplicate-free list of Change identities with explicit predecessor positions. Stack membership may imply dependencies according to stack policy, but a dependency does not imply stack membership. Mutable stack order is never itself a review or integration input.

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

Candidate creation resolves every stack position and dependency to an exact revision, verifies ordering and dependency invariants, and computes a stable identity/digest. Later Change revisions, dependency edits, or stack reordering do not mutate an existing candidate; they require a new candidate. A candidate whose target or required inputs are no longer valid becomes stale but remains auditable.

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

A ReviewRequest identifies exactly one `revision_id` or `candidate_id`, its base/target context, requester, reviewers, and creation time. A ReviewSubmission identifies that request and target, reviewer, outcome, comments, and timestamp. Outcomes include Approved, ChangesRequested, Rejected, and Blocked.

When a Change advances, or a new candidate resolves different inputs or target state, approvals for the previous target become stale. Review reuse is allowed only through an explicit recorded policy or new review submission; it is never inferred.

A ValidationResult similarly records the exact revision or candidate, base/target, validation type, environment, result, execution ID, and timestamp. Tests, build, lint, type checking, security scans, and custom checks are validations. Results become stale when their exact target changes unless the validator explicitly declares a reusable scope and Weft records that decision.

## 5. Overlap, conflict, and resolution

Weft uses separate concepts:

- **Overlap:** a risk signal that exact revisions touch intersecting files, hunks, ranges, generated artifacts, or shared resources. It does not prove incompatibility.
- **IntegrationConflict:** a provider operation could not combine exact inputs as requested, such as a merge conflict, rebase conflict, patch failure, or GitButler conflicted commit.
- **ValidationFailure:** an exact revision or candidate fails a machine or external condition.
- **Semantic incompatibility:** may require tests, static analysis, or judgment; Weft does not claim generic automatic detection.

An IntegrationConflict is durable and records the candidate and revisions, provider state, attempted operation, resolver assignment, resulting revision or candidate, and validation results. Resolution creates new immutable domain objects and never erases the failed attempt.

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

Allowed states are Planned, Running, Conflicted, Failed, Succeeded, and Aborted. An attempt is append-only except for validated state transitions and completion fields.

Planning verifies candidate freshness, dependency and stack order, required approvals and validations, provider capability, and current target state. Starting the attempt requires an operation lease and compare-and-swap against `expected_target_revision`.

The provider may use merge, rebase, patch application, commit creation, or a provider-native strategy, but the selected strategy and exact inputs are recorded before execution. A changed target causes a stale-target failure rather than implicit replanning.

Only Succeeded means the target was verified at `result_revision`. Success emits an immutable integration receipt linking the candidate, attempt, prior target, resulting target, and provider evidence. Conflicted or failed attempts retain all evidence. If a crash or provider response leaves the result uncertain, the attempt remains Running or enters reconciliation; it must not be reported as successful until the target is verified.

Retries use the same durable `operation_id` and cannot create duplicate integration effects. Replanning after changed inputs or target creates a new attempt.

## 7. Orchestration and agent protocol

Weft records readiness, assignments, leases, progress, requested actions, handoffs, and outcomes. It does not launch or supervise agent processes. Paseo, CI, other runtimes, shell automation, or humans perform execution.

The provider-neutral agent contract supports discovering Changes, acquiring a lease, inspecting the exact head, creating a materialization, producing a revision, reporting progress, requesting review or validation, handing off, releasing, and resuming. Agents never infer durable Change state solely from a dirty filesystem.

## 8. Persistence, concurrency, and recovery

Durable storage covers Changes, revisions, canonical artifacts, candidates, assignments, leases, materializations, relationships, reviews, validations, overlaps, conflicts, integration attempts and receipts, operations, and reconciliation events.

Correctness-sensitive transitions are atomic. Mutations carry the expected entity version; stale versions fail rather than overwrite. Locks may protect repository/provider and materialization operations, but must be scoped and recoverable rather than relying exclusively on a long-lived process.

Retryable operations are idempotent where practical. Non-idempotent operations use durable operation IDs. Reusing an operation ID returns its recorded outcome or resumes reconciliation instead of duplicating state.

Crash recovery distinguishes operations that completed, did not begin, partially applied, or require reconciliation. Expired leases never erase assignment or operation history.

## 9. Provider reconciliation and capabilities

Provider state can change outside Weft. Reconciliation compares known state with actual provider state and records divergence before creating a new revision, changing materialization state, or completing an operation. Provider state never silently overwrites Weft history.

Providers expose capabilities rather than pretending their semantics are identical. Capabilities may include inspecting revisions, creating materializations, applying canonical content, computing diffs, detecting overlaps, planning or attempting integration, capturing conflicts, publishing, and reconciling external state. Callers discover capabilities and receive an explicit unsupported result where needed.

Native Git and GitButler provider mappings must preserve domain identifiers and canonical artifacts even when provider objects are rewritten or removed.

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

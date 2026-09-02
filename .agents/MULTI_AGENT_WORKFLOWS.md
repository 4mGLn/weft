# Multi-agent Workflows

Weft coordinates exact work; an external orchestrator decides when and where agent processes run.

## Dependency-aware execution

Create explicit Dependencies pinned to exact revisions. A Stack defines presentation/composition order but does not silently invent dependencies. Before review or integration, resolve current heads into an immutable CompositionCandidate. If an upstream or downstream head advances, treat candidate freshness as evidence and create a new candidate; never retarget the old one.

## Review and resolution

Reviewer Assignments may overlap implementation Assignments. ReviewRequests and submissions target an exact revision or candidate. A conflict resolver receives the immutable IntegrationConflict and creates a new Change revision/candidate; it does not rewrite the failed attempt's evidence. An old attempt remains target-excluding until verified terminal closure.

## Validation and integration

Validation pipelines record exact target, scope, evidence, and completion time. Integration ordering is driven by explicit dependencies and fresh immutable candidates. Each attempt has its own expected target, effect-operation ID, execution lease, reconciliation history, and receipt. Multiple agents may prepare work concurrently, but target mutation is exclusive.

## Orchestrator algorithm

1. Query Weft for exact current state.
2. Select only work whose explicit dependencies and required actions permit progress.
3. Create Assignment/Lease records before launching mutating work.
4. Pass durable IDs and acceptance proof to the runtime.
5. On completion or loss, re-read Weft rather than trusting session status.
6. Launch reviews, validations, resolvers, or downstream work only from durable Weft evidence.
7. Reconcile every ambiguous provider mutation before any retry.

This algorithm is advisory orchestration behavior, not an embedded scheduler. Weft does not own agent queues, process health, retry cadence, model choice, or compute placement.

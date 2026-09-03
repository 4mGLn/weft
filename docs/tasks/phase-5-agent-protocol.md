# Task Record: Phase 5 Agent Protocol

## Outcome and scope

- **User/operator result:** Provider-neutral agent operations can be executed,
  retried, handed off, and resumed from durable Weft state.
- **In scope:** Published protocol, explicit error semantics, and session-resume
  proof based on canonical revision content rather than workspace dirtiness.
- **Out of scope:** Agent process scheduling and Paseo-specific transport.
- **Affected invariants:** Exact revision/candidate targets, leases, operation
  idempotency, and uncertain-provider reconciliation.

## Acceptance criteria

1. The protocol covers discovery, acquisition, revision, handoff, materialization,
   review, validation, integration, and reconciliation.
2. Required stale/lost/unsupported/uncertain errors are explicit.
3. A fresh runtime resumes exact canonical work after prior session termination.

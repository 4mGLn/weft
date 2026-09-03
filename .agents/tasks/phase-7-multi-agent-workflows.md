# Task Record: Phase 7 Multi-Agent Workflows

## Outcome and scope

- **User/operator result:** External orchestrators can coordinate multiple
  implementers, reviewers, validators, and resolvers through exact durable Weft
  requests and ordering rules.
- **In scope:** Dependency-aware readiness, assignment/handoff, validation,
  candidate composition, integration ordering, and explicit blocking semantics.
- **Out of scope:** Agent scheduling, supervision, queues, or process ownership.
- **Affected invariants:** Exact dependency pins, immutable candidates, leases,
  exact review/validation targets, and reconciliation of uncertain integration.

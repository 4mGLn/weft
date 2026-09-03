# Multi-Agent Workflow Contract v1

This contract lets an orchestrator request work from Weft durable state. It
does not start, stop, or supervise agents.

## Workflow requests

| Workflow | Durable prerequisite | Orchestrator action |
| --- | --- | --- |
| Execute a Change | exact Change and acquired operation lease | Launch an implementer only after `change acquire` succeeds. |
| Review | exact revision/candidate and review request | Assign reviewer subject; record immutable review submission. |
| Resolve conflict | durable conflict record and candidate | Assign resolver by handoff; create a successor revision, never mutate the conflicted one. |
| Validate | exact revision/candidate | Run external pipeline and record its execution ID/status. |
| Compose | exact stack version and `change@revision` inputs | Create a new immutable candidate; never resolve current heads implicitly. |
| Integrate | fresh candidate, leases, expected target, operation ID | Plan/run in order; reconcile uncertainty instead of retrying mutation blindly. |

## Ordering rules

1. Dependencies pin upstream revisions; a downstream workflow must inspect
   staleness before execution.
2. Stack order is a versioned immutable input. A changed order needs a new stack
   version and candidate.
3. Integration order is determined by explicit candidate inputs and leases, not
   by agent completion order.
4. A review, validation, or conflict resolution applies only to its exact target.

## Readiness and blocking projection

An orchestrator computes readiness using ordinary CLI queries: inspect Change
history, acquire the required lease, verify candidate freshness, then plan the
requested integration. Blocking states are explicit: held/lost lease, stale
head/candidate/stack, validation failure, provider conflict, unsupported
capability, or uncertain integration. The orchestrator may notify or launch
agents in response, but no projection grants authority to mutate Weft state.

## Minimal workflow sequence

```text
inspect exact state → acquire lease → materialize exact revision
→ execute external work → append revision CAS → validate/review
→ compose immutable candidate → plan/run/reconcile integration
```

Each arrow is restart-safe because Weft records its durable IDs, expected state,
and outcomes. Agent process identity is intentionally absent from the model.

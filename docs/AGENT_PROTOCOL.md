# Weft Agent Protocol v1

Weft coordinates durable Change state; it does not schedule, supervise, or
terminate agent processes. Clients may be humans, Codex, Paseo, CI, or another
runtime. Every mutation uses the CLI v1 JSON contract and explicit state path.

## Provider-neutral operations

| Operation | CLI surface | Exact durable inputs |
| --- | --- | --- |
| Discover/inspect | `status`, `change show`, `history` | Change or provider path as applicable |
| Acquire/renew/release | `change acquire|renew|release` | Change, operation, holder, explicit time/expiry |
| Create revision | `change revise` | Change, expected head, exact base, provider content |
| Assign/handoff | `change assign|handoff` | Change, immutable assignment ID, actor/time |
| Dependencies/stacks/candidates | `dependency`, `stack`, `candidate` | Exact revisions and expected stack version |
| Materialize | `materialization create|transition` | Exact revision and expected lifecycle state |
| Review/validate | `review`, `validation` | Exact revision or candidate target |
| Integrate/reconcile | `integrate`, `reconcile`, `conflict list` | Immutable candidate, operation ID, expected target, receipt/reconciliation IDs |

## Required error handling

Agents must not reinterpret any of these conditions as success:

- `stale revision head`, `stale stack version`, or stale materialization state:
  re-read durable state and create an explicit next action.
- `lease held` or `lease lost`: stop exclusive work; acquire/recover before retry.
- stale candidate or changed target: plan a new candidate/integration; do not
  retarget the existing attempt.
- unsupported provider capability/version: report the explicit error and choose
  another supported operation only with operator direction.
- uncertain integration: preserve the running attempt and use reconciliation;
  never emit a success receipt from inference.

## Session resume rule

An agent may resume only from `--state` durable metadata, canonical artifact
content, exact revision/candidate IDs, and provider observations. Dirty workspace
files, a prior conversation, or an agent process identifier are never resume
authority. The resumed agent first inspects its Change/history, obtains or
recovers the required lease, materializes an exact revision if needed, and then
records a new revision or outcome through the normal CAS/operation-ID path.

## Compatibility

The protocol is versioned by the CLI JSON `schemaVersion`. Clients must reject
unknown major schema versions and must not depend on provider raw JSON.

# Weft Agent Protocol v1

Weft coordinates durable Change state; it does not schedule, supervise, or
terminate agent processes. Clients may be humans, Codex, Paseo, CI, or another
runtime. Every mutation uses the `weft.cli.v1` JSON envelope and an explicit
state path. The normative, repository-maintained copy is
[`../.agents/AGENT_PROTOCOL.md`](../.agents/AGENT_PROTOCOL.md).

## Process contract

```text
weft --format json --state-dir PATH <noun> <verb> [options]
```

Global options precede the command. JSON mode emits one envelope on stdout and
the exit status is part of the protocol. Mutations carry caller-owned
`--operation-id`, `--actor`, and `--at` values. Head/version-changing commands
also carry the exact observed expected value. Reusing an operation ID with the
same intent replays its recorded result; a different intent is a conflict.

## Provider-neutral operations

| Agent intent | CLI surface | Exact durable checkpoint |
| --- | --- | --- |
| Discover provider | `native-git discover`, `gitbutler discover` | Capability and locator evidence |
| Acquire work | `assignment create`, `lease acquire` | Assignment/lease ID and version |
| Inspect exact work | `change show`, `change history`, `candidate show` | Change/revision/candidate ID |
| Materialize | `native-git materialize` or `materialization create` | Exact revision and Materialization ID |
| Publish progress | `native-git capture` or `revision append` | Expected Change head and artifact digest |
| Handoff | `assignment create`, then `assignment release` | Both immutable tenures |
| Review/validate | `review request|submit`, `validation record` | Exact revision or candidate target |
| Compose | `stack create|replace`, `candidate create` | Exact ordered revision inputs |
| Integrate | `integration plan`, provider execute command | Expected target and stable effect ID |
| Recover uncertainty | `integration uncertain`, provider `reconcile-integration` | Reconciliation outcome before closure |
| Release workspace | Provider release, `lease release`, `assignment release` | Observed versions and `--yes` |

## Required error handling

Agents must not reinterpret stale heads/versions, held or lost leases, stale
candidates, changed targets, unsupported capabilities, provider failures, or
uncertain integration as success. Reload durable state and create an explicit
next action. A provider mutation with an ambiguous result must be recorded and
reconciled; it must never be blindly retried.

## Session resume rule

An agent may resume only from `--state-dir` durable metadata, canonical artifact
content, exact revision/candidate IDs, and provider observations. Dirty workspace
files, a prior conversation, or an agent process identifier are never resume
authority. The resumed agent inspects its Change/history, obtains or reclaims
the required lease, materializes an exact revision in a new workspace if needed,
and records the next revision or outcome through the normal CAS and operation-ID
path.

## Compatibility

Clients must reject unknown major protocol schemas and must not depend on raw
provider JSON. The installed Paseo action adapter uses the same CLI contract;
see [Paseo Integration](PASEO_INTEGRATION.md).

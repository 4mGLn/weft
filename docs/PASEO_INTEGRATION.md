# Paseo Integration v2

Paseo is an external agent/workspace launcher. Weft remains the source of truth
for Change, revision, assignment, lease, candidate, materialization, and
integration state. A Paseo workspace ID or agent ID is recorded only as an
assignment subject or provider/workspace reference; it never replaces Weft IDs.

## Environment contract

Paseo-launched actions provide these explicit variables:

```text
WEFT_STATE_DIR=/absolute/path/to/weft-state
WEFT_CHANGE_ID=change-id
WEFT_ACTOR=paseo-agent-or-human
WEFT_NOW_UNIX_MS=milliseconds
```

An external launcher may use its workspace ID as the adapter's `WORKSPACE_ID`
or assignment subject argument. The action shim refuses to invent missing
identities or timestamps.

## Action adapter

The installed `weft-paseo-action` command (and its source copy at
`scripts/paseo-weft-action.sh`) translates one Paseo lifecycle action into one
current `weft.cli.v1` command. Unix-like release archives include the adapter
as `bin/weft-paseo-action`; Windows archives currently expose the same bridge
and CLI contract but require a PowerShell caller to perform this mapping.
Set `WEFT_BIN` only when the adapter should use a specific binary; otherwise it
uses a sibling installed `weft`, a `weft` on `PATH`, or the source workspace's
Cargo binary.

Every action keeps IDs, expected versions, operation IDs, actor, and time
explicit. The supported actions are:

| Action | Arguments | Weft operation |
| --- | --- | --- |
| `assign` | `ASSIGNMENT_ID OPERATION_ID [ROLE]` | `assignment create` |
| `acquire` | `LEASE_ID OPERATION EXPECTED_VERSION EXPIRES_AT OPERATION_ID` | `lease acquire` |
| `renew` | `LEASE_ID EXPECTED_VERSION EXPIRES_AT OPERATION_ID` | `lease renew` |
| `checkpoint` | `REVISION_ID EXPECTED_HEAD BASE_REVISION PROVIDER_REVISION OPERATION_ID` | `native-git capture` |
| `materialize` | `MATERIALIZATION_ID WORKSPACE_ID REVISION_ID PROVIDER_REVISION DESTINATION OPERATION_ID` | `native-git materialize` |
| `observe` | `MATERIALIZATION_ID EXPECTED_VERSION WORKTREE PROVIDER_REVISION OPERATION_ID` | `native-git observe-materialization` |
| `release-materialization` | `MATERIALIZATION_ID EXPECTED_VERSION WORKTREE OPERATION_ID` | `native-git release-materialization` |
| `release-lease` | `LEASE_ID EXPECTED_VERSION OPERATION_ID` | `lease release` |
| `release-assignment` | `ASSIGNMENT_ID EXPECTED_VERSION OPERATION_ID` | `assignment release` |
| `handoff` | `ASSIGNMENT_ID SUBJECT_ID OPERATION_ID [ROLE] [SUBJECT_KIND]` | `assignment create` |
| `history` | none | `change history` |

The common environment is `WEFT_STATE_DIR`, `WEFT_CHANGE_ID`, `WEFT_ACTOR`, and
`WEFT_NOW_UNIX_MS`. The adapter defaults `WEFT_SUBJECT_KIND` to `agent`,
`WEFT_SUBJECT_ID` to `WEFT_ACTOR`, and `WEFT_ASSIGNMENT_ROLE` to `implementer`.
Native Git actions require `WEFT_REPOSITORY`; checkpoint/capture additionally
requires `WEFT_REPOSITORY_ID` because it creates the durable Repository binding.

Paseo decides when to launch, notify, or stop an agent. Weft decides whether a
lease can be acquired and records all resulting state. A Paseo outage cannot
block direct `weft --state-dir ...` operation.

## Resume and blocking

On a resumed Paseo session, invoke `history`, acquire a new lease with the
observed scope version, and materialize the exact recorded revision into a new
workspace. A replacement lease has a new ID and points to the expired
predecessor; it never revives the old lease. If the lease is held, stale, or
lost, surface that Weft JSON error as a blocking reason; do not run an
exclusive provider mutation. Providers and candidates are still reconciled
through Weft's normal CLI commands.

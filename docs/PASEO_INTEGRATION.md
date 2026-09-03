# Paseo Integration v1

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

The optional `PASEO_WORKSPACE_ID` is passed as a Weft materialization workspace
reference or assignment subject. The action shim refuses to invent missing
identities or timestamps.

## Supported requested actions

`scripts/paseo-weft-action.sh` supports:

- `acquire`: obtain an exclusive operation lease before delegated work;
- `handoff`: append a durable handoff assignment;
- `history`: inspect durable Change evidence.

Paseo decides when to launch, notify, or stop an agent. Weft decides whether a
lease can be acquired and records all resulting state. A Paseo outage cannot
block direct `weft --state ...` operation.

## Resume and blocking

On a resumed Paseo session, first invoke `history` and then `acquire`. If the
lease is held, stale, or lost, surface that Weft JSON error as a blocking reason;
do not run an exclusive provider mutation. Providers and candidates are still
reconciled through Weft's normal CLI commands.

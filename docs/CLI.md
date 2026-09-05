# Weft CLI v1

`weft` is a local, noninteractive executable. Human output is the default;
`--format json` emits one `weft.cli.v1` envelope to standard output. JSON errors
also use standard output so callers can parse one machine-readable result. The
state directory defaults to `.weft` and can be explicitly set with
`--state-dir <directory>`.

## Global options

```text
weft [--format human|json] [--state-dir PATH] [-v|--verbose] <command>
weft [-V|--version]
```

- `-V` and `--version` print the runtime version and accept no arguments.
- `-v` and `--verbose` emit one diagnostic line to standard error containing
  the parsed command, format, and state directory. They never alter the human
  result or JSON envelope on standard output.

## Stable JSON envelope

Successful results include `"schemaVersion": 1` and a `kind` field. Errors are
written to standard error as:

```json
{"schemaVersion":1,"error":{"code":"usage|domain","message":"..."}}
```

Exit code `0` is success, `2` is invalid command syntax/input, and `3` is a
domain or provider failure. A nonzero result never asserts a successful provider
mutation; uncertain provider outcomes remain durable reconciliation work.

## Commands

```text
weft [--state DIR] status --json
weft [--state DIR] change create CHANGE --json
weft [--state DIR] change show CHANGE --json
weft [--state DIR] change revise CHANGE --repository PATH --base COMMIT \
  --revision REVISION --expected-head REVISION|none --json
weft [--state DIR] change assign CHANGE --assignment ID --subject SUBJECT \
  --role ROLE --actor ACTOR --at UNIX_MS --json
weft [--state DIR] change acquire CHANGE --operation OPERATION --holder HOLDER \
  --now UNIX_MS --expires UNIX_MS --json
weft [--state DIR] change renew CHANGE --operation OPERATION --now UNIX_MS \
  --expires UNIX_MS --json
weft [--state DIR] change release CHANGE --operation OPERATION --now UNIX_MS --json
weft [--state DIR] change handoff CHANGE --assignment ID --to SUBJECT --actor ACTOR \
  --at UNIX_MS --json
weft [--state DIR] dependency add UPSTREAM@REVISION DOWNSTREAM --json
weft [--state DIR] stack create STACK CHANGE... --json
weft [--state DIR] stack revise STACK EXPECTED_VERSION CHANGE... --json
weft [--state DIR] candidate create CANDIDATE CHANGE@REVISION... --json
weft [--state DIR] materialization create ID --revision REVISION --workspace ID \
  --provider PROVIDER --provider-ref REF --actor ACTOR --at UNIX_MS --json
weft [--state DIR] materialization transition ID --expected-state STATE \
  --next-state STATE --actor ACTOR --at UNIX_MS --json
weft [--state DIR] review request ID --target revision:ID|candidate:ID \
  --requester ACTOR --reviewers REVIEWERS --at UNIX_MS --json
weft [--state DIR] review submit ID --request REQUEST --reviewer ACTOR \
  --outcome approved|changes-requested|rejected|blocked --comments TEXT \
  --at UNIX_MS --json
weft [--state DIR] validation record ID --target revision:ID|candidate:ID \
  --kind KIND --environment ENVIRONMENT --status passed|failed|blocked \
  --execution EXECUTION --at UNIX_MS --json
weft [--state DIR] integrate plan ID --candidate CANDIDATE --repository-id ID \
  --target-ref REF --expected-target COMMIT --provider native-git \
  --strategy STRATEGY --operation-id ID --actor ACTOR --at UNIX_MS --json
weft [--state DIR] integrate run ID --repository PATH --destination PATH \
  --receipt-id ID --conflict-id ID --reconciliation-id ID --now UNIX_MS --yes --json
weft [--state DIR] reconcile integration ID --repository PATH --expected-result COMMIT \
  --receipt-id ID --reconciliation-id ID --actor ACTOR --at UNIX_MS --json
weft [--state DIR] conflict list --json
weft [--state DIR] history CHANGE --json
```

Native Git is the supported CLI mutation provider in v1. GitButler remains
available through the reusable provider API; a CLI command must return an
explicit unsupported error until its same durable run/reconcile workflow is
exposed.

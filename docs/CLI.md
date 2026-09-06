# Weft CLI v1

`weft` is a local, noninteractive boundary for agent runtimes, orchestrators, and
operators. It is not the everyday workflow UI for a human who has already wired an
agent runtime; see [Runtime wiring](RUNTIME_WIRING.md) for that path.

## Global syntax

```text
weft [--format human|json] [--state-dir PATH] [-v|--verbose] <command>
weft [-V|--version]
```

- Human output is the default.
- `--format json` emits exactly one `weft.cli.v1` envelope on standard output,
  including errors. Agent callers consume that envelope and the process exit code
  together.
- `--state-dir PATH` selects local durable state. The default is `.weft`.
- `-v` / `--verbose` write one bounded invocation diagnostic to standard error;
  they never change standard output.
- `-V` / `--version` print the runtime version and accept no other arguments.

## Human setup and diagnostics

```text
weft setup [--project-dir PATH] [--runtime auto|all|NAME,...]
weft doctor [--project-dir PATH]
```

Run `weft setup` once in a repository after installation. It initializes state,
detects supported agent/orchestration executables without running them, writes the
runtime bridge, and safely maintains project instruction blocks where supported.
`weft doctor` only reports state/wiring health; it never repairs or mutates it.

Known runtime names are `codex`, `claude-code`, `gemini-cli`, `paseo`, `omc`, `omg`,
and `omx`. `auto` wires only currently detected executables. `all` prepares the
known adapters regardless of current availability. Read [Runtime wiring](RUNTIME_WIRING.md)
for support boundaries and lifecycle responsibility.

## Agent protocol command families

These commands are the machine-facing protocol. Mutations require caller-owned
operation IDs, actors, timestamps, and expected heads or versions where relevant.
They never prompt. Terminal transitions require explicit `--yes`.

```text
change create|show|history
revision append
assignment create|list|release
lease acquire|show|renew|release
relationship create|list|remove
dependency create|list|repin|remove
stack create|show|replace
candidate create|show|freshness
materialization create|show|list|transition
review request|show|submit|submissions
validation record|show
integration plan|show|start|renew|uncertain|reconcile|conflict|succeed|finish|abort|supersede
native-git discover|inspect|capture|materialize|observe-materialization|release-materialization
native-git execute-integration|reconcile-integration
gitbutler discover
```

Use `weft --help` for the complete current grammar. The durable ownership,
recovery, provider, and error rules live in the [agent protocol](../.agents/AGENT_PROTOCOL.md).

## Exit behavior

| Exit | Meaning | Agent action |
| --- | --- | --- |
| `0` | Command completed or inspection completed | Consume returned durable state. |
| `1` | Local problem | Repair local path, permissions, or configuration. |
| `2` | Invalid invocation | Correct the command; do not retry unchanged. |
| `3` | Durable record not found | Rediscover or stop. |
| `4` | Concurrency conflict | Reload exact state and re-plan. |
| `5` | Unsupported capability | Choose a proven provider capability. |
| `6` | Provider failure | Inspect provider state; reconcile ambiguity. |
| `7` | Integrity failure | Stop and preserve evidence. |

An uncertain provider mutation is never ordinary retry permission. Record and
reconcile it through the integration protocol before making a success claim.

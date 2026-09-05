# Runtime Wiring

Weft is coordination infrastructure for concurrent agents. A person installs it
and runs setup once in a repository; they then continue to start Codex, Claude
Code, Gemini CLI, Paseo, or another orchestrator in the usual way.

```bash
cd your-project
weft setup
weft doctor
```

`setup` initializes Weft state, writes `.weft/runtime-bridge.json` in the
project as a discoverable pointer, and detects executable runtimes on `PATH`.
When `--state-dir` selects an external state location, it also keeps the
authoritative bridge beside that state. It never launches a detected executable, reads a
credential, changes user-home configuration, or schedules an agent.

## What setup wires

| Runtime | Setup behavior | Boundary |
| --- | --- | --- |
| Codex | Maintains a marked block in `AGENTS.md` | Codex remains the agent runtime. |
| Claude Code | Maintains a marked block in `CLAUDE.md` | Claude Code remains the agent runtime. |
| Gemini CLI | Maintains a marked block in `GEMINI.md` | Gemini CLI remains the agent runtime. |
| Paseo | Publishes an entry in the runtime bridge | Paseo remains the launcher/supervisor. |
| OMC, OMG, OMX | Publishes an entry in the runtime bridge | Their adapters remain external to Weft. |

The instruction block makes the durable protocol visible to supported code-agent
surfaces. It tells agents to use Weft's JSON CLI for shared coordination, to acquire
authority before shared mutation, and to checkpoint/reconcile rather than trusting
ephemeral sessions or provider status.

Bridge-only runtimes are discoverable but are not claimed as native lifecycle
integrations. A native adapter requires its own proof that acquisition, checkpoint,
session replacement, and release work end-to-end.

## Select runtimes explicitly

Use `auto` (the default) to wire only executables currently visible on `PATH`.
Use `all` to prepare every known adapter even when a runtime is not installed, or a
comma-separated list when only some are wanted.

```bash
weft setup --runtime codex,claude-code,gemini-cli
weft setup --runtime paseo
```

Setup is idempotent. Existing content in `AGENTS.md`, `CLAUDE.md`, and `GEMINI.md`
is preserved; Weft changes only the paired markers below. If markers are missing,
duplicated, or malformed, setup stops without modifying any wiring file.

```markdown
<!-- weft:runtime-wiring:start -->
...
<!-- weft:runtime-wiring:end -->
```

## Runtime bridge

`.weft/runtime-bridge.json` is local configuration and is intentionally ignored by
Git. It records the state location, `weft.cli.v1` protocol version, selected
runtimes, their executable names, setup-time availability, and whether Weft wrote a
project instruction block or only a bridge entry. Agents first read the project-local
bridge and pass its `state_dir` as `--state-dir`; this keeps an external shared state
location available from an isolated workspace without embedding an absolute path in
tracked instruction files.

An orchestrator reads that bridge, then uses the JSON protocol to create or inspect
durable Changes, Assignments, Leases, revisions, candidates, and integration
records. It launches and supervises processes itself; Weft never becomes a process
scheduler.

## Diagnose without changing anything

```bash
weft --format json doctor
```

Doctor verifies the state-directory shape and SQLite header, parses both bridge
locations, checks managed instruction blocks, and checks whether each configured
executable is currently visible on `PATH`. Its JSON `healthy` field is the authoritative summary.
A completed diagnostic may return `healthy: false`; that reports a condition to fix,
not an implicit repair.

## Agent-facing protocol

Agents and orchestrators use JSON rather than parsing human text:

```bash
weft --format json --state-dir .weft change show --change-id change-123
```

They must preserve Weft identifiers and exact expected versions. A runtime session,
prompt, branch, or workspace is not durable Change identity. Read the full
[agent protocol](../.agents/AGENT_PROTOCOL.md) before writing an orchestrator
adapter.

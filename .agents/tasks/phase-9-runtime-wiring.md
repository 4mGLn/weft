# Task Record: Phase 9 runtime wiring

## Outcome and scope

- **User/operator result:** One `weft setup` invocation initializes local state,
  wires supported project instruction surfaces, and exposes a runtime-neutral
  bridge for existing agent runtimes and orchestrators.
- **In scope:** Idempotent local setup, runtime discovery, managed instruction
  blocks, a machine-readable manifest, wiring diagnosis, a Codex/Claude
  Code/Gemini CLI reference path, and orchestrator-facing bridge semantics.
- **Out of scope:** Launching, scheduling, monitoring, selecting models for, or
  taking authority over external agents; remote control planes; credentials.
- **Affected domain invariants:** Assignment and lease authority, exact revision
  identity, durable session replacement, and the external-scheduler boundary.
- **Provider/runtime scope:** Codex, Claude Code, Gemini CLI, Paseo, OMC, OMG,
  and OMX discoverability; provider-neutral bridge behavior.
- **Compatibility surface:** CLI, local configuration, documentation, release.

## Acceptance criteria

1. `weft setup` safely initializes a project and is idempotent across repeated
   invocations.
2. Setup never overwrites user-authored agent instructions; it manages only its
   marked block and fails safely when markers are malformed.
3. Runtime wiring declares exactly what is configured, detected, unavailable, or
   unsupported in one stable JSON result.
4. Wired agent instructions direct runtimes to the durable JSON protocol without
   treating prompts, sessions, branches, or workspaces as Change identity.
5. `weft doctor` validates local state and wiring without mutating it.
6. Tests cover setup, repeat setup, existing instruction preservation, malformed
   marker denial, unavailable runtimes, and machine-readable output.

## Risks

- **Data/security:** Setup must not read credentials or execute detected tools.
- **Concurrency/crash recovery:** Managed-file publication must be atomic; a
  failed setup must not claim a completed wiring.
- **Provider divergence/compatibility:** Runtime-specific files are convenience
  adapters, not proof that a provider accepts or executes Weft operations.
- **Upgrade/rollback:** The manifest format is versioned and remains local;
  removal must preserve user-authored instruction content.

## Evidence and plan

- Relevant paths: `crates/weft-cli`, `.agents/AGENT_PROTOCOL.md`,
  `.agents/PASEO.md`, `docs/GOAL.md`, `docs/ROADMAP.md`.
- Official version-sensitive evidence: current Gemini CLI project context/MCP
  documentation; Codex instruction-file behavior is limited to documented
  `AGENTS.md` guidance.

1. Define the runtime-wiring boundary and manifest — ADR and roadmap proof.
2. Implement setup/doctor and managed instruction blocks — focused CLI tests.
3. Add runtime/reference documentation — link checker and JSON contract proof.
4. Run full workspace, clean setup/repeat/denial checks — `make check`.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-cli wiring` | Passed | Managed-block, runtime detection, and non-mutating doctor coverage |
| CLI/JSON contract | `cargo test -p weft-cli setup_` | Passed | Setup, doctor, JSON envelope, and preflight path |
| Idempotency and denial | `setup_wires_project_context_idempotently_and_denies_malformed_markers` | Passed | Existing content, repeat bytes, malformed-marker, and no-state denial proof |
| Static/harness | `make check` | Passed | Documentation links, 17 CLI tests, full workspace tests, formatting, strict Clippy |
| Package/deployment | `make package-release VERSION=v0.2.0 && make test-release ARCHIVE=dist/weft-0.2.0-x86_64-unknown-linux-musl.tar.gz` | Passed | Clean installed binary ran setup, doctor, durable Change restart, and uninstall retention |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0016; reject embedded scheduling,
  user-home configuration mutation, and unsupported native-adapter claims.
- **Residual risks:** Native runtime plug-ins and lifecycle hooks require
  runtime-specific capability proof before Weft can claim automatic lifecycle
  capture.
- **Unavailable evidence:** No runtime process is launched by setup; native
  runtime hooks and cross-worktree environment injection remain unproven.
- **Follow-up:** Add native adapters only after each runtime's versioned config
  and lifecycle semantics are proved end-to-end.

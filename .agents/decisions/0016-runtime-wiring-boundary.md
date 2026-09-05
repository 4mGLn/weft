# ADR-0016: Local runtime wiring without embedded scheduling

- **Status:** Accepted
- **Date:** 2026-09-05

## Context

Weft already exposes durable, provider-neutral agent operations through its JSON
CLI, but users still have to understand and invoke the internal domain workflow.
The intended user experience is simpler: install Weft once, wire a repository to
the agent tools already in use, and let those tools use Weft as their shared
coordination layer.

Codex, Claude Code, Gemini CLI, Paseo, OMC, OMG, and OMX have different prompt,
configuration, lifecycle, and scheduling surfaces. Treating a detected executable
as authority to start it, edit user-level configuration, or infer a completed
operation would violate Weft's scheduler boundary and make unproven compatibility
claims.

## Decision

`weft setup` owns only project-local, reversible wiring. It initializes Weft
state, detects requested runtimes without executing them, writes a versioned
machine-readable runtime bridge, and maintains explicitly marked instruction
blocks in supported project instruction files. The blocks teach runtimes to use
the `weft.cli.v1` JSON protocol and retain the invariant that an external
orchestrator creates/supervises processes while Weft owns durable coordination.

`weft doctor` is read-only and reports whether state, bridge, instruction blocks,
and requested runtime executables are present. It never repairs state silently.

The bridge is provider-neutral. Runtime-specific adapters may consume it, but a
runtime is only reported as natively integrated after a versioned end-to-end proof
shows setup, invocation, lease/assignment acquisition, checkpoint, recovery, and
release behavior. Paseo remains the first documented launcher mapping; setup does
not register projects, create workspaces, or launch Paseo agents.

## Consequences

- A developer's normal agent entry point stays Codex, Claude Code, Gemini CLI, or
  their chosen orchestrator; Weft is coordination infrastructure beneath it.
- Setup does not modify user-home configuration, run detected executables, inspect
  credentials, or turn Weft into a scheduler.
- Project instruction files may contain user content. Weft modifies only paired
  managed markers and fails if their structure is ambiguous.
- The initial bridge gives generic orchestrators a stable discovery surface while
  preserving room for proven native adapters.

## Alternatives rejected

- A Weft-owned agent runner: conflicts with the product boundary and duplicates
  existing runtime/orchestrator responsibilities.
- Unconditional edits to `~/.codex`, `~/.claude`, or `~/.gemini`: too broad,
  nonportable, and unsafe for a project-local setup command.
- Claiming every detected executable is fully integrated: detection is not proof
  of configuration support or lifecycle correctness.

## Required proof

- Setup creates valid local state and a versioned bridge without executing a
  runtime or reading environment secrets.
- Repeated setup is byte-stable; pre-existing instruction content survives.
- Malformed managed markers fail without changing files.
- Doctor detects missing/corrupt state and unavailable runtimes without mutation.
- Stable JSON and human output, CLI exit behavior, documentation, and full gate
  remain compatible.

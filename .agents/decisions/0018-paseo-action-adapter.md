# ADR-0018: Paseo action adapter and lifecycle proof

- **Status:** Accepted
- **Date:** 2026-09-10

## Context

Runtime wiring made Weft discoverable to Paseo, but a bridge by itself still
left each launcher integration to translate durable assignments, leases,
materializations, and revisions into the current CLI grammar. The old helper
scripts also used obsolete flags and commands, so a freshly wired runtime could
not reliably resume work across processes.

The adapter must preserve Weft's identity, compare-and-swap, canonical-content,
lease, and reconciliation invariants while keeping process launch and
scheduling in Paseo.

## Decision

Keep the existing `weft.runtime-bridge.v1` shape backward-readable and add an
optional `adapter` entry. A Paseo setup entry identifies the current
`paseo-action-adapter-v2` integration and the `weft-paseo-action` executable.
Older bridge entries without that field remain parseable, but `weft doctor`
reports the missing adapter until setup refreshes the bridge.

Provide `weft-paseo-action` as a thin Unix shell adapter. Each action maps to
one current `weft.cli.v1` command and receives the state directory, Change ID,
actor, timestamp, repository identity, workspace identity, operation IDs, and
expected versions explicitly. It does not generate durable identities, launch
or supervise processes, retry ambiguous provider mutations, or alter user-home
configuration.

The supported lifecycle is assignment/lease acquisition, exact Native Git
checkpoint, exact-revision materialization, observation, handoff, session
replacement with a new lease/workspace, and explicit release. The repository
gate runs the lifecycle proof and a separate-process resume proof. Unix-like
release archives ship the adapter; Windows archives retain the bridge and CLI
contract but do not claim to ship a Bash adapter.

## Consequences

- Paseo can call a stable action boundary without duplicating Weft's durable
  state machine or using stale command syntax.
- A replacement session can reclaim an expired scope, materialize the exact
  recorded revision in a new workspace, and release all durable ownership.
- Callers still supply IDs and expected versions, so malformed or stale actions
  fail as explicit usage or concurrency errors.
- OMC, OMG, OMX, Codex, Claude Code, and Gemini CLI remain bridge/instruction
  integrations rather than unproven native lifecycle adapters.
- A PowerShell adapter remains future work if Windows Paseo lifecycle parity is
  required.

## Alternatives rejected

- Embedding a process scheduler or agent runner in Weft: violates the external
  scheduler boundary.
- Keeping the obsolete helper grammar as a compatibility alias: hides caller
  drift and would make the public v1 command contract ambiguous.
- Inventing IDs or silently recovering stale versions in the adapter: breaks
  durable identity and compare-and-swap guarantees.

## Required proof

- Current CLI and bridge-schema tests prove setup metadata, idempotency, and
  legacy bridge diagnosis.
- `scripts/test-paseo-weft-bridge.sh` proves two isolated workspaces,
  acquisition, renewal, checkpoint, dirty observation, handoff, expiry/reclaim,
  exact replacement materialization, and release.
- `scripts/test-cli-session-resume.sh` proves the checkpoint survives a separate
  process.
- `make check` and the Linux release archive smoke test must remain green.

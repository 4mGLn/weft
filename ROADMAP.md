# Weft Implementation Roadmap

This roadmap implements the product in [GOAL.md](GOAL.md) and the normative model in [DOMAIN.md](DOMAIN.md). Phase boundaries may move after the provider spike, but domain guarantees must not be weakened silently.

## Phase 0 — Git and GitButler feasibility spike

Before freezing storage technology, language, or provider interfaces, build a minimal executable prototype and technical report that validates:

1. Mapping a Change and canonical ChangeRevision to Native Git.
2. Mapping a Change to a GitButler virtual or parallel branch.
3. Mapping an ordered Weft Stack to GitButler stacked branches.
4. Materializing multiple independent Changes in a GitButler workspace.
5. Preserving canonical content and Weft identity across branch, commit, rebase, and provider rewrites.
6. Creating an immutable CompositionCandidate from exact stacked revisions.
7. Capturing Git and GitButler failures as IntegrationConflicts.
8. Planning and verifying an IntegrationAttempt against an expected target.
9. Reconnecting provider references and reconciling external provider changes.
10. Identifying unsupported or irreducibly provider-specific semantics that require Weft-owned state.

Deliver a short report, the prototype, a provider capability matrix, and explicit recommendations for the initial implementation language, storage, canonical artifact representation, and adapter boundaries.

## Phase 1 — Persistence and domain kernel

Implement durable identifiers and storage for Change, linear ChangeRevision creation with head compare-and-swap, canonical artifacts, Materialization, Assignment, Lease, relationships, Stack, CompositionCandidate, review and validation targets, IntegrationAttempt/receipt, operations, and audit events.

Implement atomic transitions, optimistic concurrency, operation idempotency, lease expiry, and crash-recovery primitives. Prove core invariants with storage-level and domain tests before provider mutation is enabled.

## Phase 2 — Native Git provider

Implement repository discovery and identity, exact revision inspection, canonical content capture and reconstruction, worktree materialization, diff and overlap detection, candidate composition, integration planning/execution, conflict capture, target compare-and-swap, receipts, and reconciliation of external Git changes.

Validate the first end-to-end local workflow through the reusable API.

## Phase 3 — GitButler provider

Implement the supported capability subset established by Phase 0: parallel/virtual branch mapping, stack mapping, materialization, provider references, canonical artifact export, conflict mapping, integration receipts, revision behavior, and external-state reconciliation.

Unsupported capabilities must be discoverable and return explicit errors rather than approximating different semantics.

## Phase 4 — CLI

Expose Change and revision lifecycle, assignment, handoff, leases, dependencies, stacks, candidates, materializations, review, validation, conflicts, integration, history, and reconciliation.

Provide stable JSON schemas, documented exit codes, noninteractive behavior, confirmation flags, expected-version arguments, and durable operation IDs. Exercise equivalent Native Git workflows through both human-readable and JSON modes.

## Phase 5 — Agent protocol

Publish provider-neutral operations for discovery, acquisition, inspection, materialization, revision creation, progress, handoff, review, validation, and release. Specify errors for stale heads, lost leases, unsupported provider capabilities, stale candidates, and uncertain operations.

Demonstrate session termination followed by safe resume from another runtime using canonical revision content rather than dirty workspace state.

## Phase 6 — Paseo integration

Connect Paseo sessions and workspaces to Weft Changes, revisions, assignments, leases, materializations, candidates, and requested actions. Paseo launches agents and reacts to readiness or blocking; Weft remains independently operable through its API and CLI.

## Phase 7 — Multi-agent workflows

Add richer orchestration integrations for dependency-aware execution requests, reviewer and resolver assignment, validation pipelines, composition planning, and integration ordering. Keep agent process scheduling and supervision outside Weft.

## Phase acceptance

Every phase must preserve immutable identity and history, use exact revision or candidate targets, avoid implicit dependency or provider retargeting, and leave uncertain mutations recoverable through reconciliation.

Provider-facing phases include failure and crash-injection tests. Public CLI/API phases include compatibility tests for JSON shapes and exit codes. A phase is complete only when its documented workflow can be reproduced from durable state after process restart.

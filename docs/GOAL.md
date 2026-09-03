# Weft — Agentic Change Coordination

## Product thesis

Weft is an independent, local-first tool for coordinating software changes created by humans and AI agents.

> A software change should be a durable, transferable object with explicit provenance, ownership, dependencies, review state, and integration history—not merely a mutable diff or a Git branch.

Weft provides the change-management layer between humans and agents, orchestration systems, local workspaces, Git providers, review, validation, and integration workflows. It remains independent of any particular agent runtime, orchestrator, Git client, or workspace implementation.

The normative domain model is defined in [DOMAIN.md](DOMAIN.md). Delivery sequencing and provider research are defined in [ROADMAP.md](ROADMAP.md).

## Problem

Git exposes commits, branches, worktrees, and merges. Agent runtimes expose sessions and workspaces. Neither directly preserves:

- the durable identity of a unit of work;
- the exact revision or composition being reviewed;
- its reproducible base and content;
- ownership and handoff history;
- dependencies and stack order;
- materializations across workspaces and providers;
- conflicts, validations, and integration outcomes across sessions.

Weft makes the logical **Change** the primary unit. A Change survives agent sessions, workspace changes, rebases, provider rewrites, review cycles, and handoffs. Its immutable revisions and integration history explain what happened and allow the work to be reconstructed.

## Product boundaries

Weft coordinates work and records requested actions. External systems run and supervise agents.

Weft supports provider-specific capabilities through adapters while keeping its public domain provider-neutral. Native Git and GitButler are initial providers; neither is required by the domain model. Paseo is an important orchestrator integration, but Weft remains usable without it.

Weft is not:

- a replacement for Git;
- an AI agent runtime or generic scheduler;
- a replacement for Paseo;
- a GitButler clone or mandatory GitButler wrapper;
- an issue tracker or general project-management system;
- a CI/CD platform;
- an authorization bypass around repository or provider policy.

## Design principles

### Change over branch

The Change is the durable domain object. Branches, commits, patches, and virtual branches are representations.

### Immutable provenance

Every reviewable state has an immutable revision, an exact base, and provider-independent canonical content. Dirty filesystems and provider references are never the sole durable source of truth.

### Exact composition

Dependencies and stacks resolve to immutable candidates containing exact Change revisions. Reviews, validations, and integrations never operate on an ambiguous moving set of inputs.

### History over mutable ownership

Assignment, handoff, review, resolution, and integration are durable events. Agent sessions are ephemeral; Change history is not.

### Provider and agent neutrality

Git, GitButler, Claude, Codex, Gemini, Paseo, and other systems are providers or clients—not the domain model.

### External orchestration

Weft exposes readiness, assignment, leases, requested actions, and outcomes. An external orchestrator, CI system, shell, or human performs the work.

### Local-first, scriptable, and recoverable

The initial product works locally without a hosted service. Core behavior is available through a reusable API and a noninteractive CLI with stable JSON. Crashes, retries, expired leases, concurrent writers, and provider divergence are expected conditions.

### Observable and auditable

Users can determine who is working, which exact revisions exist, where they are materialized, what depends on what, what is blocked, which checks passed, and what was integrated. Important transitions remain reconstructable from durable history.

## Success criteria

Weft succeeds when a developer can:

1. Point Weft at an existing repository and create a durable Change.
2. Create immutable, linearly ordered Change revisions using compare-and-swap against the current head.
3. Reconstruct every revision from an exact base and canonical provider-independent content after the originating session or provider state disappears.
4. Materialize a revision in supported workspaces and hand the Change between humans or agents without losing identity or history.
5. Maintain concurrent assignments, exclusive operation leases, and safe atomic transitions across multiple local writers.
6. Represent task decomposition, revision ancestry, dependencies, stack order, and contextual relationships separately.
7. Pin dependency inputs and resolve a stack or dependency set into an immutable CompositionCandidate.
8. Detect stale downstream work when an upstream revision changes, without silently reinterpreting dependencies.
9. Bind reviews and validations to exact revisions or candidates and make results stale when their target changes.
10. Distinguish overlap signals, provider integration conflicts, validation failures, and undetectable semantic incompatibility.
11. Plan and execute an IntegrationAttempt against an expected target state, record uncertain operations for reconciliation, and issue an immutable receipt only for success.
12. Recover from crashed processes, expired leases, repeated operation IDs, and provider state changed outside Weft.
13. Use Native Git and GitButler where supported without allowing either provider to define Change identity.
14. Exercise the workflow through the local API and CLI, including stable JSON output and documented exit behavior.
15. Connect Paseo or another orchestrator without making that orchestrator a prerequisite.

## Product statement

> Weft gives humans and AI agents a durable, provider-neutral model for creating, revising, transferring, composing, reviewing, validating, resolving, and integrating concurrent software Changes across Git providers.

Agents may come and go. Workspaces may change. Providers may rewrite their representations. The Change remains understandable, transferable, reproducible, and recoverable.

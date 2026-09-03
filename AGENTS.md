# Weft Agent Context

This is the canonical project context for coding agents. Keep it concise and link to detailed sources instead of duplicating them.

## Read first

| Work | Source of truth |
| --- | --- |
| Product thesis, boundaries, success | `docs/GOAL.md` |
| Entities, invariants, lifecycle | `docs/DOMAIN.md` |
| Phases and provider spike | `docs/ROADMAP.md` |
| Working protocol | `.agents/agent-harness/README.md` |
| Agent process protocol | `.agents/AGENT_PROTOCOL.md` |
| Paseo integration | `.agents/PASEO.md` |
| Multi-agent workflows | `.agents/MULTI_AGENT_WORKFLOWS.md` |
| Task/checkpoint format | `.agents/agent-harness/task-template.md` |
| Required proof by change class | `.agents/agent-harness/verification-matrix.md` |
| Durable architecture decisions | `.agents/decisions/` |
| Current project evidence | `.agents/PROGRESS.md` |
| Current decision ledger | `.agents/DECISIONS.md` |
| Development commands | `docs/DEVELOPMENT.md` |
| Release/deployment policy | `docs/DEPLOYMENT.md` |

## Non-negotiable domain invariants

1. A Change has immutable identity and exactly one linear revision head in v1.
2. Revision creation uses compare-and-swap against the expected head.
3. Every revision has an exact base and durable provider-independent canonical content.
4. Provider references supplement domain identity and content; they never replace them.
5. Reviews and validations target an exact revision or immutable CompositionCandidate.
6. Dependencies pin exact revisions; stacks resolve to immutable candidates before review or integration.
7. Integration uses an expected target, durable operation ID, auditable attempt, and verified receipt.
8. Uncertain provider mutations require reconciliation and are never reported as successful.
9. Assignments persist; exclusive operations use expiring recoverable leases.
10. Weft coordinates agents but does not become their process scheduler.

## Working protocol

1. Convert the request into measurable outcomes, scope, risks, and proof. Use a task record for material work.
2. Read the smallest relevant product, domain, decision, code, and test surface before editing.
3. Keep one behavior-focused change per patch and preserve unrelated user work.
4. Add the smallest proof first, then run the boundary checks required by the verification matrix.
5. Record decisions that change public contracts, invariants, provider semantics, persistence, recovery, security, or deployment.
6. Update docs and examples with the behavior they define; one rule has one canonical owner.
7. Report commands, results, residual risk, unavailable environments, and unverified claims explicitly.

Never weaken, delete, skip, or exclude a failing check merely to obtain green output. Historical reports and plans are context, not proof of current behavior.

## Specialist routing

Use the project reviewers only for bounded read-only review:

- `domain_reviewer`: Change/revision/candidate, lifecycle, concurrency, and integration invariants.
- `provider_reviewer`: Native Git/GitButler mappings, capability differences, reconciliation, and crash behavior.
- `release_reviewer`: artifacts, CI, publication, installation, upgrade, rollback, signing, and provenance.

The primary owner integrates findings and verifies the final state. Reviewers do not edit files or perform provider mutations.

## Safety boundary

Do not publish releases, deploy services, sign artifacts, change credentials, mutate production repositories, or perform destructive migrations unless the user explicitly requests that side effect. Never log secrets or place tokens, credentials, private keys, customer repositories, or captured source artifacts in committed fixtures.

The current repository is specification-first. Do not introduce a programming language, storage engine, container topology, or hosted control plane before the Phase 0 evidence and decision record justify it.

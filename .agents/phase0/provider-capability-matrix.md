# Phase 0 Provider Capability Matrix

Evidence date: 2026-08-26. Tested locally with Git 2.43.0 and GitButler CLI 0.22.0. “Supported” means the repository spike reproduced the behavior; it is not a compatibility promise for other versions.

| Weft capability | Native Git evidence | GitButler evidence | Weft-owned requirement |
| --- | --- | --- | --- |
| Stable Change identity | Git object/ref identity changes on rewrite; no logical identity exists | `changeId` survives `but amend`, including a dependent stack entry | Allocate and persist `change_id`; map provider references as replaceable observations |
| Canonical revision content | `git diff --binary` plus exact base reconstructed the exact tree after branch deletion | Virtual branch/commit state is addressable but was not proven durable after provider removal | Store canonical content and digest outside either provider’s mutable workspace metadata |
| Parallel materializations | Separate branches/worktrees are possible; the spike used disposable clones | Two virtual branches coexist as separate stacks in one workspace | Persist materialization identity, provider capability, workspace, revision, and state |
| Ordered stack | Exact commits/patches compose in declared order | Anchored branches and `but move` create one ordered stack | Snapshot exact `{change_id, revision_id}` inputs into an immutable candidate |
| Provider rewrite | Rewritten commit creates a different object while old artifact remains valid | Commit ID changes while GitButler `changeId` remains stable | Append a ChangeRevision; never mutate the earlier revision or treat provider ID as domain ID |
| Conflict capture | Failed merge exposes unmerged paths | `but pull` stores the rebased commit with `conflicted: true` | Persist IntegrationConflict with exact attempt, inputs, provider evidence, and resolution revision |
| Changed target guard | Exact expected/actual target comparison prevents stale execution | External target advance is observed by `but pull` | IntegrationAttempt must compare expected target before provider mutation |
| External-state reconciliation | Recorded and observed refs differ after outside commit | `but pull` advances merge base to the external target and retains Change ID/conflict state | Reconciliation is an explicit operation/event; provider state never silently overwrites history |
| Integration result | Resulting Git target/tree can be verified | `but land --whole-stack` advances configured target | Success requires target verification and immutable receipt; uncertainty is not success |
| Machine-readable control | Git plumbing is scriptable but has command-specific formats | `but --json status` exposes stacks, change IDs, commits, target, and conflict flags | Adapters normalize capabilities/errors; public API never exposes raw provider JSON as domain state |

## Explicit limitations

- A Git binary patch is feasibility evidence, not the selected long-term artifact format.
- GitButler provider removal and recovery from a crash during `land` were not proven. Treat both as unsupported until implementation-level fault injection exists.
- GitButler CLI JSON is machine-readable, but schema compatibility across versions was not established. The adapter must version-gate and reject unknown shapes.
- Neither provider supplies Weft assignments, dependency contracts, immutable candidate identity, review/validation targeting, operation idempotency, or integration receipts.
- Native Git worktree concurrency and remote branch-protection behavior were outside this local spike.

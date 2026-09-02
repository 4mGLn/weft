# ADR-0011: Exact GitButler 0.22 local capability adapter

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Phase 0 reproduced stable GitButler Change IDs across amend, parallel virtual branches, explicit stacks, whole-stack landing, conflicted commits, and external-target reconciliation with `but 0.22.0`. It did not establish compatibility with another CLI/JSON version, canonical durability after provider removal, safe provider reconnect, remote mutation policy, or crash recovery for general `but land` behavior.

The domain already owns immutable Change/Revision identity, canonical content, candidates, integration intent, effect-operation IDs, uncertainty, and receipts. The adapter must expose only provider facts it can verify without turning GitButler JSON, CLI IDs, branch names, or mutable Change IDs into domain contracts.

## Decision

`weft-provider-gitbutler` supports exactly `but 0.22.0` and the complete status shape exercised by declared-version fixtures and a live disposable workflow. Discovery binds a caller-owned `RepositoryId` to the canonical Native Git common directory, verifies SHA-1, rechecks the configured target ref against GitButler's upstream observation, and returns explicit capabilities. Every later observation rechecks repository identity, locator, target configuration, CLI version, output bounds, and JSON shape. Unknown top-level fields, unsupported nested data, duplicate provider IDs, invalid object IDs, and malformed stack ancestry fail closed.

GitButler `changeId` is stored only as `ProviderRef`. Stack/branch/commit arrays are normalized from GitButler's tip-first representation to base-to-tip exact inputs. Every listed commit must have the exact preceding commit (or merge base) as first parent. Empty branch segments and non-empty published `upstreamCommits` cannot be represented by the evidenced v1 mapping and return explicit unsupported errors. Uncommitted workspace content yields Dirty materialization evidence; a conflicted commit yields Diverged evidence. Conflict evidence identifies the exact provider reference, commit, and branch but does not invent path data absent from status.

Canonical export re-observes the `changeId` and expected commit, rejects conflicts and rewrites, requires the supplied base to equal the commit's exact first parent, and delegates raw object capture to `weft-provider-git`. The resulting `tree-delta-v1` manifest/blobs remain durable independently of GitButler metadata. Provider removal is reported as a missing reference; reconnect remains unsupported rather than matching by branch name or commit similarity.

The only mutating capability is whole-stack landing to the repository-local `gb-local` remote. Planning requires a clean, conflict-free, fully represented stack, upstream target and merge base both equal to the expected target, and an exact first-parent chain whose tip is the planned result. The sealed plan copies repository/locator, target, complete inputs, top CLI selector, result commit/tree, and stable Weft effect-operation ID. Execution re-observes all inputs and uses noninteractive, deadline/output-bounded `but land --whole-stack --yes --json`. Command failure, timeout, or output overflow is mutation-ambiguous and immediately enters reconciliation. The exact planned tip and tree is `ResultVerified`; another target is `Diverged`; the unchanged expected target remains `StillUncertain` because a completed landing followed by an external reset cannot be disproven.

Commands run in a dedicated process group on Unix and terminate descendants at deadline/output overflow. Returned command errors disclose status and redacted byte counts, not provider output. Caller-supplied child environment cannot override noninteractive Git controls.

Adapter evidence uses an unambiguous length-prefixed opaque text format. It can contain raw provider references, branch names, repository locators, and object IDs, so it is durable provider evidence rather than a secret-redaction boundary; command output and stderr remain excluded. Callers must apply their normal repository-metadata access controls when persisting or displaying evidence.

## Alternatives

- Accept a minimum GitButler version while ignoring unknown JSON: rejected because Phase 0 established only one version/shape.
- Treat `changeId` as Weft Change identity: rejected because provider identity is replaceable and provider-owned.
- Infer provider reconnect from branch name, commit, or tree: rejected because each can collide or be rewritten and removal recovery is unproven.
- Import canonical artifacts by writing files and asking GitButler to commit: deferred because exact assignment, filters, dirty-state ownership, and crash behavior were not established.
- Support general or remote landing: deferred because merge-result identity, credentials, branch policy, atomic push, and recovery are unproven.
- Report an unchanged target after timeout as no effect: rejected because current state cannot disprove a landing followed by external reset.

## Consequences and limitations

This adapter is deliberately narrower than GitButler itself. It supports exact inspection, existing parallel materializations, stack mapping, canonical export, conflict mapping, external-state reconciliation, and repository-local fast-forward landing. Canonical import, assigned uncommitted-change details, empty/published segments, SHA-256 projects, provider reconnect, remote landing/push, code-review/CI payloads, and schema versions other than `0.22.0` are explicit unsupported results.

Landing receipts bind the exact preplanned result commit/tree and stable Weft effect-operation ID, but GitButler does not embed that operation ID into the commit. Therefore only equality with the exact sealed candidate tip is accepted; the expected target is never sufficient no-effect evidence after ambiguity.

## Required proof

- Exact-version discovery, strict unknown-shape rejection, repository/locator binding, target agreement, and explicit capability denial.
- Base-to-tip stack ordering, exact first-parent enforcement, provider-only Change references, clean/dirty/conflicted materialization evidence, and conflict normalization.
- Exact-base canonical export with artifact verification after status removal plus stale rewrite/removal denial.
- Rewrite, missing/new reference, target, and conflict reconciliation without inferred reconnect.
- Local landing plan/execute/receipt, changed-state denial, result-tree verification, before-mutation timeout uncertainty, after-mutation timeout recovery, and divergence.
- A live isolated-XDG `but 0.22.0` workflow covering stack export, local landing, external target advance, `but pull`, stable Change reference, and conflicted commit.
- Strict static checks, the full repository gate, and read-only provider review.

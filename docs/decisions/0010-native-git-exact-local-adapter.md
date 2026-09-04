# ADR-0010: Exact local Native Git adapter

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Phase 0 proved that Git commits and refs can expose exact trees, target movement, and conflicts, but cannot supply stable Weft Change identity, durable canonical content, workflow state, idempotency, or receipts. Phase 1 established provider-neutral artifacts and an integration kernel that requires an exact candidate base, target compare-and-swap, stable effect identity, conservative uncertainty, and verified result evidence.

The first provider adapter must retain those guarantees in repositories whose refs, commits, configuration, worktrees, filters, and helper processes can change outside Weft.

## Decision

`weft-provider-git` is the Native Git v1 adapter and supports local Git 2.38 or newer with SHA-1 or SHA-256 object formats. Discovery returns the canonical worktree/common-directory locator, observed version and object format, encoded provider-locator evidence, and an explicit capability set. A caller registers and persists the Weft-owned `RepositoryId`; every artifact, materialization, candidate, and integration call rechecks that identity instead of deriving it from a path or Git object.

Exact revisions are normalized to commit and tree object IDs. Capture uses literal-path, no-rename Git plumbing, reads raw blob objects, and writes sorted `tree-delta-v1` operations and blobs to the Weft CAS. Captured revisions and their provider observations have private fields and read-only accessors, so composition can accept only adapter-verified artifact/commit/tree bindings. UTF-8 repository-relative paths and regular, executable, and symbolic-link blobs are supported. No-op deltas, non-UTF-8 changed paths, gitlinks/submodules, unsupported modes, and unknown object formats return explicit unsupported errors.

Materialization starts from a detached worktree at the artifact's exact base commit. Canonical bytes are written to the filesystem and staged with raw `hash-object --no-filters`/`update-index --cacheinfo` plumbing, never `git add`, so configured clean filters cannot rewrite the canonical tree. The resulting index tree must equal the captured revision tree. Observation compares exact HEAD, index tree, raw worktree bytes/modes, and nonignored untracked paths. Ignored paths are intentionally outside canonical cleanliness in v1. Clean release is non-forcing; internal failed-operation cleanup may force-remove only its disposable worktree.

Candidate composition accepts ordered captured revisions, not bare artifact references. It binds one `RepositoryId`, requires every artifact base commit to equal the preceding exact revision commit, verifies each intermediate tree against the preceding durable observation, and therefore continues after intermediate provider commits are pruned. The returned composition has private fields and retains the ordered commit/tree/artifact bindings; callers can inspect but cannot forge or retarget it. It reports canonical changed and overlapping paths. Planning accepts only this sealed composition result, copies its exact bindings into the sealed plan, and requires its exact first base commit—not merely an equal tree—to equal the expected target commit.

The initial integration strategy is a local one-parent squash commit whose tree is the exact candidate tree and whose parent is the expected target. Its controlled message contains a hex-encoded stable Weft effect-operation ID. Only `refs/heads/*` targets are supported. The plan's fields are private and retain the Weft Repository identity plus canonical provider locator; execute and reconcile rebind both before use, so callers cannot retarget intent or replay it against another clone with coincidentally equal objects. Execution re-observes the target, creates the commit with signing disabled for this adapter operation, advances the ref with `update-ref <new> <expected>`, then verifies the current ref, exact tree, exactly one expected parent, and exact operation marker. A failed CAS is classified as changed-target only when re-observation differs; otherwise it remains a provider command failure.

Reconciliation uses the same frozen plan. The exact controlled result is `ResultVerified`; another observed commit is `Diverged`. Seeing the expected target after an ambiguous execution remains `StillUncertain`, because an external actor could have reset a successfully updated ref. `NoEffectVerified` requires durable execution-boundary evidence outside the current-ref observation and is not claimed by this adapter alone.

Planning persists discovery-derived provider-locator evidence and the sealed candidate tree in the durable IntegrationAttempt. After process loss, the adapter reconstructs the candidate from the attempt's exact ordered artifact references and provider-independent canonical content; source provider commits are not inputs and may already be pruned. Rehydration rediscovers the repository, exact-compares its canonical locator evidence, and rechecks target ref, expected commit, candidate tree, and effect-operation ID. A different clone is rejected before target mutation or reconciliation, even when it contains equal objects. Rehydration deliberately does not require the live target to equal the expected commit; only `reconcile_integration` classifies that live state.

All Git commands are noninteractive, literal-path, deadline-bound, output-bound, and redact output from returned errors. Stdin delivery runs concurrently inside the same deadline, so a helper that never reads a large canonical blob cannot block timeout enforcement. On Unix commands run in a dedicated process group so timeouts and output overflow terminate descendants that retain pipes. Raw worktree verification rejects every missing, symlinked, or non-directory ancestor before reading a leaf, preventing traversal through a substituted directory symlink. Provider strings are hex-encoded in evidence fields. Raw command formats and stderr never become public domain state.

## Alternatives

- Treat equal trees as equal candidate bases: rejected because reviews and target intent bind exact repository revisions.
- Use `git add` for staging: rejected because clean filters and attributes can change canonical bytes.
- Report expected target after timeout as no effect: rejected because ref history can advance and reset between observations.
- Use refs or commits as Weft repository/revision identity: rejected because provider objects and paths are replaceable observations.
- Make remote push and branch protection part of the local adapter: deferred until hosting capabilities, credentials, policies, and remote atomicity have separate evidence.

## Consequences and limitations

The first complete reusable workflow is local and squash-based. Remote publication, protected branches, signed integration commits, non-UTF-8 paths, submodules, sparse worktrees, partial clones, network filesystems, ignored-file cleanliness, and custom merge-driver containment are not supported claims. Missing or deleted targets remain uncertain rather than inventing an absent `TargetRevision`. Git object retention is required only for the first candidate base; all captured revision content and intermediate chain evidence remain durable in Weft.

The adapter creates provider commit/blob/tree objects before guarded ref mutation. Unreferenced objects after a failed or changed-target attempt are harmless provider garbage, not integration success. A timeout at or after mutation always enters domain reconciliation; execution is never blindly retried under a new effect-operation ID.

## Required proof

- SHA-1 and SHA-256 discovery, exact inspection, binary/executable/symlink/delete capture, and exact trees.
- Materialization and ordered composition after source refs and intermediate commits are pruned.
- Configured clean-filter resistance plus clean, tracked-dirty, untracked-dirty, and diverged observations.
- Exact base-commit rejection even when another commit has the same tree.
- Target CAS, changed target, unchanged-target command failure, exact receipt evidence, exact-result reconciliation, expected-target uncertainty, divergence, and forged merge-result rejection.
- Conflict-path normalization, sealed-plan repository/locator rebinding, ancestor-symlink rejection, blocked-stdin deadline enforcement, and deadline/output process-group termination with a surviving descendant fixture.
- Process-restart plan rehydration from canonical artifacts after source commits are pruned, plus execute/reconcile rejection against a different clone.
- Strict static checks, the full repository gate, and read-only provider review.

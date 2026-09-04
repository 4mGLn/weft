# Project Progress

## Current phase

Phase 8 — local-runtime upgrade/rollback checkpoint complete.

## Completed

- Defined product goal, normative domain model, and implementation roadmap.
- Resolved revision identity, canonical content, exact composition, review targeting, and integration semantics.
- Established the evidence-driven development, review, CI, and specification-release harness.
- Added a passing Native Git Phase 0 spike for canonical reconstruction, provider
  rewrite survival, candidate composition, target guarding, conflict capture, and
  external-ref reconciliation.
- Added a passing GitButler Phase 0 spike for virtual-branch identity across
  rewrites, parallel and stacked changes, local whole-stack landing, conflicted
  commits, and external-target reconciliation.
- Completed the Phase 0 capability matrix, technical report, and implementation
  foundation decision (Rust, SQLite metadata, filesystem CAS, capability adapters).
- Started Phase 1 with a compiling Rust workspace and sealed domain types for
  linear revision-head CAS and canonical `tree-delta-v1` manifests; all eight
  focused domain tests pass.
- Added the first SQLite metadata migration and transactional Change/Revision
  repository with WAL, deferred foreign keys, database-enforced linear ancestry,
  exact operation replay, immutable audit events, reopen reconstruction, and
  stale-head rejection across both independent connections and a child process.
- Resolved domain-review findings by matching replay against complete immutable
  revision intent and constraining every audit revision reference to the same Change.
- Added deterministic `tree-delta-v1` bytes, a verified no-replace filesystem CAS,
  and provider-independent tree reconstruction covering binary, executable, symlink,
  rename, and deletion semantics after originating workspace removal.
- Bound SQLite revision append/load to durable manifest/blob verification and exact
  artifact-base equality; missing or corrupt canonical content now fails closed.
- Added typed overlapping Assignment tenures with exact-version release and durable
  assign/release events.
- Added exclusive `(Change, operation)` Lease scopes with versioned acquire, renew,
  release, exact-boundary expiry, and predecessor-linked reclaim after process exit.
- Added schema v2 with safe concurrent v1 upgrade, immutable coordination history,
  constrained projections, and a global operation registry spanning Change,
  Revision, Assignment, and Lease mutations.
- Verified the integrated Phase 1 checkpoint with 40 active workspace tests, all
  four spawned-process helpers, documentation/harness checks, and warning-free
  strict Clippy.
- Added typed exact-revision Materializations with immutable Change/revision/
  workspace/provider identity and versioned clean, dirty, diverged, suspended,
  released, and invalidated observations.
- Added schema v3 with exact-revision foreign keys, one active placement per
  Change/workspace/provider, immutable provider-evidenced lifecycle events,
  global exact operation replay, and fail-closed event/projection reconstruction.
- Proved canonical-content enforcement, stale independent-writer denial, exact and
  conflicting retries, terminal release, restart reconstruction, projection drift
  rejection, and concurrent v2-to-v3 upgrade without coordination-history loss.
- Verified the Materialization checkpoint with 53 active workspace tests, all four
  spawned-process helpers, documentation/harness checks, strict Clippy, and a
  resolved read-only domain review.
- Added canonical symmetric task-decomposition/related-to records with versioned
  removal and no inferred direction, ordering, dependency, or transitive closure.
- Added directed Dependencies that pin exact downstream and upstream revisions,
  derive four-way head freshness without retargeting, and preserve explicit repin
  and terminal removal history through globally idempotent operations.
- Added schema v4 with exact revision ownership, active-edge uniqueness, recursive
  cycle guards, fail-closed lifecycle/audit reads, and concurrent v1/v2/v3 upgrade
  preservation.
- Proved a two-connection opposite-edge race commits exactly one acyclic edge,
  multi-hop cycle rejection, stale-writer denial, historical replay, restart,
  canonical-content loss, both contextual kinds, and projection/event drift.
- Verified the Relationship/Dependency checkpoint with 71 active workspace tests,
  all four spawned-process helpers, documentation/harness checks, strict Clippy,
  and a resolved read-only domain review.
- Added non-empty, duplicate-free ordered Stacks with explicit predecessors,
  `order_only`/`predecessor_dependencies` policies, exact-version definition CAS,
  finalized full-snapshot history, and exact historical operation replay.
- Added immutable CompositionCandidates that atomically resolve current exact heads,
  durable canonical content, one repository, every active Dependency, and optional
  Stack-predecessor requirements into `composition-candidate-v1` digests.
- Added schema v5 with immutable candidate inputs/requirements, exact historical
  Dependency/Stack provenance validation, derived input/dependency/Stack freshness,
  restart and drift failure, and populated concurrent v4 upgrade preservation.
- Verified the Stack/CompositionCandidate checkpoint with 83 active workspace
  tests, all four spawned-process helper classes, documentation/harness checks,
  warning-free strict Clippy, and a resolved read-only domain review.
- Added exact revision-or-candidate targets that copy and revalidate repository/
  context and canonical digest, including evidence/source temporal ordering.
- Added immutable ReviewRequests with canonical finalized reviewer sets and explicit
  `new_submission_required` policy plus append-only requested-reviewer submissions.
- Added immutable ValidationResults with exact or declared-reusable scopes while
  keeping revision/candidate freshness factual and never auto-applying reuse.
- Added schema v6 with exact target constraints, requested-reviewer foreign keys,
  operation provenance, immutable evidence, restart/drift denial, and concurrent
  populated v5 migration preservation.
- Verified the Review/Validation checkpoint with 95 active workspace tests, all
  four spawned-process helper classes, documentation/harness checks, warning-free
  strict Clippy, and a clean read-only domain review.
- Added exact-candidate IntegrationAttempts with immutable repository/target/
  provider/strategy intent and stable provider-effect IDs distinct from metadata
  transition operations.
- Added target-scoped execution authority, start-time candidate/gate revalidation,
  target compare-and-swap evidence, and exclusion that remains held throughout
  Running and Reconciling, including after lease expiry.
- Added mandatory uncertainty reconciliation with StillUncertain, Diverged,
  NoEffectVerified, and ResultVerified observations; expiry never implies failure
  or permits blind re-execution.
- Added terminal immutable IntegrationConflicts, separate exact validation-backed
  ConflictResolutions, and atomic verified IntegrationReceipts with complete copied
  attempt/candidate/target/provider/effect provenance.
- Added schema v7 append-only event reconstruction, immutable conflict/receipt/
  resolution rows, exact historical operation outcomes after later transitions,
  fail-closed drift checks, and concurrent v6 migration proof.
- Resolved domain-review findings by retaining unresolved target exclusion through
  reconciliation, revalidating freshness at start, and replaying each operation at
  its recorded event version rather than the current attempt head.
- Added explicit receipt-free Superseded closure after exact Diverged evidence so
  the old target remains blocked until a new candidate can safely target the
  observed external revision.
- Enforced terminal evidence cardinality on every authoritative attempt read and
  linked conflict/receipt inserts to the same attempt's terminal event, rejecting
  raw orphan evidence for nonmatching states.
- Verified the Integration/Recovery checkpoint with 106 active workspace tests,
  spawned-process proofs, documentation/harness checks, warning-free strict Clippy,
  and the full `make check` gate.
- Added version-gated Native Git discovery with SHA-1/SHA-256 object support,
  canonical common-directory locator evidence, explicit capabilities, literal path
  handling, noninteractive execution, redacted bounded output, and deadline-driven
  Unix process-group termination.
- Added exact commit/tree capture into provider-independent `tree-delta-v1` content,
  including binary, executable, symbolic-link, addition, modification, and deletion
  semantics with explicit unsupported behavior for unrepresentable inputs.
- Added filter-independent detached materialization and exact raw worktree/index
  observation, including nonignored untracked state, clean non-forcing release, and
  proof against configured clean-filter rewriting.
- Added exact-commit-chained candidate composition with overlap evidence and durable
  intermediate tree observations, proven after source refs and provider commits are
  deleted and pruned.
- Added exact candidate-base integration planning, local target-ref compare-and-swap,
  one-parent squash results bound to stable effect IDs, exact result verification,
  conflict-path normalization, and conservative no-effect/result/divergence recovery.
- Resolved provider-review findings covering same-tree/different-commit bases,
  ref advance/reset ambiguity, untracked files, clean filters, forged merge results,
  descendant timeout, unchanged-target ref failures, and repository identity binding.
- Verified the Phase 2 checkpoint with 112 active workspace tests, four ignored
  process helpers, Git 2.43.0 SHA-1/SHA-256 fixtures, documentation/harness checks,
  warning-free strict Clippy, and the full `make check` gate.
- Added an exact `but 0.22.0` adapter with strict complete-shape JSON decoding,
  canonical repository/target rebinding, explicit capabilities, bounded noninteractive
  subprocesses, global provider-selector uniqueness, and rejection of unknown or
  unproven nested states.
- Normalized GitButler `changeId` values only as replaceable provider references,
  mapped exact base-to-tip first-parent stacks, reported clean/dirty/conflicted
  materializations, exported exact-first-parent canonical artifacts, and reconciled
  rewrites, missing/new references, target changes, and conflicts without inferred
  reconnect.
- Added clean conflict-free repository-local `gb-local` fast-forward landing with
  sealed exact inputs/result/effect intent, result commit/tree verification, and
  mandatory reconciliation after every post-preflight command outcome, including
  injected post-spawn failure, timeout-before/after-mutation, external reset, and
  divergence boundaries.
- Resolved provider-review findings covering duplicate cross-stack landing selectors,
  evidence-format disclosure, negative schema/ancestry cases, descendant/output
  bounds, and post-spawn I/O ambiguity; the final read-only audit found no remaining
  actionable provider-invariant finding.
- Verified the Phase 3 checkpoint with 119 active workspace tests, five ignored
  provider/process helpers, live isolated-XDG `but 0.22.0` stack/export/landing and
  external-conflict reconciliation, documentation/harness checks, warning-free strict
  Clippy, `git diff --check`, and the full `make check` gate.
- Added the stable noninteractive `weft` CLI and `weft.cli.v1` JSON contract over
  Change/revision, coordination, relationships/dependencies, stacks/candidates,
  materializations, reviews/validations, integration, history, and reconciliation.
- Sealed Native Git integration plans to discovery-derived repository locator
  evidence and canonical candidate trees; execute/reconcile reject another clone
  and rebuild from durable artifacts after source provider commits are pruned.
- Resolved the Phase 4 provider review with no remaining actionable finding and
  verified CLI restart, operation replay, exit compatibility, confirmations,
  provider targeting, guarded mutation, and reconciliation across 10 CLI tests.
- Published the provider-neutral agent protocol, safe session-replacement rules,
  Paseo workspace/agent mapping, and dependency-aware multi-agent review,
  validation, conflict-resolution, and integration workflow boundaries.
- Adapted the strongest applicable practices from the three EZIS ecosystems:
  concise routed context, deterministic harness checks, bounded specialist review,
  supervised command entry points, least-privilege/concurrent CI, artifact-first
  release proof, and operational runbooks; excluded their product-specific hooks,
  branch rules, runners, promotion topology, and platform claims.
- Added reproducible Ubuntu 24.04 x86_64 archives, atomic local-prefix install,
  state-retaining uninstall, deterministic CycloneDX inventory, SHA-256 checks,
  clean-install/restart smoke proof, exact tag/version guards, and GitHub artifact
  provenance.
- Verified the integrated Phase 4–7 state with 129 active workspace tests, five
  ignored provider/process helpers, documentation/harness checks, warning-free
  strict Clippy, `git diff --check`, archive checksum/SBOM/install/restart/uninstall
  proof, and the full `make check` gate.
- Published the signed, GitHub-provenanced public `v0.1.0` Ubuntu 24.04 x86_64
  runtime archive with checksums and a CycloneDX SBOM.
- Added an archive-to-archive upgrade/rollback checkpoint from that public runtime
  to the unpublished `v0.1.1` candidate, including durable-state reads after direct
  rollback and complete pre-upgrade state-snapshot restoration.

## Next checkpoint

Retain the passing checkpoint for the next authorized `v0.1.1` runtime release;
before a schema migration release, define and prove its compatibility boundary.

## Known gaps

- GitButler provider removal/reconnect remains explicitly unsupported; general and
  remote crash-uncertain landing remain outside the exact local fast-forward subset.
- Runtime distribution is limited to a local Ubuntu 24.04 x86_64 archive. There is
  no hosted service, container, package-manager channel, auto-update, artifact
  signing, vulnerability scan, remote-provider deployment, or non-Linux proof.
- Direct rollback is proven only for the shared schema-7 `v0.1.0`/`v0.1.1`
  boundary; migration rollback still requires an explicit compatible-schema claim
  or complete state-snapshot restoration proof.
- No software license selected.

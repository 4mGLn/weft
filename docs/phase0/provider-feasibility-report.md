# Phase 0 Provider Feasibility Report

## Outcome

The Phase 0 spikes show that the Weft domain can be implemented across Native Git and GitButler without making either provider the domain model. Both can materialize and combine exact work, expose target divergence, and surface conflicts. Neither provides all durable identities, canonical artifacts, workflow relationships, idempotency, or integration receipts that Weft requires.

Reproduce the evidence with:

```bash
make phase0-spike
```

See the [capability matrix](provider-capability-matrix.md) for the evidence boundary.

## Native Git findings

- An exact base plus `git diff --binary` reconstructed the expected tree, including after deleting the originating branch reference.
- Provider rewrites create different commit objects and do not have a provider-level logical Change identity.
- Ordered revision artifacts can reconstruct a candidate deterministically.
- Expected-target comparison is available before integration; merge failures expose unmerged paths.
- External ref movement is detectable by comparing recorded and observed object IDs.

The official Git documentation describes `--binary` as producing binary diffs usable by `git apply`: [git-diff](https://git-scm.com/docs/git-diff.html) and [git-apply](https://git-scm.com/docs/git-apply).

## GitButler findings

- GitButler Change IDs survive commit rewrites and preserve the identity of dependent stack entries in the tested amend flow.
- Parallel virtual branches coexist in one workspace and can be explicitly stacked.
- Whole-stack landing advances the configured target.
- When an external target update overlaps an active Change, `but pull` advances the observed base, retains the Change ID, and records the rebased commit as conflicted.
- The CLI exposes parseable JSON for scripting, but Weft must normalize and version-gate it rather than publish it as a domain contract.

These results match GitButler’s official descriptions of [scriptable JSON output](https://docs.gitbutler.com/cli-guides/cli-tutorial/scripting) and [first-class conflicted commits](https://docs.gitbutler.com/cli-guides/cli-tutorial/conflict-resolution).

## Implementation recommendation

Adopt a Rust workspace for the local domain library and CLI, SQLite for transactional metadata, and a filesystem content-addressed store for canonical artifacts. Use SQLite WAL only for same-host local processes, with short write transactions, explicit busy handling, and bounded checkpoint policy. SQLite documents concurrent readers with a single writer and the same-host constraint in its [WAL documentation](https://www.sqlite.org/wal.html).

Start both providers as capability-based adapters:

- Native Git adapter: invoke version-gated Git plumbing with structured normalization and exact object verification.
- GitButler adapter: invoke a pinned/minimum-compatible `but` CLI through JSON output, validate schemas, and map stable Change IDs only as provider references.
- Domain layer: own Weft IDs, linear revision CAS, canonical artifact manifests, candidates, assignments/leases, conflicts, operations, and receipts.

The canonical artifact should be a Weft `tree-delta-v1` manifest: exact base identity, sorted canonical repository-relative path operations, file modes, lowercase SHA-256 content digests, and blobs in the content-addressed store. This preserves binary files, executable bits, symlinks, additions, modifications, and deletions without depending on a live provider object. Git binary patches remain useful interchange/test fixtures, not the storage contract.

## Risks and gates

- Rust/SQLite library selection and minimum supported versions are Phase 1 dependency decisions, not implied by this report.
- CLI adapters inherit installed-provider behavior. Capability discovery, timeouts, cancellation, output limits, version gates, and redaction are mandatory.
- SQLite WAL is not supported on network filesystems for Weft state; reject or fall back explicitly rather than silently weakening locking.
- Crash injection is required around provider mutation and receipt recording before integration can be called reliable.
- GitButler schema/version compatibility, removal/reconnect behavior, and uncertain `land` recovery remain implementation gates.

# Task Record: public-runtime upgrade and rollback checkpoint

## Outcome and scope

- **User/operator result:** Verify a `v0.1.0` installation can upgrade to the next unpublished runtime candidate and roll back without losing durable state.
- **In scope:** Ubuntu x86_64 release archives, checksums, SBOMs, atomic prefix replacement, state snapshot recovery, CI proof, and release documentation.
- **Out of scope:** Publishing `v0.1.1`, a schema migration, other operating systems, package managers, auto-update, and hosted services.
- **Affected domain invariants:** None; the checkpoint reads and writes normal durable Change history.
- **Provider/runtime scope:** Local CLI only; no provider mutation.
- **Compatibility surface:** artifact | storage | release.

## Acceptance criteria

1. The test verifies checksums and SBOMs for the public `v0.1.0` archive and current candidate archive.
2. A state directory created by `v0.1.0` remains readable after candidate install and after binary rollback to `v0.1.0`.
3. A complete pre-upgrade state snapshot restores exactly, CI runs the proof against GitHub's public `v0.1.0` asset, and a tagged release checks its latest prior runtime.

## Risks

- **Data/security:** Only an isolated temporary prefix and state directory are mutated; release assets are checksum-verified first.
- **Concurrency/crash recovery:** The test models a stopped mutator and durable state snapshot, not a concurrent upgrade or interrupted migration.
- **Provider divergence/compatibility:** No provider repository or worktree is touched.
- **Performance/resource limits:** Two small archives are extracted in a temporary directory.
- **Upgrade/rollback:** Both tested runtimes use metadata schema 7; schema-migration rollback remains explicitly unproven.

## Evidence and plan

- **Relevant paths, symbols, decisions, and tests:** `scripts/test-upgrade-rollback.sh`, `scripts/package-release.sh`, `docs/DEPLOYMENT.md`, `.github/workflows/ci.yml`.
- **Reproduction or baseline:** Published `v0.1.0` GitHub release archive and its checksums/SBOM.
- **Official version-sensitive evidence:** GitHub Actions release assets from `4mGLn/weft` `v0.1.0`.
- **Required decision/documentation updates:** Deployment policy and changelog.

1. Build a distinct `v0.1.1` candidate archive — proof: package version guard and archive smoke test.
2. Install `v0.1.0`, upgrade, roll back, and restore snapshot — proof: archive-to-archive test.
3. Download the public prior release in CI — proof: successful repository gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `make test-upgrade-rollback` | Passed | Public `v0.1.0` to local `v0.1.1` candidate, direct rollback, and snapshot restore. |
| Static/harness | `make check` | Passed | 129 active workspace tests, formatting, Clippy, harness, and docs. |
| Package/deployment | `make package-release`, `make test-release` | Passed | `v0.1.1` candidate archive, checksum/SBOM, clean install, restart, and uninstall retention. |
| Public CI | repository gate | Passed | [run 33359105845](https://github.com/4mGLn/weft/actions/runs/33359105845) downloaded public `v0.1.0` and completed the candidate checkpoint. |

## Decision and follow-up

- **Decision and alternatives rejected:** Test a real public prior archive rather than a copied fixture; use direct rollback only while the schema remains unchanged and prove backup restoration separately.
- **Residual risks:** Migration rollback, concurrent callers, and cross-platform upgrades remain unproven.
- **Unavailable evidence:** No schema-increasing public runtime exists.
- **Follow-up, owner, resumption condition:** Before a schema migration release, define runtime schema compatibility and prove its migration/restore behavior.

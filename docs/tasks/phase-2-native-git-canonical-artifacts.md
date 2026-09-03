# Task Record: Native Git canonical artifact capture and materialization

## Outcome and scope

- **User/operator result:** A local Git commit range can be captured into durable, provider-independent content and reconstructed in an isolated detached worktree at its recorded base.
- **In scope:** Repository discovery, exact commit resolution, `tree-delta-v1` capture from Git tree objects, CAS blob persistence, detached-worktree reconstruction, changed-path overlap inspection, and ordered artifact composition.
- **Out of scope:** Target CAS/integration, provider receipts, durable conflict records, reconciliation, public CLI, and Windows materialization.
- **Affected domain invariants:** Revision content remains exact-base-bound and locally durable; provider object IDs supplement rather than replace canonical artifacts; a materialization starts from an exact base.
- **Provider/runtime scope:** Native Git plumbing only. Capture is read-only; materialization explicitly creates a Git detached worktree and stages its reconstructed tree.
- **Compatibility surface:** API | artifact | provider | storage

## Acceptance criteria

1. Capture resolves both refs to exact commits, reads Git blob bytes and supported modes, and persists every upsert in the CAS.
2. Capture represents additions, modifications, type changes, and deletions as sorted `tree-delta-v1` operations; unsupported modes fail explicitly.
3. Materialization rejects a foreign or unresolvable base and produces a detached base worktree whose staged tree equals the captured target tree.
4. Composition reconstructs disjoint ordered artifacts and reports base-relative overlapping paths without inventing a merge result.

## Risks

- **Data/security:** Domain path validation remains the canonical traversal defense; the adapter rejects non-UTF-8 repository paths because v1 artifact paths are UTF-8. Blob bytes, including binary and symlink-target bytes, remain lossless.
- **Concurrency/crash recovery:** Capture writes idempotent CAS objects. A failure after `git worktree add` leaves the worktree for explicit inspection/reconciliation rather than claiming success.
- **Provider divergence/compatibility:** Git submodules and unexpected diff statuses return explicit errors. Git remains an external mutable provider; integration reconciliation is still pending.
- **Performance/resource limits:** Capture invokes Git once per upsert blob and currently has no output/time limits; provider process hardening remains a later Phase 2 requirement.
- **Upgrade/rollback:** The public pre-release API has no compatibility promise yet; it relies on the existing `tree-delta-v1` and CAS contracts.

## Evidence and plan

- **Relevant paths, symbols, decisions, and tests:** `crates/weft-native-git/src/lib.rs`, `crates/weft-domain/src/artifact.rs`, `crates/weft-domain/src/storage.rs`, `docs/decisions/0002-implementation-foundation.md`.
- **Reproduction or baseline:** Phase 0 established the Git plumbing feasibility; Phase 1 established validated artifacts and durable CAS.
- **Official version-sensitive evidence:** Local Git commands only; supported plumbing behavior is exercised against temporary repositories in the provider crate tests.
- **Required decision/documentation updates:** This task record and progress ledger record the completed slice; no new architecture decision is required.

1. Discover and resolve exact Git commits — proof: provider test resolves `HEAD` and detects external ref advance.
2. Capture canonical delta and CAS blobs — proof: provider test covers binary bytes, deletion, executable mode, and raw-byte symlink target.
3. Reconstruct detached worktree — proof: test stages materialization and compares its Git tree ID to the target commit tree.
4. Compose exact artifacts — proof: provider test verifies a combined tree and explicit ambiguous-path conflict.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-native-git` | Passed | Discovery, target guard, capture, CAS reopen, reconstruction, overlap, and composition tests. |
| Domain/contract | `cargo test --workspace --all-targets` | Passed | 37 domain tests plus 4 Native Git tests. |
| Concurrency/recovery | Existing restart test in `weft-domain` | Passed | Provider mutation/reconciliation recovery remains pending. |
| Provider integration | Native Git temporary repository tests | Passed | Materialized staged tree ID equals exact target tree ID; disjoint artifacts compose; ambiguous overlaps return explicit paths. |
| Static/harness | `make check` | Passed | Harness, documentation links, formatting, tests, and strict Clippy. |
| Package/deployment | Not run | Unavailable by scope | Packaging is a later phase. |

## Decision and follow-up

- **Decision and alternatives rejected:** Capture Git trees and blobs via plumbing, then store only canonical paths/modes/CAS digests. Rejected retaining binary patches or Git commit IDs as durable revision content.
- **Residual risks:** Process deadlines/output caps, non-UTF-8 paths, submodules, integration, durable conflict mapping, and reconciliation remain unimplemented.
- **Unavailable evidence:** Windows file-mode/symlink behavior and crash injection during real provider mutations.
- **Follow-up, owner, resumption condition:** Primary agent continues Phase 2 with guarded target updates, receipts, durable conflict records, and reconciliation; each provider mutation needs failure and crash evidence.

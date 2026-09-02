# Task Record: Durable assignments and materializations

## Outcome and scope

- **User/operator result:** Assignment responsibility survives restart as immutable history, and each materialized exact revision has a durable, guarded lifecycle record.
- **In scope:** Assignment events, materialization identity/provider metadata, clean/dirty/diverged/suspended/released/invalidated states, expected-state transitions, audit events, and SQLite schema v4.
- **Out of scope:** Provider-created worktrees, dirty-work capture, assignment revocation policy, and reconciliation against external provider state.
- **Affected domain invariants:** Assignments persist independently of leases. Materialization targets an exact immutable revision. State transitions are atomic and stale expectations fail.
- **Provider/runtime scope:** Provider-neutral metadata only; native-Git behavior starts in Phase 2.
- **Compatibility surface:** API | schema | storage

## Acceptance criteria

1. Assignment event history persists and reloads across restart.
2. Materialization records one exact revision and provider/workspace references.
3. Illegal and stale materialization transitions fail without overwriting state.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- |
| Focused | `cargo test -p weft-domain` | Passed | Assignment restart and legal/illegal/stale materialization transitions. |
| Domain/contract | `cargo test -p weft-domain` | Passed | 19 total storage/domain tests. |
| Static/harness | `cargo clippy --workspace --all-targets -- -D warnings` | Passed | Strict lint clean. |

## Decision and follow-up

- **Decision:** `Released` and `Invalidated` are terminal; dirty work cannot be silently marked clean because capture must create a new revision.
- **Residual risks:** Actor/time fields are not yet universal audit metadata; provider reconciliation and capture must enforce the dirty-work rule in Phase 2.
- **Follow-up:** Continue Phase 1 with exact review/validation targets, then operation/idempotency and integration attempt/receipt records.

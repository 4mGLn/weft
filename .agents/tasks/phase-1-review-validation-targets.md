# Task Record: Exact review and validation targets

## Outcome and scope

- **User/operator result:** Reviews and validations can only target immutable revisions or composition candidates, never a mutable Change.
- **In scope:** Durable review requests/submissions, validation results, exact target existence validation, and SQLite schema v5.
- **Out of scope:** Retrieval APIs, stale-result projection, reviewer policy, and provider execution.
- **Affected domain invariants:** Review and validation target an exact immutable revision or candidate.
- **Compatibility surface:** API | schema | storage

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- |
| Focused | `cargo test -p weft-domain` | Passed | Exact revision review/validation records survive database reopen; missing target is rejected. |
| Static | `cargo clippy --workspace --all-targets -- -D warnings` | Passed | Strict lint clean. |

## Follow-up

- Result/query APIs, candidate stale projection, and explicit review reuse policy remain for the next Phase 1 slice.

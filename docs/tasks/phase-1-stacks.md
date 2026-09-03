# Task Record: Durable stack versions

## Outcome and scope

- **User/operator result:** Stacks persist ordered, duplicate-free Change identities as immutable versions.
- **In scope:** Creation, optimistic version replacement, historical reads, restart persistence, SQLite schema v7.
- **Out of scope:** Candidate stack-version references and provider stack materialization.
- **Affected invariants:** Mutable stack order is never itself a review/integration input; historic stack versions remain immutable.

## Validation record

| Check | Command/test | Result |
| --- | --- | --- |
| Focused | `cargo test -p weft-domain` | Passed: ordering, immutable history, stale-version rejection, restart reload, duplicate entry rejection. |
| Static | `cargo clippy --workspace --all-targets -- -D warnings` | Passed. |

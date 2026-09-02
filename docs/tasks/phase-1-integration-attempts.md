# Task Record: Durable integration attempts and receipts

## Outcome and scope

- **User/operator result:** An integration attempt has an idempotent operation ID, immutable candidate input, expected target guard, required active lease, validated state transitions, and a required receipt for success.
- **In scope:** Provider-neutral planning/start/finish records and receipts; SQLite schema v6.
- **Out of scope:** Provider mutation, conflict evidence, reconciliation, and native-Git target inspection.
- **Affected invariants:** No implicit replanning on a changed target; only verified success creates a receipt; retry operation IDs do not create duplicate effects.

## Validation record

| Check | Command/test | Result |
| --- | --- | --- |
| Focused | `cargo test -p weft-domain` | Passed: stale target, missing lease, duplicate operation, receipt enforcement, and persisted success. |
| Static | `cargo clippy --workspace --all-targets -- -D warnings` | Passed. |

## Follow-up

- Provider execution/reconciliation and integration conflict capture are Phase 2 work. Stack support and remaining audit/query semantics remain in Phase 1.

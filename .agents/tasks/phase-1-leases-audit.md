# Task Record: Phase 1 leases and audit history

## Outcome and scope

- **User/operator result:** Exclusive operations have durable, expiring ownership, and correctness-sensitive Change mutations emit durable audit events.
- **In scope:** SQLite lease acquisition/recovery, audit-event persistence, and focused storage tests.
- **Out of scope:** Assignments, candidates, reviews, integrations, providers, CLI, and crash injection.
- **Affected domain invariants:** Recoverable exclusive leases and atomic, auditable Change transitions.

## Acceptance criteria

1. An active lease rejects a competing holder; an expired lease is recoverable.
2. Change creation, revision append, and lease acquisition write ordered audit events in the same transaction.
3. The repository gate passes.

## Validation record

| Check | Result | Evidence |
| --- | --- | --- |
| Lease recovery and audit history | Passed | storage lease/audit test |
| Static/harness | Passed | 2026-09-02: repository gate, 15 tests, strict Clippy |

## Residual risks

- Assignment history, candidate/integration operations, and crash-injection recovery are not implemented.

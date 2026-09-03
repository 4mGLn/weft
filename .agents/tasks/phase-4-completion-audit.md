# Phase 4 CLI Completion Audit

Date: 2026-09-03

## Delivered surface

`weft` provides noninteractive schema-v1 JSON commands for Change/revision CAS,
assignment and handoff history, acquire/renew/release leases, exact dependency
pins and relations, versioned stacks, immutable candidates, materializations,
reviews, validations, conflicts, integration planning/execution/reconciliation,
and Change audit history. The canonical grammar, JSON envelope, and exit codes
are documented in [CLI v1](../../docs/CLI.md).

Native Git is the supported CLI mutation provider. GitButler remains exposed
through the reusable adapter API and returns an explicit unsupported CLI result
until its equivalent durable CLI run/reconcile commands are implemented.

## Acceptance evidence

| Requirement | Evidence |
| --- | --- |
| Stable JSON and exits | `weft-cli` contract test proves schema version, persistence across invocations, and usage (`2`) versus domain (`3`) errors. |
| Exact revision lifecycle | Disposable Git fixture captured canonical content, advanced a head through expected-head CAS, and rejected stale head input. |
| Exact workflow targets | CLI probes verified dependency pins/cycles, stack version CAS, immutable candidates, materialization state CAS, reviews, and validations. |
| Native Git workflow | A durable leased attempt was planned, restarted, executed with explicit `--yes`, target-CAS verified, receipted, and reconciled without false success on divergence. |
| Observable recovery | `history` reads durable events; `conflict list` reads durable conflict records; reconciliation reports unresolved divergence explicitly. |

## Validation

```text
make check
cargo test --offline -p weft-gitbutler \
  tests::lands_a_virtual_branch_with_a_durable_receipt_after_restart \
  -- --ignored --exact
```

The external GitButler acceptance command remains opt-in because it requires a
locally installed supported CLI; its passing Phase 3 evidence is retained
separately.

# Phase 3 GitButler Provider Completion Audit

Date: 2026-09-03

## Scope

Phase 3 implements the capability subset established by the Phase 0 GitButler
spike. The adapter is intentionally version-gated to `but 0.22.*`; unsupported
versions fail discovery rather than silently interpreting a different JSON schema.

## Acceptance evidence

| Roadmap requirement | Evidence |
| --- | --- |
| Parallel/virtual branch mapping and provider references | `GitButlerBranch` records the observed name, stable GitButler `changeId`, exact provider commit, and conflict flag. The status projection test covers multiple branches and whitespace. |
| Stack mapping and workspace materialization | `create_stacked_branch` uses a named anchor; the GitButler workspace returned by `root()` is the provider materialization. Phase 0 reproduces parallel and ordered stack behavior. |
| Revision behavior and canonical content | Virtual-branch creation/amend re-observe provider references. `export_branch_artifact` captures the exact base-to-provider-commit delta through the content-addressed Native Git artifact boundary. |
| Conflict mapping | An observed conflicted virtual branch records a durable `IntegrationConflict`; a stale configured target likewise records a conflict before provider mutation. |
| Integration receipt | `execute_integration` starts the persisted attempt only against the exact observed configured target, lands one complete stack, re-observes target, and then stores a durable receipt. |
| External-state reconciliation | `reconcile_target` refreshes provider state. A running integration can be reconciled only to an explicit expected result; divergence remains unresolved and auditable. |
| Crash/command uncertainty | Any landing-command or post-landing-observation error after durable transition to `running` records unconfirmed reconciliation evidence and returns `Uncertain`; it never reports success. |

## Reproducible validation

The ordinary offline workspace gate passed:

```text
cargo test --offline --workspace --all-targets
cargo clippy --offline --workspace --all-targets -- -D warnings
python3 scripts/check_docs.py
```

This includes 37 domain tests, 6 Native Git provider tests, and GitButler status
projection coverage. The opt-in live-provider acceptance test also passed with
GitButler CLI 0.22.0:

```text
cargo test --offline -p weft-gitbutler \
  tests::lands_a_virtual_branch_with_a_durable_receipt_after_restart \
  -- --ignored --exact
```

It initializes a disposable workspace, exports virtual-branch content to the
filesystem CAS, creates a leased durable attempt, lands the whole stack, then
reopens SQLite and verifies `IntegrationState::Succeeded` from the receipt.

## Explicit unsupported boundary

Provider removal/reconnect and fault-injected process death during `but land`
are not claimed as successful provider operations. The adapter treats an
unverified landing outcome as recoverable uncertainty, and an unsupported CLI
version as an explicit error. Future CLI and protocol work must expose those
errors without converting them to success.

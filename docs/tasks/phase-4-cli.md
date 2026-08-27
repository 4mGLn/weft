# Task Record: Phase 4 CLI

## Outcome and scope

- **Target result:** A stable noninteractive `weft` process API over the complete local domain/provider lifecycle, with equivalent human/JSON behavior and durable retry/concurrency inputs.
- **In scope:** state initialization, Change/revision, assignment/handoff, leases, relationships/dependencies, stacks/candidates, materializations, reviews/validations, conflicts/integration/reconciliation/history, Native Git workflows, JSON v1, exit codes, confirmations, and restart proof.
- **Out of scope:** hosted APIs, remote Git/GitButler capabilities not supported by their adapters, credential management, interactive TUI/prompts, shell completion, daemon scheduling, and runtime packaging.
- **Affected invariants:** all public lifecycle, exact-target, optimistic-concurrency, operation-idempotency, provider uncertainty, and audit invariants.

## Acceptance criteria

1. The grammar, state layout, JSON envelope, exit codes, confirmation rule, and compatibility policy are documented and fixture tested.
2. All mutations require durable operation/actor/time inputs plus expected head/version where applicable; retries replay exact outcomes and conflicting reuse fails.
3. The Phase 4 lifecycle is accessible without raw provider JSON or internal Rust/storage representations becoming contracts.
4. Provider commands discover capabilities, reject unsupported behavior explicitly, guard exact targets, and preserve uncertain outcomes for reconciliation.
5. Human and JSON modes are behaviorally equivalent and noninteractive; JSON stdout contains exactly one object on both success and failure.
6. End-to-end tests reopen state between commands and cover invalid input, not found, stale writers, confirmation denial, provider conflict/uncertainty, and exit-code compatibility.
7. Documentation, strict static checks, full repository gate, and relevant read-only reviews pass.

## Proof record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| CLI compatibility fixtures | `cargo test -p weft-cli` | Passed | 10 tests: stable envelopes, parser, exits, process restart |
| Native Git CLI workflow | `native_git_discovery_inspection_and_capture_create_exact_durable_revision` | Passed | Exact capture, planning, different-clone denial, canonical rebuild, guarded integration |
| Provider recovery | `cargo test -p weft-provider-git` plus read-only provider review | Passed | Rehydration after source-commit pruning; both prior P1 findings resolved |
| Repository gate | `CARGO_NET_OFFLINE=true make check` | Passed | 129 active tests, five ignored helpers, docs/harness/fmt/strict Clippy green |

## Residual risk and unavailable evidence

- Remote provider mutation, credentials, signing, and hosted deployment remain outside the local CLI contract.

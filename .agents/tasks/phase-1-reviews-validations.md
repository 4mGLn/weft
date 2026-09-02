# Task Record: Phase 1 Exact-Target Reviews and Validations

## Outcome and scope

- **User/operator result:** Weft can durably request and submit human review and record automated validation against one exact revision or immutable candidate, then report target staleness without retargeting evidence.
- **In scope:** Exact target snapshots, ReviewRequest/reviewer sets, ReviewSubmission history/outcomes, v1 review reuse policy, ValidationResult/outcome/execution/scope, derived target freshness, immutable audit persistence, global operation replay, authoritative reads, schema v6 migration, restart/concurrency/drift proof.
- **Out of scope:** Change lifecycle aggregation, review quorum/readiness rules, authorization/identity federation, comments as external documents, executing validators, cross-target reuse application events, IntegrationAttempt policy, CLI/API schemas, notifications, hosted coordination.
- **Affected domain invariants:** `DOMAIN.md` sections 3, 4, 8, and 10; GOAL success criteria 9 and 12; ADR-0002, ADR-0003, and ADR-0007.
- **Provider/runtime scope:** Provider-neutral local SQLite WAL and filesystem CAS; no provider mutation or validator execution.
- **Compatibility surface:** Domain API, exact-target snapshot fields, immutable review/validation storage API, global operation idempotency, migration behavior.

## Acceptance criteria

1. An exact target is exactly one revision with Change ownership or one candidate, and copies repository/context plus canonical digest; invalid shape or mismatched source fails.
2. ReviewRequest identity/target/requester/time are immutable; reviewers are non-empty and duplicate-free; v1 records `new_submission_required` and never infers cross-target approval reuse.
3. ReviewSubmission identity/request/target/reviewer/outcome/comments/time are immutable, accepts only a requested reviewer, appends repeated reviewer history, and cannot retarget its request.
4. ValidationResult identity/target/type/environment/outcome/execution/validator/time/scope are immutable; identifiers and declared reusable scope/rationale are non-empty.
5. Revision freshness compares the exact target revision to its Change head; candidate freshness derives advanced inputs, changed Dependencies, and changed Stack without rewriting evidence.
6. A declared reusable validation scope never changes factual target freshness or silently applies a result to another target.
7. Exact operation retries return recorded immutable outcomes after later submissions or target advancement; conflicting cross-kind/payload reuse fails.
8. Authoritative reads revalidate revision/candidate source, canonical content, copied target fields, reviewer membership, operation provenance, and immutable row/child history; drift fails closed.
9. Fresh and v1–v5 databases reach schema v6 under serialized concurrent opens without losing existing histories; focused, domain-review, workspace, strict Clippy, harness, and documentation gates pass.

## Risks

- **Data/security:** Comments, environment, execution IDs, and scope text are bounded metadata and must not capture secrets or repository contents in fixtures/logs.
- **Concurrency/crash recovery:** Immediate transactions atomically register operations and immutable parent/child rows; no mutable review-result projection exists in this checkpoint.
- **Policy ambiguity:** Submission history is evidence, not a quorum or readiness decision. Reusable validation scope is a declaration, not automatic applicability.
- **Provider divergence:** Exact target sources are Weft-owned canonical state; live provider observations are not review/validation truth.
- **Upgrade/rollback:** v6 is additive but older binaries reject it. Rollback uses a pre-migration backup.

## Evidence and plan

- Relevant sources: `GOAL.md`, `DOMAIN.md`, ROADMAP Phase 1, ADR-0002/0003/0007, `crates/weft-domain`, `crates/weft-artifact`, and `crates/weft-storage-sqlite`.
- Required proof: verification-matrix review/validation row plus read-only domain review.

1. Freeze exact-target, review, validation-scope, and freshness types.
2. Add schema v6 immutable review/validation records and exact replay.
3. Prove exact ownership/content, staleness, reuse declaration, restart, migration, and drift behavior.
4. Run domain review, resolve findings, record ADR/evidence, and execute the full repository gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `cargo test -p weft-domain -p weft-storage-sqlite --target x86_64-unknown-linux-gnu --offline` | Passed | 35 domain and 49 active storage tests; 3 process helpers intentionally ignored at top level and exercised by parent tests |
| Domain/contract | Exact target, canonical reviewer set, review history, validation scope/freshness tests; read-only domain review | Passed | No actionable findings; requested canonicalization proof added in domain and operation-replay tests |
| Concurrency/recovery | Exact/conflicting replay, restart, immutable rows, source drift, concurrent populated v5 migration | Passed | Focused storage suite and full gate |
| Provider integration | Not applicable | No provider mutation or validator execution in scope |
| Static/harness | `CARGO_HOME=<isolated-cache> CARGO_NET_OFFLINE=true make check`; `git diff --check` | Passed | Harness/docs/fmt, 95 active workspace tests, spawned-process helpers, strict Clippy; clean diff whitespace |
| Package/deployment | Not applicable | No packaging behavior changed |

## Decision and follow-up

- **Decision and alternatives rejected:** ADR-0008 accepts copied exact targets, immutable append-only evidence, `new_submission_required`, and factual freshness separated from reusable-scope declarations; mutable targets, inferred approval transfer, automatic validation reuse, overwrite history, and persisted stale flags are rejected.
- **Residual risks:** Review aggregation/quorum, authorization, validator execution, explicit cross-target reuse application, lifecycle/readiness, integration consumption, and CLI remain open.
- **Unavailable evidence:** External review systems, CI providers, identity providers, distributed writers, provider target observation, and signed attestations are not claimed.
- **Follow-up:** Continue Phase 1 with IntegrationAttempt, conflict, reconciliation, and verified receipt persistence.

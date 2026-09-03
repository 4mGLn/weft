# Task Record: Phase 3 GitButler provider

## Outcome and scope

- **Target result:** A reusable, exact-evidence GitButler adapter for the supported Phase 0 subset without leaking provider identity or semantics into the Weft domain.
- **In scope:** exact `but 0.22.0`/JSON discovery, normalized provider Change/stack/materialization/conflict observations, canonical export, external-state reconciliation, exact repository-local fast-forward landing, receipts, bounded subprocesses, and crash ambiguity.
- **Out of scope:** other GitButler versions, SHA-256, canonical import, assigned uncommitted-change details, empty/published segments, provider reconnect, review/CI payloads, remote push/landing/policy, credentials, and the public CLI.
- **Affected invariants:** provider-independent identity/content, exact candidate inputs, expected-target guarding, mandatory uncertainty reconciliation, and verified receipts.

## Acceptance criteria

1. Discovery pins the evidenced CLI/schema, rebinds Weft repository identity and canonical locator, reports explicit capabilities, and rejects unknown shapes.
2. GitButler `changeId` remains a provider reference; exact base-to-tip stacks, rewrites, dirty/conflicted states, and missing/new references are normalized without retargeting domain identity.
3. Canonical export requires the currently observed Change/commit and exact first parent, persists `tree-delta-v1`, and survives later provider-status removal.
4. Only clean conflict-free repository-local fast-forward landing is planned/executed; exact target/input/result evidence is sealed and revalidated.
5. Mutation ambiguity never reports success: exact result is verified, another target diverges, and an unchanged target remains uncertain.
6. Timeouts/output are bounded, noninteractive, redacted, and terminate descendants on Unix.
7. Declared-version fixtures, live isolated-XDG proof, strict static checks, full repository gate, and provider review pass.

## Proof record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Declared-version fixtures | `cargo test -p weft-provider-gitbutler` | Passed | 7 active fixtures; 1 ignored live proof helper; strict schema/identity, canonical export, reconciliation, landing/crash and subprocess boundaries |
| Live GitButler workflow | `make phase3-gitbutler-live` | Passed | `but 0.22.0`; isolated XDG state; stack export/landing plus external target conflict reconciliation |
| Strict static checks | `cargo clippy -p weft-provider-gitbutler --all-targets -- -D warnings` | Passed | Warning-free adapter and fixtures |
| Repository gate | `CARGO_NET_OFFLINE=true make check` | Passed | 119 active workspace tests; 5 ignored helpers; formatting, docs, harness, strict Clippy, and `git diff --check` green |
| Provider review | `provider_reviewer` read-only audit | Findings resolved | Duplicate landing selectors and post-spawn error reconciliation corrected; final audit found no remaining actionable provider-invariant finding |

## Residual risk and unavailable evidence

- The support boundary is local Linux with Git 2.43.0 and `but 0.22.0`; other operating systems, GitButler builds, schemas, object formats, and provider database migrations are not claimed.
- GitButler does not embed Weft's effect-operation ID in the landed commit. Exact sealed tip/tree equality is required, and an unchanged target after ambiguity remains uncertain.
- Provider metadata removal is observed but reconnect is deliberately unsupported. Remote authentication, hosting policy, protected branches, atomic push, network failure, and signing are not claimed.

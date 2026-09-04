# Task Record: Phase 2 Native Git provider

## Outcome and scope

- **Target result:** A reusable Native Git adapter that completes Weft's first local exact-revision workflow from discovery and canonical capture through guarded integration and reconciliation.
- **In scope:** capability/version discovery, exact commit and tree observation, `tree-delta-v1` capture, detached worktree materialization, ordered candidate composition, changed-path overlap evidence, conflict normalization, target compare-and-swap, verified result evidence, bounded subprocesses, and recovery observations.
- **Out of scope:** remote hosting APIs and branch protection, credentials, network filesystems, signing, publication, GitButler behavior, and the public CLI.
- **Affected invariants:** provider-independent identity/content, exact candidate inputs, expected-target guarding, uncertainty reconciliation, and verified receipts.

## Acceptance criteria

1. Provider discovery rejects unsupported Git versions/object formats and reports explicit capabilities.
2. Exact commits can be captured into provider-independent canonical artifacts and reconstructed after source refs disappear.
3. Detached materializations and ordered candidates verify their resulting Git tree; overlaps and conflicts are normalized without exposing Git output as domain state.
4. Integration mutates a local target only through exact `update-ref` compare-and-swap and success includes a re-observed commit/tree result.
5. Reconciliation distinguishes no effect, the exact verified effect, divergence, and unresolved ambiguity using the same durable effect-operation ID.
6. Timeouts and output limits are bounded, noninteractive, redacted, and never reported as success.
7. Fixture-backed end-to-end, changed-target, conflict, rewrite/ref-removal, timeout, and crash-boundary tests pass with the full repository gate.

## Proof record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Native Git focused workflow | `cargo test -p weft-provider-git --target x86_64-unknown-linux-gnu --offline` | Passed | 6 fixture tests; Git 2.43.0; SHA-1 and SHA-256 |
| Timeout stability | focused deadline/output test, repeated five times | Passed | descendant process, deadline, and output overflow terminated fail-closed |
| Repository gate | `CARGO_NET_OFFLINE=true make check` with the isolated Cargo home | Passed | 112 active workspace tests; 4 ignored process helpers; strict Clippy, formatting, docs, and harness green |
| Provider review | `provider_reviewer` read-only audit | Findings resolved | Exact base, ambiguity, untracked state, filters, result parsing, descendant termination, ref failure, and repository binding corrected |

## Residual risk and unavailable evidence

- Remote push, hosting branch protection, credentials, signing, partial clones, sparse worktrees, network filesystems, non-UTF-8 paths, submodules, ignored-file cleanliness, and custom merge-driver containment are not claimed.
- Current-ref observation alone cannot prove no effect after an ambiguous mutation; this adapter deliberately returns StillUncertain until a higher execution boundary supplies exact no-effect evidence.
- The successful environment is local Linux `x86_64-unknown-linux-gnu` with Git 2.43.0. Other operating systems and the minimum Git 2.38 boundary were not exercised live.

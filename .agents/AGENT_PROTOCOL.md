# Agent Protocol v1

Weft's provider-neutral agent API is the noninteractive `weft` process interface with `--format json`. The stable envelope schema is `weft.cli.v1`; stdout contains exactly one JSON object and diagnostics remain inside that object. Agents must treat process exit status and the envelope together.

## Ownership boundary

An orchestrator launches and supervises processes. Weft owns Change identity, revisions, canonical artifacts, assignments, leases, exact candidates, reviews, validations, integration attempts, receipts, and audit history. An agent session, branch, worktree, or provider change ID is replaceable evidence and never durable Change identity.

Every invocation supplies an explicit state directory:

```bash
weft --format json --state-dir "$WEFT_STATE_DIR" <group> <command> ...
```

The caller owns stable IDs. Mutations also carry `--operation-id`, `--actor`, and `--at`; head/version-changing commands carry the observed expected head/version. Reusing an operation ID with identical intent replays its result. Reusing it with different intent fails.

## Operation map

| Agent intent | Weft operation | Required durable checkpoint |
| --- | --- | --- |
| Discover provider | `native-git discover`, `gitbutler discover` | capability and locator evidence |
| Acquire work | `assignment create`, `lease acquire` | assignment/lease ID and version |
| Inspect exact work | `change show`, `change history`, `candidate show` | Change/revision/candidate ID |
| Materialize | `native-git materialize` | Materialization ID and exact revision |
| Publish progress | `revision append` after provider capture | expected Change head and artifact digest |
| Handoff | create successor Assignment, then release prior Assignment | both immutable tenures |
| Request/submit review | `review request`, `review submit` | exact revision/candidate target |
| Record validation | `validation record` | exact target and evidence time |
| Compose | `stack create/replace`, `candidate create` | exact ordered revision inputs |
| Integrate | `integration plan`, provider execute command | expected target and stable effect ID |
| Record uncertain effect | `integration uncertain` | exact Running attempt/version, prior execution-lease identity, and provider observation; the lease need not remain live |
| Re-observe uncertainty | provider `reconcile-integration` | same IntegrationAttempt, effect ID, and new reconciliation ID |
| Close verified recovery | `integration finish`, `integration succeed`, or `integration supersede` | exact reconciliation outcome required by the transition |
| Release workspace | provider release command, lease/assignment release | observed version and `--yes` |

Use `weft --help` for the complete grammar. Provider-specific commands translate provider observations into these provider-neutral records; they do not change the identity model.

## Error policy

| Exit | Class | Agent response |
| --- | --- | --- |
| 0 | success | consume the returned durable identifiers/version |
| 1 | local | repair local state/path/permissions; do not claim progress |
| 2 | usage | correct the invocation; do not retry unchanged |
| 3 | not found | rediscover durable state or stop |
| 4 | conflict | reload current head/version/target and re-plan |
| 5 | unsupported | select a proven capability or report blocked |
| 6 | provider | inspect provider state; mutation outcome may require reconciliation |
| 7 | integrity | stop and preserve evidence; never bypass verification |

The envelope's `retryable` field is advisory within the class. A timeout or ambiguous provider mutation is never ordinary retry permission. Record the Running attempt as uncertain, then reconcile repeatedly while the result remains `StillUncertain`. `NoEffectVerified` permits `integration finish` to Failed or Aborted without a receipt. `ResultVerified` permits `integration succeed` only with matching result evidence and an immutable receipt. Exact `Diverged` permits `integration supersede` without a receipt; only then may a new candidate and attempt target the observed revision. Never use supersession for an unknown, no-effect, or verified-success outcome.

## Safe session replacement

Before a session ends, it should capture canonical content into a revision, record current assignment/materialization state, and release only authority it no longer owns. A replacement runtime resumes by reading the Change and assignment, acquiring or reclaiming an expired lease with a new lease ID, and materializing the exact canonical revision. It must not depend on the predecessor's dirty worktree.

Paseo-specific placement and lifecycle mapping is documented in [PASEO.md](PASEO.md). Multi-agent ordering is documented in [MULTI_AGENT_WORKFLOWS.md](MULTI_AGENT_WORKFLOWS.md).

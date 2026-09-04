# ADR-0012: Stable local CLI v1 contract

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

The domain, artifact, SQLite, Native Git, and GitButler crates now provide reusable local APIs, but automation has no stable process boundary. Phase 4 requires equivalent human and machine workflows, explicit concurrency and operation inputs, noninteractive behavior, documented exit codes, and JSON compatibility without making Rust debug strings or provider output public contracts.

## Decision

The executable is `weft`. Its v1 grammar is noun/verb based (`weft change create`, `weft revision append`, and equivalent lifecycle groups). Global options precede the noun. `--state-dir` defaults to `.weft`; it owns `metadata.sqlite3` and `artifacts/`. `weft init` is the only command that creates the state directory deliberately. Other commands require an initialized state. `--format human|json` defaults to human. Commands never prompt; irreversible or terminal transitions require `--yes`, and absence is a usage error rather than an interactive question.

Every mutation requires caller-supplied `--operation-id`, `--actor`, and `--at` Unix milliseconds. Correctness-sensitive mutations additionally require the relevant `--expected-head` or `--expected-version`; the literal `none` represents an empty revision head. IDs are caller supplied in v1 so retries can reproduce exact intent without a hidden random generator. Reusing an operation ID delegates to the storage layer's exact replay/conflict behavior.

JSON mode emits exactly one UTF-8 JSON object and no progress text. Success uses:

```json
{"schema":"weft.cli.v1","ok":true,"command":"change.create","data":{}}
```

Failure uses:

```json
{"schema":"weft.cli.v1","ok":false,"command":"change.create","error":{"code":"usage","message":"...","retryable":false}}
```

The envelope keys, `schema`, boolean `ok`, command names, error codes, field names, scalar representations, array ordering, and omission/null rules are compatibility contracts. Domain and provider types are copied into explicit CLI view models; raw provider JSON, Rust debug output, and database errors are never serialized as data. Human output is not a machine contract.

Exit codes are stable: `0` success, `2` usage/invalid input/confirmation required, `3` not found, `4` stale/conflict/held/duplicate operation intent, `5` unsupported capability or version, `6` provider execution/reconciliation unavailable, `7` integrity/invariant/canonical-content failure, and `1` other local I/O/database failure. Error JSON repeats the matching stable symbolic code and a non-secret message. Retryability is explicit and conservative.

The CLI opens the same SQLite/CAS state per invocation and finishes each metadata mutation transactionally before emitting success. Provider mutations use durable operation IDs and the domain's Running/Reconciliation boundary; an uncertain provider outcome is never flattened into exit `0`. JSON compatibility fixtures and end-to-end process-restart tests are mandatory.

## Alternatives

- Expose Rust `Debug` or derive serialization on domain types: rejected because internal representation changes would silently become API changes.
- Generate IDs/timestamps implicitly: deferred because deterministic agent retries and audit attribution are more important than convenience in v1.
- Prompt for confirmation: rejected because agents and CI require noninteractive behavior.
- Use stdout for logs plus JSON: rejected because one machine-readable object must be safely parseable.
- Create `.weft` implicitly on any command: rejected because a typo or wrong working directory must not mutate the filesystem silently.

## Consequences and limitations

The first CLI is intentionally explicit and verbose. Shell callers must supply durable IDs, operation IDs, actors, timestamps, and expected versions. Convenience aliases, configuration files, generated IDs, relative-time parsing, completion, color, paging, and hosted APIs can be added only without changing v1 JSON semantics.

Provider prerequisites and exact supported subsets remain those of ADR-0010 and ADR-0011. Phase 4 does not turn unsupported remote Git/GitButler behavior into a CLI approximation.

## Required proof

- Parser rejects unknown, duplicate, missing, malformed, and misplaced options without mutation.
- Human and JSON modes perform equivalent operations; JSON emits exactly one stable envelope on success and failure.
- Exit codes cover usage, missing, stale/conflict, unsupported, provider, integrity, and local failures.
- Every mutation proves explicit operation ID/actor/time and relevant expected-version behavior, including exact replay and conflicting reuse.
- Lifecycle groups cover the Phase 4 roadmap and preserve exact target/provider invariants.
- Process restart, concurrent stale writer, provider uncertainty, compatibility fixtures, docs, strict static checks, and the full repository gate pass.

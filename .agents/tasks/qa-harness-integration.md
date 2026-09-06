# Task Record: Weft declarative QA harness integration

## Outcome and scope

- **User/operator result:** Weft has a local, declarative QA entrypoint that
  composes existing proof commands and reports their exact outcome without
  replacing the repository gate.
- **In scope:** Dependency-aware suite planning, preflight capability checks,
  bounded reports, timeout cleanup, stable result/exit semantics, runner tests,
  and Weft-specific smoke/full/release profiles.
- **Out of scope:** Agent scheduling, provider mutation, CMake/DBMS/container
  policies from the source project, and claiming unavailable platform coverage.
- **Affected domain invariants:** None directly; harness evidence must never
  overstate provider, release, or recovery proof.
- **Provider/runtime scope:** Local Rust CLI, Native Git feasibility evidence,
  optional GitButler live evidence, and release archives.
- **Compatibility surface:** CLI, harness, release.

## Acceptance criteria

1. Profiles resolve dependency order once and fail safely on invalid config or cycles.
2. Every suite reports PASS, FAIL, BLOCKED, or SKIPPED with stable exit behavior.
3. Commands run directly (without a shell), time out as a process group, and retain bounded output.
4. `make check` validates the runner contract; smoke and release profiles are exercised end to end.

## Risks

- **Data/security:** Commands must not interpolate through a shell or capture unbounded output.
- **Concurrency/crash recovery:** Timeout must terminate suite descendants; reports must disclose interruption/blocking.
- **Provider divergence/compatibility:** Optional provider/platform evidence remains skipped, never passed.
- **Upgrade/rollback:** Release profile uses the declared version and existing archive proof.

## Evidence and plan

- Relevant paths: `qa/`, `Makefile`, `.agents/agent-harness/`, `docs/DEPLOYMENT.md`.
- Source comparison: retain declarative planning/status/reporting from ezis-nexus;
  exclude CMake, DBMS, compatibility-container, and branch-target rules.

1. Build pure config/planning/report helpers — unit tests for invalid configs, ordering, and exit codes.
2. Build Weft runner lifecycle — tests for preflight, dependency skip, and timeout cleanup.
3. Define Weft profiles — end-to-end smoke/release evidence and full local gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `python3 qa/runner/harness.py self-test` | Passed | Config, dependency, status, preflight, and descendant-timeout tests |
| Harness/docs | `python3 qa/runner/harness.py --profile smoke` | Passed | Canonical gate executed through declarative profile |
| Release | `python3 qa/runner/harness.py --profile release --version v0.2.1` | Passed | Gate, package, clean archive install/restart/uninstall |
| Provider observation | `python3 qa/runner/harness.py --profile provider-observation` | Passed | Explicit live GitButler adapter proof in this environment |

## Decision and follow-up

- **Decision and alternatives rejected:** Retain additive orchestration, not a replacement build system or agent scheduler.
- **Residual risks:** Optional live-provider evidence remains environment-dependent and explicitly reported; it is not part of `make check`.
- **Follow-up:** Add a suite only when its proof contract is stable and directly relevant to Weft.

# Weft QA Harness

The QA harness is a small standard-library orchestration layer around Weft's
existing Make and script entrypoints. It does not replace `make check`, provider
tests, or release scripts; it declares when and how those existing proofs are
combined, then produces bounded local evidence.

## Commands

```bash
# Canonical deterministic repository gate
python3 qa/runner/harness.py --profile smoke

# Add Native Git provider feasibility evidence
python3 qa/runner/harness.py --profile full

# Gate, package, and clean archive install/restart/uninstall proof
python3 qa/runner/harness.py --profile release --version v0.2.1

# Explicit, environment-dependent GitButler adapter observation
python3 qa/runner/harness.py --profile provider-observation

# Runner configuration, planning, lifecycle, timeout, and report tests
python3 qa/runner/harness.py self-test
```

Profiles and suites live in `qa.config.json`. Commands execute as argument arrays,
not shell strings. Dependencies resolve once in order; a dependent suite is
`SKIPPED` if its prerequisite did not establish the required evidence.

## Result contract

| Status | Meaning |
| --- | --- |
| `PASS` | The declared command ran and completed successfully. |
| `FAIL` | The command ran but failed or exceeded its deadline. |
| `BLOCKED` | A required option or mandatory local capability is unavailable. |
| `SKIPPED` | The suite is inapplicable, optional capability is unavailable, or a dependency was not satisfied. |

Exit code `0` means no executed suite failed and at least one proof passed; `1`
means an executed suite failed; `2` means blocked, all-skipped, or invalid
configuration. A skipped provider/platform suite is disclosure, not coverage.

Reports are written to the ignored `qa/reports/<profile>/` directory. `latest.json`
is machine-readable and the timestamped Markdown companion is human-readable.
Output tails are capped at 24,000 characters. On POSIX, a timeout terminates the
suite process group so descendants cannot outlive the result.

## Weft-specific boundaries

The harness adopts declarative planning, lifecycle status, bounded reporting, and
standard-library portability from ezis-nexus. It deliberately excludes that
project's CMake build assumptions, DBMS branch rules, compatibility containers,
and self-hosted runner checks. It never launches or schedules agents; Weft's
durable agent/process boundary remains defined in `.agents/AGENT_PROTOCOL.md`.

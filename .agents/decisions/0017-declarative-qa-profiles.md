# ADR-0017: Declarative QA profiles around the canonical gate

- **Status:** Accepted
- **Date:** 2026-09-06

## Decision

Weft adopts a small standard-library QA runner with declarative smoke, full,
release, and opt-in provider-observation profiles. It resolves suite dependencies,
preflights local capabilities, bounds output, cleans up timed-out process groups,
emits ignored reports, and distinguishes pass, failure, blocking, and skipped
coverage. `make check` remains the canonical local and CI gate; the QA runner
does not replace or recursively redefine it.

## Consequences

- QA configuration contains only Rust/CLI/release-relevant checks.
- Generated reports remain local under `qa/reports/`.
- `SKIPPED` is disclosed evidence, never a passing provider/platform claim.
- DBMS, CMake, compatibility-container, and branch-name rules from the source
  project are deliberately excluded.
- The runner self-test is included in `make check`.

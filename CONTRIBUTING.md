# Contributing to Weft

## Before changing the repository

Read `docs/GOAL.md`, the relevant part of `docs/DOMAIN.md`, and `AGENTS.md`. Material work should create or update a task record from `docs/agent-harness/task-template.md`.

Discuss changes that alter domain invariants, provider semantics, persistence, compatibility, security, or deployment through an ADR before implementation.

## Change expectations

- Keep changes coherent and narrowly scoped.
- Preserve provider neutrality unless a provider-specific capability is explicitly isolated.
- Bind claims to reproducible evidence.
- Add or update tests with implementation changes.
- Update contracts, decisions, and operational documentation in the same change.
- Use English Conventional Commit messages.

## Local verification

```bash
make check
```

As implementation tooling is selected, `make check` remains the stable developer entry point and delegates to language-specific gates.

## Pull requests

Complete the pull-request template with outcome, risk, proof, and residual gaps. A green CI run does not substitute for provider, crash-recovery, compatibility, security, or deployment evidence required by the verification matrix.

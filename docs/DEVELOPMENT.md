# Development Guide

## Stable entry point

Run all repository checks from the root:

```bash
make check
```

The current gate validates the agent harness and local Markdown links. Once Phase 0 selects implementation technology, language-specific format, unit, static-analysis, integration, and package checks will be added behind the same entry point.

## Work lifecycle

1. Define a measurable outcome and acceptance criteria.
2. Classify the affected domain/provider/release boundary.
3. Create a task record for material work.
4. Inspect existing decisions and evidence.
5. Implement one coherent behavior.
6. Run focused proof, then the verification-matrix gates.
7. Record decisions, results, residual risks, and unavailable environments.

Use `.agent/PROGRESS.md` for current project checkpoints and `.agent/DECISIONS.md` as a concise index into durable ADRs. Put large temporary evidence under `.agent/references/`; archive completed working plans under `.agent/archive/`.

## Source-of-truth order

Current code, tests, schemas, generated contracts, and command output outrank plans or progress reports. Normative domain behavior lives in `DOMAIN.md`; an ADR may explain a choice but cannot silently override the domain specification.

## Commit and review

Use focused English Conventional Commit messages. Review the exact intended change, run `make check`, and keep unrelated work out of the commit. Pull requests must identify verification gaps rather than treating unavailable checks as passed.

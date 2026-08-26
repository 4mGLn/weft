# Development Guide

## Stable entry point

Run all repository checks from the root:

```bash
make check
```

The current gate validates the agent harness, local Markdown links, Rust formatting, workspace tests, and Clippy warnings. Provider spikes remain explicit targets because the GitButler spike requires a local GitButler installation and registry access.

## Rust workspace

Phase 1 uses the pinned toolchain in `rust-toolchain.toml`. Keep domain invariants in `crates/weft-domain` independent of SQLite, subprocesses, and provider JSON. Storage and provider crates will depend inward on domain types, never the reverse.

The stable `make check` gate explicitly tests the active Rust host target so a developer's global cross-compilation default cannot silently change local verification. Cross-target builds will become separate release-matrix gates after packaging is decided.

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

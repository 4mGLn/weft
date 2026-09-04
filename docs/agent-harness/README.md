# Agent Harness Operating Guide

Repository documents are the system of record; `AGENTS.md` is their routing map.

## Evidence hierarchy

1. Current code, tests, schemas, canonical fixtures, and command output.
2. Reproduced Native Git/GitButler behavior from declared versions and environments.
3. Normative specifications and accepted decision records.
4. Official upstream documentation for version-sensitive behavior.
5. Clearly labelled inference.

Provider names, branches, commands, and successful happy paths do not prove identity preservation, crash safety, reconciliation, compatibility, or integration atomicity.

## Task lifecycle

1. Use the task template for material work.
2. Declare outcome, scope, affected invariants, provider capability, risk, and proof.
3. Inspect the smallest relevant surface and establish a baseline.
4. Implement one coherent change while preserving unrelated edits.
5. Run focused proof and then the required boundary gates.
6. Update contracts, ADRs, runbooks, and current evidence together.
7. Close with evidence, residual risk, and a concrete resumption condition for gaps.

## Documentation hygiene

- One rule has one canonical owner; link rather than copy.
- Remove obsolete instructions with the behavior that invalidates them.
- Add a project skill only for a repeated workflow with deterministic inputs, outputs, and validation.
- Keep historical task reports out of normative specifications.

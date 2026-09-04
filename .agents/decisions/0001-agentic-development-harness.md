# ADR-0001: Evidence-driven agentic development harness

- **Status:** Accepted
- **Date:** 2026-08-26

## Context

Weft will be developed by humans and multiple agent runtimes while its core product coordinates the same kind of work. The repository needs small authoritative context, measurable task contracts, explicit decision history, and verification that distinguishes current evidence from plans.

The reviewed EZIS repositories demonstrate complementary patterns: routing-map agent instructions and boundary evidence, goal and quality loops, specialist read-only reviewers, single-source project context, progress/decision ledgers, stable verification entry points, and release gates.

## Decision

Adopt those patterns as a Weft-specific harness rather than copying product- or language-specific rules:

- `AGENTS.md` routes to canonical product, domain, roadmap, task, verification, decision, and deployment documents.
- `.agents/` holds agent state, durable ADRs, phase evidence, task records, and agent skills; user-facing operational documentation remains under `docs/`.
- Material work starts with measurable acceptance and ends with evidence and residual risk.
- Specialist reviewers are read-only and scoped to domain, provider, or release boundaries.
- `make check` is the stable local and CI entry point.
- Deployment remains specification-only until Phase 0 selects runtime technology and packaging.

## Alternatives

- Copy an existing repository harness unchanged: rejected because DBMS, frontend, C/CMake, and product-specific rules would misdirect Weft.
- Keep only the three specification files: rejected because it provides no repeatable implementation, evidence, review, or release workflow.
- Select implementation and deployment technology now: rejected because it would bypass the required provider feasibility spike.

## Consequences and proof

The repository gains more process files before runtime code, but each has one owner and an automated drift/link gate. Reassess the harness after Phase 0 and remove rules that do not improve correctness or coordination.

Proof: `make check`, a successful pull-request CI run, and a tag-based specification release dry run before the first release.

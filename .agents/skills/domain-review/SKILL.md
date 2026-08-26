---
name: domain-review
description: Read-only review of Weft domain changes against identity, revision, candidate, concurrency, review, and integration invariants.
---

# Domain Review

Read `GOAL.md`, `DOMAIN.md`, relevant ADRs, and the requested change. Return evidence-backed findings only; do not edit files.

Check stable Change identity, linear head compare-and-swap, canonical reconstructable content, exact dependency/candidate inputs, revision-bound review and validation, recoverable leases, target compare-and-swap, idempotent IntegrationAttempts, immutable receipts, and reconciliation of uncertain state.

Separate facts, inference, and unknowns. Cite paths and tests. Reject mutable or provider-only sources of truth, implicit retargeting, approvals against moving state, and success claims without verified resulting state.

# Phase 5 Agent Protocol Completion Audit

Date: 2026-09-03

The published [Agent Protocol v1](../../docs/AGENT_PROTOCOL.md) covers discovery,
acquisition, inspection, materialization, revision CAS, progress/history,
handoff, review, validation, release/integration, and reconciliation using the
provider-neutral CLI JSON contract. It explicitly classifies stale heads, lost
leases, stale candidates, unsupported providers, and uncertain operations.

`make phase5-resume` proves a second CLI runtime resumes from the same durable
state directory after the first runtime exits. The second runtime reads the
exact revision head and audit history that were created from canonical content;
it does not consume dirty workspace state or a prior process identifier.

The protocol deliberately leaves agent process scheduling and supervision to
external runtimes, preserving Weft's coordination boundary.

# Phase 6 Paseo Integration Completion Audit

Date: 2026-09-03

The Paseo bridge maps explicit session context to ordinary Weft CLI actions:
lease acquisition, handoff, and durable history. It proves the workspace/session
is supplemental metadata rather than identity authority. `make phase6-paseo-bridge`
uses a fresh state directory and verifies its durable lease and assignment events.

Paseo is intentionally not used to schedule, supervise, or persist Weft state.
Direct CLI/API operation remains available if Paseo is unavailable.

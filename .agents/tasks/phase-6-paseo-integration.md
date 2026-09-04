# Task Record: Phase 6 Paseo Integration

## Outcome and scope

- **User/operator result:** Paseo sessions can request Weft acquisition, handoff,
  and inspection without taking ownership of durable domain state.
- **In scope:** Explicit environment mapping, thin requested-action bridge, and
  resume/blocking semantics.
- **Out of scope:** Agent scheduling, supervision, credentials, or a required
  Paseo daemon dependency.
- **Affected invariants:** Assignments and leases persist independently of
  sessions; materializations retain exact Weft revision targets.

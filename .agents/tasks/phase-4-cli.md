# Task Record: Phase 4 CLI

## Outcome and scope

- **User/operator result:** A local, noninteractive `weft` executable exposes durable domain operations with a versioned JSON envelope and documented exit behavior.
- **In scope:** Command grammar, state-directory lifecycle, JSON schema, Change/revision lifecycle, and subsequent domain operation groups.
- **Out of scope:** Hosted service, release packaging, and provider mutation shortcuts.
- **Affected domain invariants:** Exact immutable targets, CAS revision heads, durable operation IDs, and no implicit provider retargeting.
- **Compatibility surface:** CLI and JSON schema.

## Acceptance criteria

1. Commands are noninteractive and accept explicit state, expected-version, and operation inputs where required.
2. JSON success and error output use a stable documented envelope and exit codes.
3. Equivalent Native Git workflows are exercised in human-readable and JSON modes.

## Risks

- **Concurrency/crash recovery:** CLI must delegate all state transitions to the transactional domain repository.
- **Compatibility:** Schema versions and exit codes must be tested before adding command groups.
- **Provider divergence:** Provider commands must preserve explicit conflict and uncertainty classifications.

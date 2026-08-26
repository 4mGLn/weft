# Deployment and Release Policy

## Current phase

Weft has no deployable runtime yet. Until Phase 0 selects the implementation and packaging model, releases contain verified specifications and development-harness artifacts only. Do not add placeholder containers, services, installers, or infrastructure that imply unsupported runtime behavior.

## Specification releases

Tags matching `v*` run the repository gate, create a versioned specification bundle, and publish it through GitHub Releases. The bundle includes product/domain/roadmap documents, contribution and security guidance, agent context, decisions, and task/verification templates.

## Runtime release requirements

Before the first runtime release, record an ADR covering:

- supported operating systems and architectures;
- artifact and canonical-content storage layout;
- config, data, cache, log, and secret ownership;
- Native Git and GitButler prerequisites and capability detection;
- service identity, filesystem and network permissions;
- installation, unattended setup, health, upgrade, rollback, and uninstall behavior;
- checksums, provenance, SBOM, signing, and vulnerability scanning;
- compatibility and migration policy.

Release proof must include a clean environment, restart/recovery, provider divergence, partial integration reconciliation, upgrade, and rollback. Publishing or signing requires explicit authorization.

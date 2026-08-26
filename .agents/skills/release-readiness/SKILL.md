---
name: release-readiness
description: Read-only review of Weft CI, artifacts, publication, installation, upgrade, rollback, signing, and provenance.
---

# Release Readiness

Review the declared release surface without building, publishing, signing, installing, deploying, or changing credentials.

For specification releases, verify repository gates, exact included files, version/tag identity, checksums, and release permissions. For future runtime releases, inspect OS/architecture support, data/config/secret ownership, provider prerequisites, least privilege, health, clean install, unattended setup, restart/recovery, upgrade, rollback, uninstall retention, SBOM, provenance, signing, and vulnerability scanning.

Never accept a release claim from a successful build alone. Report evidence, missing environments, rollback gaps, and residual risks.

# ADR-0014: Minimal runtime archive and release metadata

- **Status:** Accepted
- **Date:** 2026-09-01

## Context

The `v0.1.0` release page exposed the runtime archive, separate checksum and
CycloneDX files, and an archive containing the full project documentation and a
development-only documentation checker. End users install only the runtime;
these additional release assets and payload files are not operational inputs.

## Decision

GitHub Releases publish only the installable runtime archive. CI continues to
generate and verify archive/SBOM SHA-256 sidecars and compares them for
same-runner reproducibility, but does not upload them as release assets.

The archive contains `bin/weft`, install/uninstall helpers, a small operator
reference (`README.md`, `GETTING_STARTED.md`, `MANUAL.md`, `USAGE.md`, and
`LICENSE`),
`SBOM.cdx.json`, and `MANIFEST.sha256`. The SBOM stays embedded as
machine-readable release metadata; the repository documentation tree and
development validation scripts remain in the source repository. GitHub
provenance attests the archive, which covers its embedded SBOM and manifest.

## Consequences

- End users download one installable asset and can compare its digest with the
  digest shown by GitHub.
- CI retains deterministic SBOM/checksum evidence without presenting it as a
  runtime download.
- Upgrade verification supports both legacy releases with sidecars and future
  archive-only releases through their embedded SBOM and manifest.

## Required proof

- Package smoke test verifies the operator reference, embedded SBOM, manifest,
  and absence of `docs/` and `scripts/` in the archive.
- Upgrade/rollback proof accepts legacy sidecars and archive-only metadata.
- Full repository gate and tag-release workflow pass before publication.

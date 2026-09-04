# ADR-0015: Stable latest-release Unix installer asset

- **Status:** Accepted
- **Date:** 2026-09-04

## Context

Downloading `install.sh` from `main` makes a convenient command execute code from
the development branch, rather than from the selected published release. A raw
GitHub URL has no dynamic release selector: a `latest` path would be a mutable
repository ref maintained by the project. GitHub Releases do provide a stable
redirect for a same-named asset on the latest published release.

## Decision

Every successful runtime release uploads the tagged source's Unix `install.sh`
as a release asset with that exact stable name, alongside the versioned runtime
archives. Operators install the latest published stable runtime with:

```sh
curl -fsSL https://github.com/4mGLn/weft/releases/latest/download/install.sh | sh
```

The bootstrapper remains intentionally small. It selects the matching platform
archive from the release metadata, verifies GitHub's published SHA-256 digest,
then delegates to the archive's installer. Exact-version installs use the
versioned release asset and set `WEFT_VERSION` for the shell process. The
versioned runtime archive remains the installable payload and the subject of
GitHub artifact attestation.

## Consequences

- The latest-stable command follows only published GitHub Releases, not `main`.
- Replacing a release asset is security-sensitive; release assets and tagged
  source remain subject to repository release authorization and tag verification.
- Windows keeps its archive-based PowerShell installation path; this decision
  adds only the Unix bootstrap asset.

## Required proof

- Release workflow smoke-tests Linux, macOS, and Windows archives.
- The release workflow uploads `install.sh` from the exact tagged source.
- A published-release smoke test proves the latest-release bootstrapper selects
  a matching asset and verifies its digest before installation.

# Deployment and Release Policy

## Supported runtime

The first deployable surface is the local `weft` CLI built and smoke-tested on GitHub-hosted Ubuntu 24.04 x86_64. Other GNU/Linux distributions and glibc versions may work but are not supported claims. It is not a daemon and opens no network listener. The caller chooses a state directory and grants access only to identities that coordinate that repository. Native Git requires Git 2.38 or newer.

Tags matching `vMAJOR.MINOR.PATCH` run the repository gate, build a relocatable archive, smoke-test it in a clean temporary prefix, and attach GitHub build provenance. GitHub Releases publish only the runtime archive. The archive contains the binary, install/uninstall helpers, a small operator reference (`README.md`, `GETTING_STARTED.md`, `MANUAL.md`, `USAGE.md`, and `LICENSE`), the embedded `SBOM.cdx.json`, and `MANIFEST.sha256`; it does not contain the repository documentation tree or development scripts. CI generates and verifies SHA-256 sidecars and a CycloneDX dependency inventory; the SBOM is embedded at the archive root, not published as a separate asset. A tag must point to a commit on `main`. Publication requires explicit authorization.

## Install and health

Download the archive, optionally compare its SHA-256 digest with the digest displayed by GitHub, extract it, and install without elevated privileges:

```bash
sha256sum weft-0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf weft-0.1.0-x86_64-unknown-linux-gnu.tar.gz
PREFIX="$HOME/.local" ./weft-0.1.0-x86_64-unknown-linux-gnu/install.sh
weft --version
weft --help
```

Operational health is a successful process invocation against the intended state directory. There is no service health endpoint. Configuration consists of explicit CLI arguments. Weft owns metadata/database and canonical artifacts under the selected state directory. It owns no credentials, network cache, or log directory. Provider repositories and worktrees remain outside it.

## Upgrade, rollback, and uninstall

Before upgrade, stop mutating callers and preserve the current binary plus the entire state directory. Install the new archive over the same prefix, run `weft --version`, and read representative Changes, candidates, and IntegrationAttempts. Schema migration occurs when state opens and must not be interrupted.

Rollback the binary only when its documented schema range includes the opened database. Otherwise restore the pre-upgrade state-directory backup together with the old binary. Never mix a restored database with newer canonical-artifact contents. `uninstall.sh` removes only the binary; state is deliberately retained and must be deleted separately by an operator using an exact path.

The `v0.1.0` archive-to-`v0.2.0` candidate checkpoint proves direct binary rollback while both runtimes use metadata schema 7: it creates durable state under `v0.1.0`, upgrades in place, writes and reads a candidate-era Change, restores the old binary and reads both Changes, then restores the complete pre-upgrade state snapshot. It does not claim rollback across a schema migration. A release that raises the schema must document its compatibility range and prove the backup-restore path before publication.

## Evidence and unsupported claims

The archive smoke test proves CI-generated checksum and SBOM verification, embedded-SBOM equality, clean installation, process restart, the absence of shipped project documentation, and uninstall retention. The reproducibility test builds the same source twice in independent Cargo target directories and compares the archive, checksums, and SBOM byte-for-byte; it is a same-runner proof, not a cross-platform or cross-toolchain claim. The upgrade/rollback smoke test verifies any legacy sidecars when present, then verifies the embedded SBOM and manifest for both archives, in-place binary replacement, durable-state reads before and after rollback, and complete state-snapshot restoration. Main CI checks the fixed public `v0.1.0` baseline against the current candidate; a tagged release checks the latest prior GitHub runtime before publication. Repository tests prove provider divergence and partial-integration reconciliation.

GitButler currently requires exactly `but 0.22.0`, a local SHA-1 repository, and a writable GitButler project registry. Its only mutating capability is repository-local `gb-local` fast-forward landing. Canonical import, remote landing, provider reconnect, credentials, remote policy enforcement, artifact signing, vulnerability scanning, additional platforms, package managers, auto-update, services, containers, and hosted deployment are not release claims.

Any release-surface expansion requires an ADR and proof for operating systems/architectures, storage ownership, permissions, provider prerequisites, unattended setup, health, migration, upgrade, rollback, uninstall, checksums, provenance, SBOM, signing, and vulnerability scanning.

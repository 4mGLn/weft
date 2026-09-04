# Deployment and Release Policy

## Supported runtime

The first deployable surface is the local `weft` CLI built and smoke-tested on GitHub-hosted Ubuntu 24.04 x86_64. Other GNU/Linux distributions and glibc versions may work but are not supported claims. It is not a daemon and opens no network listener. The caller chooses a state directory and grants access only to identities that coordinate that repository. Native Git requires Git 2.38 or newer.

Tags matching `vMAJOR.MINOR.PATCH` run the repository gate, build a relocatable archive, smoke-test it in a clean temporary prefix, publish SHA-256 checksums and a CycloneDX dependency inventory, and attach GitHub build provenance. A tag must point to a commit on `main`. Publication requires explicit authorization.

## Install and health

Verify the adjacent checksum, extract the archive, and install without elevated privileges:

```bash
sha256sum -c weft-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf weft-0.1.0-x86_64-unknown-linux-gnu.tar.gz
PREFIX="$HOME/.local" ./weft-0.1.0-x86_64-unknown-linux-gnu/install.sh
weft --version
weft --help
```

Operational health is a successful process invocation against the intended state directory. There is no service health endpoint. Configuration consists of explicit CLI arguments. Weft owns metadata/database and canonical artifacts under the selected state directory. It owns no credentials, network cache, or log directory. Provider repositories and worktrees remain outside it.

## Upgrade, rollback, and uninstall

Before upgrade, stop mutating callers and preserve the current binary plus the entire state directory. Install the new archive over the same prefix, run `weft --version`, and read representative Changes, candidates, and IntegrationAttempts. Schema migration occurs when state opens and must not be interrupted.

Rollback the binary only when its documented schema range includes the opened database. Otherwise restore the pre-upgrade state-directory backup together with the old binary. Never mix a restored database with newer canonical-artifact contents. `uninstall.sh` removes only the binary; state is deliberately retained and must be deleted separately by an operator using an exact path.

The `v0.1.0` archive-to-`v0.1.1` candidate checkpoint proves direct binary rollback while both runtimes use metadata schema 7: it creates durable state under `v0.1.0`, upgrades in place, writes and reads a candidate-era Change, restores the old binary and reads both Changes, then restores the complete pre-upgrade state snapshot. It does not claim rollback across a schema migration. A release that raises the schema must document its compatibility range and prove the backup-restore path before publication.

## Evidence and unsupported claims

The archive smoke test proves checksum verification, clean installation, process restart, and uninstall retention. The upgrade/rollback smoke test verifies checksums and SBOMs for both archives, in-place binary replacement, durable-state reads before and after rollback, and complete state-snapshot restoration. Main CI checks the fixed public `v0.1.0` baseline against the current candidate; a tagged release checks the latest prior GitHub runtime before publication. Repository tests prove provider divergence and partial-integration reconciliation.

GitButler currently requires exactly `but 0.22.0`, a local SHA-1 repository, and a writable GitButler project registry. Its only mutating capability is repository-local `gb-local` fast-forward landing. Canonical import, remote landing, provider reconnect, credentials, remote policy enforcement, artifact signing, vulnerability scanning, additional platforms, package managers, auto-update, services, containers, and hosted deployment are not release claims.

Any release-surface expansion requires an ADR and proof for operating systems/architectures, storage ownership, permissions, provider prerequisites, unattended setup, health, migration, upgrade, rollback, uninstall, checksums, provenance, SBOM, signing, and vulnerability scanning.

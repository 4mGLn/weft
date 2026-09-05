# Weft Runtime Manual

## Install

Extract the runtime archive and install without elevated privileges:

```bash
tar -xzf weft-<version>-x86_64-unknown-linux-gnu.tar.gz
PREFIX="$HOME/.local" ./weft-<version>-x86_64-unknown-linux-gnu/install.sh
weft --version
```

The installer replaces only `PREFIX/bin/weft` atomically. It does not create a
service, open a network port, or select a state directory.

## State and operation

Run project-local setup once to initialize state and wire agent instructions:

```bash
cd /path/to/project
weft setup
weft doctor
```

Setup defaults to `.weft`; pass `--state-dir` when an external launcher manages
the state location. The state directory holds Weft metadata and canonical
artifacts. Keep it backed up before an upgrade or a destructive operator action.
Provider repositories, worktrees, credentials, and agent sessions remain outside
Weft state. Setup never launches agents, reads credentials, or changes user-home
runtime configuration.

## Upgrade, rollback, and uninstall

Before upgrading, stop concurrent mutating callers and back up the complete
state directory. Install a later archive into the same prefix, verify
`weft --version`, and read representative state. Roll back the binary only when
its documented schema range supports the existing state; otherwise restore the
pre-upgrade state backup with the prior binary.

`./uninstall.sh` removes only `PREFIX/bin/weft`. It deliberately retains state.

## Supported boundary

This runtime is supported only as a local Ubuntu 24.04 x86_64 CLI archive. It is
not a daemon, hosted service, container image, package-manager channel, or
auto-updater. Run `weft --help` and consult `USAGE.md` for command usage.

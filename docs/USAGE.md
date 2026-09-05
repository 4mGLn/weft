# Weft Runtime Usage

Run `weft --help` for the complete command grammar. Commands are
noninteractive; JSON mode writes one `weft.cli.v1` envelope to stdout.

For human setup, use `weft setup` once inside a repository and then continue
using the existing agent runtime or orchestrator. `weft doctor` reads state and
wiring health without modifying either. The runtime bridge at
`.weft/runtime-bridge.json` tells an external launcher where shared local state
is and which runtime surfaces setup configured.

```bash
weft --format json --state-dir /path/to/weft-state init
weft --format json --state-dir /path/to/weft-state change create \
  --change-id change-1 --operation-id create-1 --actor operator-1 --at 1000
weft --format json --state-dir /path/to/weft-state change show \
  --change-id change-1
```

The commands after setup are agent/orchestrator protocol operations, not routine
human interaction. A launcher creates and supervises sessions; Weft preserves
Change identity, authority, exact revisions, and recovery evidence.

Use a new caller-owned `--operation-id` for each distinct mutation. Commands
that change a revision head or version require the observed expected value.
Terminal release operations require `--yes`; Weft never prompts.

Exit code `0` means success. `1` is a local error, `2` usage, `3` not found,
`4` concurrency conflict, `5` unsupported capability, `6` provider error, and
`7` integrity failure. A provider mutation with an uncertain result must be
reconciled; do not retry it blindly.

## Windows

Install the latest supported Windows runtime from PowerShell:

```powershell
irm https://raw.githubusercontent.com/4mGLn/weft/main/install.ps1 | iex
```

Set `WEFT_VERSION` to select an exact release, `WEFT_PREFIX` to choose the
installation directory, or `WEFT_REPOSITORY` for a compatible fork. The script
downloads the Windows archive over HTTPS and verifies GitHub's SHA-256 asset
digest before invoking its installer.

To build from source instead:

```powershell
cargo install --git https://github.com/4mGLn/weft.git --package weft-cli
weft --help
```

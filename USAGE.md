# Weft Runtime Usage

Run `weft --help` for the complete command grammar. Commands are
noninteractive; JSON mode writes one `weft.cli.v1` envelope to stdout.

```bash
weft --format json --state-dir /path/to/weft-state init
weft --format json --state-dir /path/to/weft-state change create \
  --change-id change-1 --operation-id create-1 --actor operator-1 --at 1000
weft --format json --state-dir /path/to/weft-state change show \
  --change-id change-1
```

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

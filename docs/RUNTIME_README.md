# Weft Runtime

Weft is a local-first CLI for durable coordination of software Changes across
humans, agents, and supported Git providers. This archive contains only the
runtime and its operator reference.

Start with [Getting Started](GETTING_STARTED.md). Use [Usage](USAGE.md) for the
noninteractive CLI contract, [Paseo Integration](PASEO_INTEGRATION.md) for the
installed `weft-paseo-action` lifecycle adapter, and [the manual](MANUAL.md) for
installation, upgrade, rollback, and support boundaries.

On Unix-like archives, `bin/weft-paseo-action` is a thin, non-scheduling
adapter for Paseo to acquire, checkpoint, replace, and release agent work
through Weft. Windows archives provide the bridge and CLI contract but do not
ship this Bash adapter. It does not start or supervise an agent.
`SBOM.cdx.json` is the embedded CycloneDX component inventory. `MANIFEST.sha256`
lists SHA-256 digests for every other file in this archive. See `LICENSE` for
the distribution status.

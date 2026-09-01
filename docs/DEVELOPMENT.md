# Development Guide

## Stable entry point

Run all repository checks from the root:

```bash
make check
```

The current gate validates the agent harness, local Markdown links, Rust formatting, workspace tests, and Clippy warnings. Provider spikes remain explicit targets because the GitButler spike requires a local GitButler installation and registry access.

## Rust workspace

Phase 1 uses the pinned toolchain in `rust-toolchain.toml`. Keep domain invariants in `crates/weft-domain` independent of SQLite, subprocesses, and provider JSON. Storage and provider crates depend inward on domain types, never the reverse.

`crates/weft-storage-sqlite` owns metadata migrations and transactional repositories. It enables foreign keys, WAL mode, a bounded busy timeout, short immediate write transactions, globally unique exact operation replay, append-only histories, versioned Assignment/Lease projections, exact-revision Materializations reconstructed from provider-evidenced events, and acyclic exact-pin Dependency graphs. Tests use disposable file-backed databases; in-memory SQLite is not evidence for WAL or multi-process behavior.

`crates/weft-artifact` owns the canonical `tree-delta-v1` codec, filesystem CAS, and reconstruction boundary. Manifest bytes and object layout are compatibility contracts defined by ADR-0003. Provider adapters may create inputs and verify base materializations, but provider objects never replace these durable bytes.

`crates/weft-provider-git` owns the version-gated local Native Git adapter. It binds exact commits and trees to provider-neutral artifacts, uses raw filter-independent index plumbing, composes exact captured revisions, guards local target refs with compare-and-swap, and returns normalized evidence for integration/reconciliation. Run its focused fixture proof with `cargo test -p weft-provider-git`; the full `make check` gate includes it.

`crates/weft-provider-gitbutler` owns the exact `but 0.22.0` adapter. It rejects unknown status shapes, normalizes `changeId` values only as provider references, maps exact base-to-tip stacks, exports canonical content through verified Git objects, reports conflicts and external rewrites/removals, and supports only exact repository-local fast-forward landing. The full gate runs hermetic declared-version fixtures. Run `make phase3-gitbutler-live` for the explicit live workflow; it uses disposable repositories and isolated XDG registry/config/cache directories and requires `but 0.22.0`.

The repository-local Cargo configuration pins direct development commands to the
supported Linux GNU target, so a developer-wide cross-compilation default cannot
silently change local verification. `make check` also passes the active Rust host
target explicitly. Cross-target builds will become separate release-matrix gates
after packaging is decided.

## Local CLI

`crates/weft-cli` provides the noninteractive `weft` process boundary. Initialize an explicit state directory, then choose human output (the default) or the stable one-object JSON envelope:

```bash
cargo run -p weft-cli -- --state-dir /tmp/weft-state init
cargo run -p weft-cli -- --format json --state-dir /tmp/weft-state change create \
  --change-id change-1 --operation-id op-create-1 --actor operator-1 --at 1000
```

Mutations require caller-owned operation IDs, actors, timestamps, and relevant expected heads or versions. Terminal transitions require `--yes`; commands never prompt. Run `cargo run -p weft-cli -- --help` for lifecycle groups. Native Git commands reverify exact provider commits against durable canonical revisions before materialization or integration. Provider execution enters durable `reconciling` state on uncertain outcomes; use `native-git reconcile-integration`, never blind re-execution.

The provider-neutral orchestration contract is documented in
[`AGENT_PROTOCOL.md`](AGENT_PROTOCOL.md). Paseo launches and supervises agents
and workspaces according to [`PASEO.md`](PASEO.md), while Weft remains the
durable coordination authority. Multi-agent dependency, review, validation,
resolution, and integration ordering is described in
[`MULTI_AGENT_WORKFLOWS.md`](MULTI_AGENT_WORKFLOWS.md).

## Runtime archive

The initial deployable boundary is the Ubuntu 24.04 x86_64 local CLI archive:

```bash
make package-release VERSION=v0.1.0
make test-release ARCHIVE=dist/weft-0.1.0-x86_64-unknown-linux-gnu.tar.gz
```

The smoke test verifies the checksum, installs into a disposable prefix, checks
version/help, initializes state, creates and reads a Change across processes,
uninstalls the binary, and proves state retention. It does not publish anything.

## Work lifecycle

1. Define a measurable outcome and acceptance criteria.
2. Classify the affected domain/provider/release boundary.
3. Create a task record for material work.
4. Inspect existing decisions and evidence.
5. Implement one coherent behavior.
6. Run focused proof, then the verification-matrix gates.
7. Record decisions, results, residual risks, and unavailable environments.

Use `.agent/PROGRESS.md` for current project checkpoints and `.agent/DECISIONS.md` as a concise index into durable ADRs. Put large temporary evidence under `.agent/references/`; archive completed working plans under `.agent/archive/`.

## Source-of-truth order

Current code, tests, schemas, generated contracts, and command output outrank plans or progress reports. Normative domain behavior lives in `DOMAIN.md`; an ADR may explain a choice but cannot silently override the domain specification.

## Commit and review

Use focused English Conventional Commit messages. Review the exact intended change, run `make check`, and keep unrelated work out of the commit. Pull requests must identify verification gaps rather than treating unavailable checks as passed.

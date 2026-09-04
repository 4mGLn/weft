# Changelog

All notable project changes will be documented here.

## Unreleased

### Added

- Archive-to-archive `v0.1.0` to `v0.1.1` candidate upgrade/rollback proof with checksum/SBOM verification, durable-state compatibility, and complete pre-upgrade snapshot restoration.

## 0.1.0 - 2026-08-27

### Added

- Product goal, normative domain model, and implementation roadmap.
- Evidence-driven agent development harness and public repository automation.
- Reproducible Native Git and GitButler Phase 0 provider spikes.
- Initial Rust domain kernel for revision-head CAS and canonical tree-delta validation.
- Transactional SQLite Change/Revision persistence with WAL, migrations, exact operation replay, append-only audit events, and cross-process stale-head proof.
- Deterministic `tree-delta-v1` encoding, verified no-replace filesystem CAS, provider-independent reconstruction, and SQLite artifact durability enforcement.
- Durable overlapping assignments, versioned exclusive operation leases, exact expiry/reclaim after process failure, schema-v2 migration, and globally unique operation replay.
- Durable exact-revision Materializations with versioned provider observations, immutable evidence/history, active-placement uniqueness, fail-closed reconstruction, and concurrent schema-v3 migration.
- Durable symmetric contextual relationships and acyclic directed dependencies with exact revision pins, derived staleness, explicit repin/removal history, two-writer cycle prevention, and schema-v4 migration.
- Versioned ordered Stacks and immutable `composition-candidate-v1` targets with atomic exact-head/dependency resolution, canonical digests, historical-source verification, derived freshness, replay, and schema-v5 migration.
- Immutable exact-revision/candidate ReviewRequests, reviewer submissions, and ValidationResults with explicit reuse scope, factual staleness, authoritative replay, and schema-v6 migration.
- Exact-candidate IntegrationAttempts with target-scoped recoverable authority, mandatory uncertainty reconciliation, immutable conflicts/resolutions/verified receipts, historical operation replay, and schema-v7 migration.
- Version-gated Native Git adapter with exact SHA-1/SHA-256 observation, filter-independent canonical capture/materialization, pruned-provider-object composition, clean/dirty/diverged worktree evidence, exact-base target CAS, conflict mapping, verified squash results, conservative reconciliation, and bounded descendant-aware subprocess execution.
- Exact-version GitButler adapter with strict `but 0.22.0` JSON normalization, provider-only Change references, base-to-tip stack mapping, canonical artifact export, clean/dirty/conflicted observations, rewrite/removal reconciliation, exact local fast-forward landing receipts, and conservative crash ambiguity handling.
- Stable noninteractive `weft` CLI and `weft.cli.v1` JSON process contract over the complete local lifecycle, with exact operation replay, expected-version guards, confirmations, provider execution, and reconciliation.
- Provider-neutral agent protocol, Paseo workspace/session mapping, and dependency-aware multi-agent workflow guidance that keep scheduling outside Weft.
- Reproducible Ubuntu 24.04 x86_64 runtime archives with atomic local-prefix installation, checksum/SBOM and clean-install smoke proof, state-retaining uninstall, SHA-pinned least-privilege release automation, and provenance attestation.

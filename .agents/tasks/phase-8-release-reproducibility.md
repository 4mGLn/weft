# Task Record: deterministic runtime archive checkpoint

## Outcome and scope

- **User/operator result:** Prove the supported runtime archive is byte-for-byte reproducible from two independent local build directories.
- **In scope:** Candidate archive, SHA-256 checksums, CycloneDX SBOM, separate Cargo target directories, and CI/release gates.
- **Out of scope:** Cross-host, cross-toolchain, cross-platform, or reproducible-build attestation claims.
- **Affected domain invariants:** None.
- **Provider/runtime scope:** Local Ubuntu x86_64 runtime archive only; no provider mutation.
- **Compatibility surface:** artifact | release.

## Acceptance criteria

1. Two release builds use separate Cargo target directories.
2. Archive, archive checksum, SBOM, and SBOM checksum compare byte-for-byte.
3. Main CI and tagged releases run the proof.

## Risks

- **Data/security:** Only temporary build/output directories are written.
- **Concurrency/crash recovery:** No runtime state or provider repository is accessed.
- **Provider divergence/compatibility:** Not applicable.
- **Performance/resource limits:** The proof performs one additional clean-target release build.
- **Upgrade/rollback:** Not applicable.

## Evidence and plan

1. Parameterize package build output — proof: candidate package smoke test.
2. Build twice and compare release outputs — proof: `make test-release-reproducibility`.
3. Enforce in CI/release workflow — proof: public repository gate.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `make test-release-reproducibility VERSION=v0.1.1` | Passed | Independent target directories produced byte-identical archive, checksums, and SBOM. |
| Package/deployment | `make package-release`, `make test-release` | Passed | Standard candidate archive passed integrity, clean install, restart, and uninstall retention. |
| Static/harness | `make check` | Passed | Formatting, workspace tests, strict Clippy, harness, and docs. |
| Public CI | repository gate | Pending | |

## Decision and follow-up

- **Decision and alternatives rejected:** Compare all published release outputs from independent target directories; do not overclaim cross-environment reproducibility.
- **Residual risks:** Different compiler, linker, operating system, or CPU environments are not compared.
- **Unavailable evidence:** Cross-environment deterministic-build evidence.
- **Follow-up, owner, resumption condition:** Add a second supported build environment only with an explicit platform-support decision.

# Task Record: minimal runtime archive and release assets

## Outcome and scope

- **User/operator result:** Release pages expose only the installable runtime archive; the archive contains a small operator reference but excludes the project documentation tree and development helpers.
- **In scope:** Archive layout, operator reference, release upload/attestation subjects, embedded SBOM, legacy upgrade verification, and deployment documentation.
- **Out of scope:** Removing CI-only SBOM/checksum generation, changing runtime platforms, artifact signing, or existing `v0.1.0` assets.
- **Affected domain invariants:** None.
- **Provider/runtime scope:** Ubuntu x86_64 local runtime archive only.
- **Compatibility surface:** artifact | release.

## Acceptance criteria

1. The archive contains runtime files, the root SBOM/manifest, and only the required operator reference; it has no `docs/` or development `scripts/` tree.
2. GitHub tag releases upload and attest only the archive while CI still validates generated checksums and SBOMs.
3. Upgrade/rollback proof supports both legacy sidecars and archive-only releases.

## Risks

- **Data/security:** No runtime data or credentials are written; archive integrity remains covered by GitHub asset digest, provenance, and the embedded manifest.
- **Concurrency/crash recovery:** Not applicable.
- **Provider divergence/compatibility:** Not applicable.
- **Performance/resource limits:** Existing package/reproducibility builds are retained.
- **Upgrade/rollback:** `v0.1.0` keeps its sidecars; future archive-only releases use embedded metadata.

## Evidence and plan

1. Minimize package payload — proof: `make test-release` asserts layout and manifest.
2. Restrict release publication — proof: review tag workflow asset glob and attestation subject.
3. Preserve archive-pair recovery proof — proof: `make test-upgrade-rollback`.

## Validation record

| Check | Command/test | Result | Evidence |
| --- | --- | --- | --- |
| Focused | `make test-release ARCHIVE=dist/weft-0.1.1-x86_64-unknown-linux-gnu.tar.gz` | Passed | Archive has the operator reference and root SBOM, excludes `docs/` and `scripts/`, and installs/restarts/uninstalls correctly. |
| Package/deployment | `make test-release-reproducibility`; `make test-upgrade-rollback` | Passed | Independent builds are byte-identical; public `v0.1.0` legacy sidecars and the candidate embedded manifest/SBOM pass archive-pair recovery. |
| Static/harness | `CARGO_NET_OFFLINE=true make check` | Passed | Harness/docs, formatting, 129 active tests, and strict Clippy pass. |
| Public CI | pull-request repository gate | Pending | |

## Decision and follow-up

- **Decision and alternatives rejected:** Keep checksums/SBOM as CI evidence and retain the embedded SBOM; do not publish separate metadata assets or ship the full project documentation tree.
- **Residual risks:** End users must compare the archive digest displayed by GitHub manually; no detached artifact signature is provided.
- **Unavailable evidence:** Cross-platform runtime and third-party vulnerability scanning.
- **Follow-up, owner, resumption condition:** Publish the behavior only with an authorized post-`v0.1.0` runtime release.

# Verification Matrix

Run the smallest check proving the changed behavior, then the mandatory boundary proof.

| Change class | Minimum proof | Mandatory additional proof |
| --- | --- | --- |
| Domain invariant/state transition | Focused state-machine test | Invalid transition, stale version, persistence round-trip, audit event |
| Revision/canonical content | Capture/reconstruct equality | Provider state removed, digest mismatch, stale-head rejection |
| Dependency/stack/candidate | Exact input snapshot | Cycle rejection, stale upstream, reorder/new-candidate behavior |
| Assignment/lease | Acquire/release path | Competing writer, expiry/reclaim, crash recovery |
| Review/validation | Exact target result | New revision/candidate staleness and explicit reuse policy |
| Native Git provider | Fixture-backed operation | Dirty/diverged state, changed target, conflict, retry/reconciliation |
| GitButler provider | Declared-version fixture | Rewrite/reconnect, capability denial, conflict, external divergence |
| Integration | Planned candidate to receipt | Target CAS failure, duplicate operation ID, crash/uncertain outcome |
| CLI/API schema | Focused command/contract | JSON compatibility, exit codes, noninteractive and invalid input |
| Security/secrets | Allowed path | Denial path, least privilege, redaction, untrusted repository input |
| Performance | Reproducible baseline | Equivalent workload, profiles, bounds, unchanged correctness |
| Package/release | Artifact build/lint | Clean install, restart, upgrade, rollback, provenance/SBOM |
| Harness/docs | Link and ownership review | `make check` |

Never mark an unavailable provider, platform, or failure-path check as passed.

# Paseo Integration

Paseo is Weft's first documented agent launcher integration. The integration is intentionally loose: Paseo manages projects, workspaces, agents, terminals, scripts, schedules, and heartbeats; Weft remains independently operable and owns durable coordination state.

## Mapping

| Paseo | Weft | Rule |
| --- | --- | --- |
| Project | registered repository context | paths locate providers; they are not Repository identity |
| Worktree workspace | Materialization | bind the exact ChangeRevision and provider evidence |
| Agent | Assignment holder | session ID may identify a holder, never a Change |
| Supervised script | verification execution | record a ValidationResult only from exact evidence |
| Agent completion notification | readiness signal | inspect Weft state before launching downstream work |
| Schedule/heartbeat | external trigger | cannot grant leases or infer success |

## Workspace lifecycle

1. Create a Paseo worktree workspace for isolation.
2. Create a Weft Assignment for the chosen Change and acquire a scoped Lease only for exclusive mutation.
3. Materialize the exact revision into the workspace through the selected provider.
4. Launch the agent with the Change ID, exact revision, Assignment ID, state directory, repository ID, and acceptance proof. Do not make the prompt the only copy of these values.
5. Capture progress as canonical content and append a revision with expected-head CAS.
6. Record review/validation against the exact revision or candidate.
7. Release the Materialization, Lease, and Assignment when ownership ends; archive the Paseo workspace separately.

Paseo may stop or archive a session at any point. Resume uses the durable procedure in [AGENT_PROTOCOL.md](AGENT_PROTOCOL.md), optionally in a new workspace and with a new agent. Workspace removal must never precede successful canonical capture when uncommitted work is intended to survive.

## Launch guidance

Register the repository with `paseo project create`. Prefer worktree isolation for mutating agents and local isolation for read-only review. Select a human-configured Paseo agent profile by its current notes rather than hard-coding a provider/model in the repository. Use completion notifications to trigger inspection; do not poll agents as a scheduler loop.

Repository commands exposed to supervised terminals are the Make targets documented in [the development guide](../docs/DEVELOPMENT.md). Secrets, GitHub tokens, signing keys, and provider credentials stay in the launcher/environment and must never enter Weft state, prompts, fixtures, or logs.

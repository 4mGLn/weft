#!/usr/bin/env bash
# Phase 0 feasibility evidence for GitButler virtual branches and rewrite identity.
set -euo pipefail

spike_root="$(mktemp -d /tmp/weft-phase0-gitbutler.XXXXXX)"
trap 'rm -rf "$spike_root"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
state() { but -C "$spike_root" --json status; }
branch_value() {
  local name="$1" field="$2"
  state | jq -er --arg name "$name" --arg field "$field" \
    '.stacks[].branches[] | select(.name == $name) | .commits[0][$field]'
}

but -C "$spike_root" setup --init >/dev/null
base_ref="$(state | jq -er '.mergeBase.commitId')"

# A virtual branch maps one logical change to a stable GitButler change ID.
printf 'change A\n' > "$spike_root/a.txt"
but -C "$spike_root" diff >/dev/null
but -C "$spike_root" commit -b change-a -m 'change A' >/dev/null
change_a_id="$(branch_value change-a changeId)"
change_a_commit="$(branch_value change-a commitId)"

# A second virtual branch can be explicitly stacked on the first.
but -C "$spike_root" branch new change-b --anchor change-a >/dev/null
printf 'change B\n' > "$spike_root/b.txt"
but -C "$spike_root" diff >/dev/null
but -C "$spike_root" commit -b change-b -m 'change B' >/dev/null
change_b_id="$(branch_value change-b changeId)"

# A third Change can coexist in the same workspace as a parallel branch.
but -C "$spike_root" branch new change-parallel >/dev/null
printf 'parallel change\n' > "$spike_root/parallel.txt"
but -C "$spike_root" diff >/dev/null
but -C "$spike_root" commit -b change-parallel -m 'parallel change' >/dev/null
[[ "$(state | jq -er '.stacks | length')" == 2 ]] \
  || fail 'parallel virtual branch was not represented as an independent stack'

# Amending a virtual branch rewrites its provider commit but retains the change ID.
printf 'change A rewritten\n' > "$spike_root/a.txt"
but -C "$spike_root" amend -t change-a >/dev/null
[[ "$(branch_value change-a changeId)" == "$change_a_id" ]] \
  || fail 'GitButler change identity changed during provider rewrite'
[[ "$(branch_value change-a commitId)" != "$change_a_commit" ]] \
  || fail 'provider commit was not rewritten by amend'
[[ "$(branch_value change-b changeId)" == "$change_b_id" ]] \
  || fail 'dependent GitButler change identity changed during rewrite'

# An explicit move converts the independent branch into an ordered stack.
but -C "$spike_root" move change-parallel --above change-b >/dev/null
[[ "$(state | jq -er '.stacks | length')" == 1 ]] \
  || fail 'stack operation did not produce one ordered GitButler stack'

# Landing the stack advances the configured local target only after GitButler completes it.
but -C "$spike_root" land change-parallel --whole-stack --yes >/dev/null
landed_ref="$(state | jq -er '.upstreamState.latestCommit.commitId')"
[[ "$landed_ref" != "$base_ref" ]] || fail 'whole-stack landing did not advance the target'

# A Change that overlaps a later external target update is preserved as a stable
# logical change and marked conflicted when GitButler reconciles the new target.
printf 'branch version\n' > "$spike_root/shared.txt"
but -C "$spike_root" commit -b conflict-change -m 'conflicting branch change' >/dev/null
conflict_change_id="$(branch_value conflict-change changeId)"
recorded_target="$(git -C "$spike_root" rev-parse refs/heads/main)"

# Advance the local target without touching the GitButler workspace, simulating
# a provider change made outside Weft/GitButler.
external_blob="$(printf 'external version\n' | git -C "$spike_root" hash-object -w --stdin)"
external_tree="$({
  git -C "$spike_root" ls-tree "$recorded_target"
  printf '100644 blob %s\tshared.txt\n' "$external_blob"
} | git -C "$spike_root" mktree)"
external_target="$(printf 'external target update\n' |
  git -C "$spike_root" commit-tree "$external_tree" -p "$recorded_target")"
git -C "$spike_root" update-ref refs/heads/main "$external_target" "$recorded_target"

but -C "$spike_root" pull >/dev/null
[[ "$(state | jq -er '.mergeBase.commitId')" == "$external_target" ]] \
  || fail 'GitButler did not reconcile the externally advanced target'
[[ "$(branch_value conflict-change changeId)" == "$conflict_change_id" ]] \
  || fail 'logical change identity changed while reconciling a conflict'
[[ "$(state | jq -er \
  '.stacks[].branches[] | select(.name == "conflict-change") | .commits[0].conflicted')" == true ]] \
  || fail 'GitButler did not expose the rebased commit as conflicted'

printf 'gitbutler-spike: ok\n'
printf 'gitbutler-version: %s\n' "$(but --version)"
printf 'change-a-id: %s\n' "$change_a_id"
printf 'change-b-id: %s\n' "$change_b_id"
printf 'conflict-change-id: %s\n' "$conflict_change_id"
printf 'reconciled-target: %s\n' "$external_target"

#!/usr/bin/env bash
set -euo pipefail

root="$(mktemp -d /tmp/weft-paseo-bridge.XXXXXX)"
trap 'rm -rf "$root"' EXIT
script_dir="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
state="$root/state"
project="$root/project"
repository="$root/repository"
workspace_one="$root/workspace-one"
workspace_two="$root/workspace-two"
weft_bin="${WEFT_BIN:-}"
action_bin="${WEFT_ACTION_BIN:-$script_dir/paseo-weft-action.sh}"

run_weft() {
  if [[ -n "$weft_bin" ]]; then
    "$weft_bin" --format json --state-dir "$state" "$@"
  else
    cargo run --offline -p weft-cli -- --format json --state-dir "$state" "$@"
  fi
}

action() {
  local now="$1"
  shift
  WEFT_BIN="$weft_bin" \
    WEFT_STATE_DIR="$state" \
    WEFT_CHANGE_ID=change-1 \
    WEFT_ACTOR="${WEFT_ACTION_ACTOR:-agent-one}" \
    WEFT_NOW_UNIX_MS="$now" \
    WEFT_SUBJECT_KIND=agent \
    WEFT_SUBJECT_ID="${WEFT_ACTION_SUBJECT:-agent-one}" \
    WEFT_REPOSITORY="${WEFT_ACTION_REPOSITORY:-$repository}" \
    WEFT_REPOSITORY_ID=repo-1 \
    "$action_bin" "$@"
}

git init --quiet "$repository"
git -C "$repository" config user.name Weft
git -C "$repository" config user.email weft@example.test
git -C "$repository" config commit.gpgsign false
printf 'base\n' > "$repository/file.txt"
git -C "$repository" add file.txt
git -C "$repository" commit --quiet -m base
base="$(git -C "$repository" rev-parse HEAD)"
printf 'first revision\n' > "$repository/file.txt"
git -C "$repository" commit --quiet -am first-revision
revision_one="$(git -C "$repository" rev-parse HEAD)"

mkdir "$project"
run_weft setup --project-dir "$project" --runtime paseo >/dev/null
run_weft change create \
  --change-id change-1 \
  --operation-id change-create-1 \
  --actor setup-test \
  --at 1 >/dev/null
test -f "$project/.weft/runtime-bridge.json"
grep -q 'weft.runtime-bridge.v1' "$project/.weft/runtime-bridge.json"
grep -q 'weft.cli.v1' "$project/.weft/runtime-bridge.json"
grep -q 'paseo-action-adapter-v2' "$project/.weft/runtime-bridge.json"
grep -q 'weft-paseo-action' "$project/.weft/runtime-bridge.json"
configured_state="$(sed -n 's/^  "state_dir": "\(.*\)",$/\1/p' "$project/.weft/runtime-bridge.json")"
test "$configured_state" = "$state"
state="$configured_state"

if action 9 history unexpected >/dev/null 2>&1; then
  printf '%s\n' 'adapter accepted an unexpected history argument' >&2
  exit 1
fi
if action 9 acquire lease-usage-only >/dev/null 2>&1; then
  printf '%s\n' 'adapter accepted incomplete acquire arguments' >&2
  exit 1
fi

action 10 assign assignment-1 assignment-create-1
action 11 acquire lease-1 implement 0 100 lease-acquire-1
action 12 renew lease-1 1 200 lease-renew-1
action 13 checkpoint revision-1 none "$base" "$revision_one" revision-capture-1
action 14 materialize materialization-1 workspace-1 revision-1 "$revision_one" "$workspace_one" materialization-create-1
test "$(cat "$workspace_one/file.txt")" = 'first revision'

printf 'checkpointed progress\n' > "$workspace_one/file.txt"
action 15 observe materialization-1 1 "$workspace_one" "$revision_one" materialization-observe-1 >/dev/null
git -C "$workspace_one" commit --quiet -am checkpointed-progress
revision_two="$(git -C "$workspace_one" rev-parse HEAD)"
action 20 checkpoint revision-2 revision-1 "$revision_one" "$revision_two" revision-capture-2
action 21 handoff assignment-2 agent-two assignment-create-2
action 22 release-materialization materialization-1 2 "$workspace_one" materialization-release-1
action 23 release-assignment assignment-1 1 assignment-release-1
test ! -e "$workspace_one"

# A replacement session reclaims the expired scope and materializes the exact
# checkpoint into a different isolated workspace.
WEFT_ACTION_ACTOR=agent-two WEFT_ACTION_SUBJECT=agent-two action 201 acquire lease-2 implement 2 300 lease-acquire-2
WEFT_ACTION_ACTOR=agent-two WEFT_ACTION_SUBJECT=agent-two action 202 materialize materialization-2 workspace-2 revision-2 "$revision_two" "$workspace_two" materialization-create-2
test "$(cat "$workspace_two/file.txt")" = 'checkpointed progress'
# The exact materialization is canonical Weft content applied on top of its
# base, so return the provider worktree to that base before safe removal.
git -C "$workspace_two" reset --hard --quiet "$revision_one"
WEFT_ACTION_ACTOR=agent-two WEFT_ACTION_SUBJECT=agent-two action 210 release-materialization materialization-2 1 "$workspace_two" materialization-release-2
released_lease="$(WEFT_ACTION_ACTOR=agent-two WEFT_ACTION_SUBJECT=agent-two action 211 release-lease lease-2 3 lease-release-2)"
[[ "$released_lease" == *'"lease_id":"lease-2"'* && "$released_lease" == *'"version":4'* ]]
released_assignment="$(WEFT_ACTION_ACTOR=agent-two WEFT_ACTION_SUBJECT=agent-two action 212 release-assignment assignment-2 1 assignment-release-2)"
[[ "$released_assignment" == *'"assignment_id":"assignment-2"'* && "$released_assignment" == *'"active":false'* ]]
test ! -e "$workspace_two"

history="$(WEFT_ACTION_ACTOR=agent-two WEFT_ACTION_SUBJECT=agent-two action 213 history)"
[[ "$history" == *'revision.appended'* ]]
assignments="$(run_weft assignment list --change-id change-1)"
[[ "$assignments" == *'assignment-1'* && "$assignments" == *'assignment-2'* ]]
[[ "$assignments" == *'"active":false'* ]]
lease="$(run_weft lease show --change-id change-1 --operation implement)"
[[ "$lease" == *'"lease":null'* ]]
materializations="$(run_weft materialization list --change-id change-1)"
[[ "$materializations" == *'materialization-1'* && "$materializations" == *'materialization-2'* ]]
[[ "$materializations" == *'"state":"released"'* ]]

printf 'paseo-weft-bridge: ok\n'

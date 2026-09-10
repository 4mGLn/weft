#!/usr/bin/env bash
# Paseo-to-Weft action adapter v2; process scheduling remains in Paseo.
set -euo pipefail

: "${WEFT_STATE_DIR:?WEFT_STATE_DIR is required}"
: "${WEFT_CHANGE_ID:?WEFT_CHANGE_ID is required}"
: "${WEFT_ACTOR:?WEFT_ACTOR is required}"
: "${WEFT_NOW_UNIX_MS:?WEFT_NOW_UNIX_MS is required}"

state_dir="$WEFT_STATE_DIR"
subject_kind="${WEFT_SUBJECT_KIND:-agent}"
subject_id="${WEFT_SUBJECT_ID:-$WEFT_ACTOR}"
assignment_role="${WEFT_ASSIGNMENT_ROLE:-implementer}"
weft_bin="${WEFT_BIN:-}"
script_dir="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "$weft_bin" && -x "$script_dir/weft" ]]; then
  weft_bin="$script_dir/weft"
fi

require_var() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    printf '%s is required\n' "$name" >&2
    exit 2
  fi
}

weft() {
  if [[ -n "$weft_bin" ]]; then
    "$weft_bin" --format json --state-dir "$state_dir" "$@"
  elif type -P weft >/dev/null 2>&1; then
    command weft --format json --state-dir "$state_dir" "$@"
  else
    cargo run --offline --manifest-path "$script_dir/../Cargo.toml" \
      -p weft-cli -- --format json --state-dir "$state_dir" "$@"
  fi
}

require_repository() {
  require_var WEFT_REPOSITORY
}

require_repository_id() {
  require_repository
  require_var WEFT_REPOSITORY_ID
}

usage() {
  printf '%s\n' 'usage: weft-paseo-action ACTION [ACTION_ARGUMENTS...]' >&2
  printf '%s\n' 'actions: assign, acquire, renew, checkpoint, materialize, observe, release-materialization, release-lease, release-assignment, handoff, history' >&2
  exit 2
}

require_args() {
  local minimum="$1"
  local maximum="$2"
  shift 2
  if (( $# < minimum || $# > maximum )); then
    printf 'action %s expects between %s and %s arguments; got %s\n' \
      "$action" "$minimum" "$maximum" "$#" >&2
    exit 2
  fi
}

action="${1:-}"
[[ -n "$action" ]] || usage
shift
case "$action" in
  assign)
    require_args 2 3 "$@"
    assignment_id="$1"
    operation_id="$2"
    role="${3:-$assignment_role}"
    weft assignment create \
      --assignment-id "$assignment_id" \
      --change-id "$WEFT_CHANGE_ID" \
      --subject-kind "$subject_kind" \
      --subject-id "$subject_id" \
      --role "$role" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS"
    ;;
  acquire)
    require_args 5 5 "$@"
    lease_id="$1"
    operation="$2"
    expected_version="$3"
    expires_at="$4"
    operation_id="$5"
    weft lease acquire \
      --lease-id "$lease_id" \
      --change-id "$WEFT_CHANGE_ID" \
      --operation "$operation" \
      --holder-kind "$subject_kind" \
      --holder-id "$subject_id" \
      --expected-version "$expected_version" \
      --expires-at "$expires_at" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS"
    ;;
  renew)
    require_args 4 4 "$@"
    lease_id="$1"
    expected_version="$2"
    expires_at="$3"
    operation_id="$4"
    weft lease renew \
      --lease-id "$lease_id" \
      --expected-version "$expected_version" \
      --expires-at "$expires_at" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS"
    ;;
  checkpoint)
    require_args 5 5 "$@"
    revision_id="$1"
    expected_head="$2"
    base_revision="$3"
    provider_revision="$4"
    operation_id="$5"
    require_repository_id
    weft native-git capture \
      --repository "$WEFT_REPOSITORY" \
      --repository-id "$WEFT_REPOSITORY_ID" \
      --base-revision "$base_revision" \
      --provider-revision "$provider_revision" \
      --change-id "$WEFT_CHANGE_ID" \
      --revision-id "$revision_id" \
      --expected-head "$expected_head" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS"
    ;;
  materialize)
    require_args 6 6 "$@"
    materialization_id="$1"
    workspace_id="$2"
    revision_id="$3"
    provider_revision="$4"
    destination="$5"
    operation_id="$6"
    require_repository
    weft native-git materialize \
      --repository "$WEFT_REPOSITORY" \
      --provider-revision "$provider_revision" \
      --change-id "$WEFT_CHANGE_ID" \
      --revision-id "$revision_id" \
      --destination "$destination" \
      --materialization-id "$materialization_id" \
      --workspace-id "$workspace_id" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS"
    ;;
  observe)
    require_args 5 5 "$@"
    materialization_id="$1"
    expected_version="$2"
    worktree="$3"
    provider_revision="$4"
    operation_id="$5"
    require_repository
    weft native-git observe-materialization \
      --repository "$WEFT_REPOSITORY" \
      --worktree "$worktree" \
      --provider-revision "$provider_revision" \
      --materialization-id "$materialization_id" \
      --expected-version "$expected_version" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS"
    ;;
  release-materialization)
    require_args 4 4 "$@"
    materialization_id="$1"
    expected_version="$2"
    worktree="$3"
    operation_id="$4"
    require_repository
    weft native-git release-materialization \
      --repository "$WEFT_REPOSITORY" \
      --worktree "$worktree" \
      --materialization-id "$materialization_id" \
      --expected-version "$expected_version" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS" \
      --yes
    ;;
  release-lease)
    require_args 3 3 "$@"
    lease_id="$1"
    expected_version="$2"
    operation_id="$3"
    weft lease release \
      --lease-id "$lease_id" \
      --expected-version "$expected_version" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS" \
      --yes
    ;;
  release-assignment)
    require_args 3 3 "$@"
    assignment_id="$1"
    expected_version="$2"
    operation_id="$3"
    weft assignment release \
      --assignment-id "$assignment_id" \
      --expected-version "$expected_version" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS" \
      --yes
    ;;
  handoff)
    require_args 3 5 "$@"
    assignment_id="$1"
    successor_id="$2"
    operation_id="$3"
    role="${4:-$assignment_role}"
    successor_kind="${5:-$subject_kind}"
    weft assignment create \
      --assignment-id "$assignment_id" \
      --change-id "$WEFT_CHANGE_ID" \
      --subject-kind "$successor_kind" \
      --subject-id "$successor_id" \
      --role "$role" \
      --operation-id "$operation_id" \
      --actor "$WEFT_ACTOR" \
      --at "$WEFT_NOW_UNIX_MS"
    ;;
  history)
    require_args 0 0 "$@"
    weft change history --change-id "$WEFT_CHANGE_ID"
    ;;
  *)
    printf 'unsupported Paseo Weft action: %s\n' "$action" >&2
    exit 2
    ;;
esac

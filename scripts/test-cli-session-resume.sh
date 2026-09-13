#!/usr/bin/env bash
# Proves a new runtime resumes an exact canonical revision from durable state.
set -euo pipefail

root="$(mktemp -d /tmp/weft-cli-resume.XXXXXX)"
trap 'rm -rf "$root"' EXIT
repo="$root/repository"
state="$root/state"
weft_bin="${WEFT_BIN:-}"

run_weft() {
  if [[ -n "$weft_bin" ]]; then
    "$weft_bin" --format json --state-dir "$state" "$@"
  else
    cargo run --offline -p weft-cli -- --format json --state-dir "$state" "$@"
  fi
}

git init --quiet "$repo"
git -C "$repo" config user.name Weft
git -C "$repo" config user.email weft@example.test
git -C "$repo" config commit.gpgsign false
printf 'base\n' > "$repo/file"
git -C "$repo" add file
git -C "$repo" commit --quiet -m base
base="$(git -C "$repo" rev-parse HEAD)"
printf 'canonical revision\n' > "$repo/file"
git -C "$repo" commit --quiet -am revision
revision="$(git -C "$repo" rev-parse HEAD)"

run_weft init >/dev/null
run_weft change create \
  --change-id change-1 \
  --operation-id change-create-1 \
  --actor prior-runtime \
  --at 1000 >/dev/null
run_weft native-git capture \
  --repository "$repo" \
  --repository-id repo-1 \
  --base-revision "$base" \
  --provider-revision "$revision" \
  --change-id change-1 \
  --revision-id revision-1 \
  --expected-head none \
  --operation-id revision-capture-1 \
  --actor prior-runtime \
  --at 1010 >/dev/null

# This separate process simulates the prior runtime no longer existing.
resumed="$(run_weft change show --change-id change-1)"
[[ "$resumed" == *'"head_revision_id":"revision-1"'* ]]
run_weft change history --change-id change-1 \
  | grep -q 'revision.appended'

printf 'cli-session-resume: ok\n'

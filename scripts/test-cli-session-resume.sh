#!/usr/bin/env bash
# Proves a new runtime resumes an exact canonical revision from durable state.
set -euo pipefail

root="$(mktemp -d /tmp/weft-cli-resume.XXXXXX)"
trap 'rm -rf "$root"' EXIT
repo="$root/repository"
state="$root/state"

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

cargo run --offline -p weft-cli -- --state "$state" change create change-1 --json >/dev/null
cargo run --offline -p weft-cli -- --state "$state" change revise change-1 \
  --repository "$repo" --base "$base" --revision revision-1 \
  --expected-head none --json >/dev/null

# This separate process simulates the prior runtime no longer existing.
resumed="$(cargo run --offline -p weft-cli -- --state "$state" change show change-1 --json)"
[[ "$resumed" == *'"headRevisionId":"revision-1"'* ]]
cargo run --offline -p weft-cli -- --state "$state" history change-1 --json \
  | grep -q 'revision-appended'

printf 'cli-session-resume: ok\n'

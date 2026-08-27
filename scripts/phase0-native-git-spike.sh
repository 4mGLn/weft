#!/usr/bin/env bash
# Phase 0 feasibility evidence for provider-neutral canonical Git revisions.
set -euo pipefail

spike_root="$(mktemp -d /tmp/weft-phase0-native-git.XXXXXX)"
trap 'rm -rf "$spike_root"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
assert_eq() { [[ "$1" == "$2" ]] || fail "$3 (expected $1, got $2)"; }
digest() { sha256sum "$1" | awk '{print $1}'; }
new_repo() {
  local path="$1"
  git init --quiet "$path"
  git -C "$path" config user.name "Weft Phase 0"
  git -C "$path" config user.email "phase0@weft.invalid"
  git -C "$path" config commit.gpgSign false
}

repo="$spike_root/repository"
new_repo "$repo"
printf 'base\n' > "$repo/shared.txt"
git -C "$repo" add shared.txt
git -C "$repo" commit --quiet -m base
base="$(git -C "$repo" rev-parse HEAD)"

# A revision's durable content is a binary-capable patch against its exact base.
git -C "$repo" switch --quiet -c change-a
printf 'change A\n' > "$repo/a.txt"
git -C "$repo" add a.txt
git -C "$repo" commit --quiet -m change-a
change_a="$(git -C "$repo" rev-parse HEAD)"
artifact_a="$spike_root/change-a.patch"
git -C "$repo" diff --binary "$base" "$change_a" > "$artifact_a"
artifact_a_digest="$(digest "$artifact_a")"

# Canonical reconstruction remains valid without the original branch reference.
reconstructed="$spike_root/reconstructed"
git clone --quiet "$repo" "$reconstructed"
git -C "$reconstructed" switch --quiet --detach "$base"
git -C "$reconstructed" apply --index "$artifact_a"
assert_eq "$(git -C "$repo" rev-parse "$change_a^{tree}")" \
  "$(git -C "$reconstructed" write-tree)" \
  "canonical artifact reconstructs the exact revision tree"
git -C "$repo" switch --quiet --detach "$base"
git -C "$repo" branch -D change-a >/dev/null
[[ -s "$artifact_a" ]] || fail "canonical artifact was lost when provider ref was removed"

# A provider rewrite is a new provider object; the earlier artifact still reconstructs.
git -C "$repo" switch --quiet -c change-a-rewritten "$base"
printf 'change A rewritten\n' > "$repo/a.txt"
git -C "$repo" add a.txt
git -C "$repo" commit --quiet -m change-a-rewritten
rewritten_a="$(git -C "$repo" rev-parse HEAD)"
[[ "$(git -C "$repo" rev-parse "$rewritten_a^{tree}")" != "$(git -C "$reconstructed" write-tree)" ]] \
  || fail "rewrite unexpectedly preserved revision content"
assert_eq "$artifact_a_digest" "$(digest "$artifact_a")" \
  "rewrite must not mutate old canonical content"

# Candidate composition is an ordered snapshot, not moving branch names.
git -C "$repo" switch --quiet -c change-b "$change_a"
printf 'change B\n' > "$repo/b.txt"
git -C "$repo" add b.txt
git -C "$repo" commit --quiet -m change-b
change_b="$(git -C "$repo" rev-parse HEAD)"
artifact_b="$spike_root/change-b.patch"
git -C "$repo" diff --binary "$change_a" "$change_b" > "$artifact_b"
candidate="$spike_root/candidate.patch"
cat "$artifact_a" "$artifact_b" > "$candidate"
candidate_checkout="$spike_root/candidate-checkout"
git clone --quiet "$repo" "$candidate_checkout"
git -C "$candidate_checkout" switch --quiet --detach "$base"
git -C "$candidate_checkout" apply --index "$candidate"
assert_eq "$(git -C "$repo" rev-parse "$change_b^{tree}")" \
  "$(git -C "$candidate_checkout" write-tree)" \
  "ordered candidate reconstructs its exact inputs"

# A changed target rejects the planned integration instead of silently replanning.
target="$spike_root/target"
git clone --quiet "$repo" "$target"
git -C "$target" config user.name "Weft Phase 0"
git -C "$target" config user.email "phase0@weft.invalid"
git -C "$target" config commit.gpgSign false
git -C "$target" switch --quiet -c target "$base"
expected_target="$(git -C "$target" rev-parse HEAD)"
printf 'external target update\n' > "$target/target.txt"
git -C "$target" add target.txt
git -C "$target" commit --quiet -m external-target-update
actual_target="$(git -C "$target" rev-parse HEAD)"
[[ "$expected_target" != "$actual_target" ]] || fail "target update was not observed"
if [[ "$(git -C "$target" rev-parse HEAD)" == "$expected_target" ]]; then
  fail "stale integration plan would run"
fi

# A conflicting application is observable as a provider conflict, not a success.
git -C "$repo" switch --quiet -c conflict-left "$base"
printf 'left\n' > "$repo/shared.txt"
git -C "$repo" add shared.txt
git -C "$repo" commit --quiet -m conflict-left
git -C "$repo" switch --quiet -c conflict-right "$base"
printf 'right\n' > "$repo/shared.txt"
git -C "$repo" add shared.txt
git -C "$repo" commit --quiet -m conflict-right
if git -C "$repo" merge --no-commit conflict-left >/dev/null 2>&1; then
  fail "expected integration conflict did not occur"
fi
[[ -n "$(git -C "$repo" diff --name-only --diff-filter=U)" ]] \
  || fail "provider did not expose conflicted paths"
git -C "$repo" merge --abort

# Reconciliation compares recorded provider state with the observed ref after external work.
recorded_ref="$(git -C "$target" rev-parse HEAD)"
printf 'outside Weft\n' > "$target/reconciled.txt"
git -C "$target" add reconciled.txt
git -C "$target" commit --quiet -m external-provider-change
observed_ref="$(git -C "$target" rev-parse HEAD)"
[[ "$recorded_ref" != "$observed_ref" ]] || fail "external divergence was not detected"

printf 'native-git-spike: ok\n'
printf 'canonical-artifact-sha256: %s\n' "$artifact_a_digest"
printf 'candidate-sha256: %s\n' "$(digest "$candidate")"
printf 'git-version: %s\n' "$(git --version)"

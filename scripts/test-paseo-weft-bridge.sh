#!/usr/bin/env bash
set -euo pipefail

root="$(mktemp -d /tmp/weft-paseo-bridge.XXXXXX)"
trap 'rm -rf "$root"' EXIT
state="$root/state"

cargo run --offline -p weft-cli -- --state "$state" change create change-1 --json >/dev/null
WEFT_STATE_DIR="$state" WEFT_CHANGE_ID=change-1 WEFT_ACTOR=paseo-agent WEFT_NOW_UNIX_MS=10 \
  ./scripts/paseo-weft-action.sh acquire inspect 100 >/dev/null
WEFT_STATE_DIR="$state" WEFT_CHANGE_ID=change-1 WEFT_ACTOR=paseo-agent WEFT_NOW_UNIX_MS=11 \
  ./scripts/paseo-weft-action.sh handoff handoff-1 next-agent >/dev/null
history="$(WEFT_STATE_DIR="$state" WEFT_CHANGE_ID=change-1 WEFT_ACTOR=paseo-agent WEFT_NOW_UNIX_MS=12 ./scripts/paseo-weft-action.sh history)"
[[ "$history" == *'lease-acquired'* && "$history" == *'assignment-recorded'* ]]
printf 'paseo-weft-bridge: ok\n'

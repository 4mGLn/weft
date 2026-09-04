#!/usr/bin/env bash
# Thin Paseo-to-Weft action bridge; process scheduling remains in Paseo.
set -euo pipefail

: "${WEFT_STATE_DIR:?WEFT_STATE_DIR is required}"
: "${WEFT_CHANGE_ID:?WEFT_CHANGE_ID is required}"
: "${WEFT_ACTOR:?WEFT_ACTOR is required}"
: "${WEFT_NOW_UNIX_MS:?WEFT_NOW_UNIX_MS is required}"

action="${1:?action is required: acquire|handoff|history}"
case "$action" in
  acquire)
    operation="${2:?operation is required}"
    expires="${3:?expiry timestamp is required}"
    cargo run --offline -p weft-cli -- --state "$WEFT_STATE_DIR" \
      change acquire "$WEFT_CHANGE_ID" --operation "$operation" \
      --holder "$WEFT_ACTOR" --now "$WEFT_NOW_UNIX_MS" --expires "$expires" --json
    ;;
  handoff)
    assignment="${2:?assignment ID is required}"
    subject="${3:?handoff subject is required}"
    cargo run --offline -p weft-cli -- --state "$WEFT_STATE_DIR" \
      change handoff "$WEFT_CHANGE_ID" --assignment "$assignment" --to "$subject" \
      --actor "$WEFT_ACTOR" --at "$WEFT_NOW_UNIX_MS" --json
    ;;
  history)
    cargo run --offline -p weft-cli -- --state "$WEFT_STATE_DIR" history "$WEFT_CHANGE_ID" --json
    ;;
  *) printf 'unsupported Paseo Weft action: %s\n' "$action" >&2; exit 2 ;;
esac

#!/bin/sh
DIR="$(cd "$(dirname "$0")" && pwd)"
STATE="$DIR/state.json"
TMP="$STATE.tmp.$$"

CURRENT=$(jq -r '.current_state'      "$STATE")

if [ "$CURRENT" = "[*]" ]; then
  CHECKLIST=$(jq -r '.checklist' "$STATE")
  echo "steplock: nothing to acknowledge for '$CHECKLIST' (session already complete)"
  exit 0
fi

NEXT=$(jq -r '.next_state // empty'   "$STATE")
NEXT="${1:-$NEXT}"

VALID=$(jq -r '.transitions[]' "$STATE")
MATCHED=$(printf '%s\n' $VALID | grep -Fx "$NEXT")

if [ -z "$MATCHED" ]; then
  echo "steplock: invalid next state '$NEXT'" >&2
  echo "Valid transitions from '$CURRENT':" >&2
  printf '%s\n' $VALID | sed 's/^/  /' >&2
  exit 1
fi

jq --arg cur "$CURRENT" --arg next "$NEXT" '
  .visited      += [$cur] |
  .current_state = $next  |
  .next_state    = null
' "$STATE" > "$TMP" && mv "$TMP" "$STATE"

STEPLOCK_DIR="$(cd "$DIR/../../.." && pwd)"
SESSION="$(basename "$(dirname "$DIR")")"
CHECKLIST="$(jq -r '.checklist' "$STATE")"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
jq -cn --arg e "ack" --arg c "$CHECKLIST" --arg s "$CURRENT" \
    --arg sess "$SESSION" --arg ts "$TS" \
    '{event:$e,checklist:$c,state:$s,session:$sess,ts:$ts}' \
    >> "$STEPLOCK_DIR/audit.log" 2>/dev/null || true

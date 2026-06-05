#!/bin/sh
DIR="$(cd "$(dirname "$0")" && pwd)"
STATE="$DIR/state.json"
VISITED=$(jq -r '.visited[]?' "$STATE" 2>/dev/null)

echo "Checklist: git-push-quality-gate (4 items)"

check() {
  label="$1"; state="$2"
  if printf '%s\n' $VISITED | grep -qxF "$state"; then
    printf "  [x] %s\n" "$label"
  else
    printf "  [ ] %s\n" "$label"
  fi
}

check 'Did you write clean, readable code?' 'clean_code'
check 'Did you increase test coverage by at least a little?' 'test_coverage'
check 'Did you update relevant documentation?' 'documentation'
check 'Did you check for hardcoded secrets or credentials?' 'no_secrets'

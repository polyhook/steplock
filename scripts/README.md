# Scripts

## ack.sh

Acknowledges the current checklist state and advances to the next one.

Written by steplock to `.steplock/sessions/<scope-key>/ack.sh` on first invocation. Never regenerated once written — the agent can run it without arguments in the common (linear) case, or pass a state name for branching flows.

**Usage**

```sh
# Linear flow — auto-advance
sh .steplock/sessions/<scope-key>/ack.sh

# Branching flow — specify next state
sh .steplock/sessions/<scope-key>/ack.sh <next-state>
```

**What it does**

1. Reads `state.json` from its own directory
2. Validates `$1` (or `next_state` from the file) against `transitions[]`
3. Appends `current_state` to `visited[]`, sets `current_state = next_state`
4. Writes the updated state atomically via temp + `mv`

Requires `jq` at runtime.

## preview.sh

Prints all checklist items with `[x]` / `[ ]` markers based on the current `visited[]` list.

Written by steplock to `.steplock/sessions/<scope-key>/preview.sh` when `allow_preview_request = true`. Content is generated from the `FlowGraph` at session init — each state appears in topological order.

**Usage**

```sh
sh .steplock/sessions/<scope-key>/preview.sh
```

**Example output**

```
Checklist: git-push-quality-gate (4 items)
  [x] Did you write clean, readable code?
  [ ] Did you increase test coverage by at least a little?
  [ ] Did you update relevant documentation?
  [ ] Did you check for hardcoded secrets or credentials?
```

# steplock

**Stateful quality gates for AI coding agent actions.**

steplock sits on top of the [polyhook](https://github.com/tupe12334/polyhook) SDK. It intercepts hook events and gates actions behind a sequential checklist — one question per invocation, acknowledged by the agent, persisted to disk.

---

## The Problem

AI coding agents act fast. They run commands, push code, and make changes — but they have no built-in mechanism to enforce quality gates before doing so. Every hook invocation starts from scratch: no memory of what was already checked, no way to hold the agent accountable to a sequential process.

You want the agent to answer "did you increase test coverage?" before every push — not just the first time, but every time. You want it to work regardless of which AI tool the agent runs in. And you want to define that requirement once, declaratively, without writing stateful logic yourself.

steplock solves this. polyhook is the transport layer that normalizes hook events across AI tools (Claude Code, Cursor, Windsurf, Cline, Amp, and others) — steplock sits on top and adds the stateful gate logic.

---

## How It Works

```
agent tries action (e.g. git push)
        │
steplock reads state file
        │
   unchecked items?
   ┌────┴────┐
  yes        no
   │          │
block        approve
agent with   action
next item
   │
agent acknowledges
   │
state file updated
   │
agent retries → next item shown
   │
... until all items checked
   │
action approved
```

Each checklist item requires one full hook invocation to acknowledge. The agent cannot skip ahead.

---

## Configuration

Each checklist lives in its own directory under `.steplock/checklists/<name>/`. The directory name is the checklist identifier. Two files per checklist:

```
.steplock/
└── checklists/
    └── git-push-quality-gate/
        ├── config.toml
        └── flow.mmd
```

**`config.toml`** — trigger and behaviour:

```toml
#:schema https://raw.githubusercontent.com/polyhook/steplock/main/checklist-config.schema.json

on_event              = "tool:before"
on_tool               = "bash"
match_input           = "input.command.contains('git push')"
reset                 = "session"
allow_preview_request = true
```

**`flow.mmd`** — the [Mermaid `stateDiagram-v2`](https://mermaid.js.org/syntax/stateDiagram.html) diagram. State labels are the questions shown to the agent. steplock walks the graph — on each block it shows the label of the current state; on ack it advances to the next state. `[*]` marks start and end.

```
stateDiagram-v2
    [*] --> clean_code
    clean_code --> test_coverage
    test_coverage --> documentation
    documentation --> no_secrets
    no_secrets --> [*]

    clean_code   : Did you write clean, readable code?
    test_coverage: Did you increase test coverage by at least a little?
    documentation: Did you update relevant documentation?
    no_secrets   : Did you check for hardcoded secrets or credentials?
```

Branching — multiple transitions from one state. steplock prompts the agent with options:

```
stateDiagram-v2
    [*] --> clean_code
    clean_code --> test_coverage
    clean_code --> skip_reason

    test_coverage --> [*]
    skip_reason   --> [*]

    clean_code   : Did you write clean, readable code?
    test_coverage: Did you increase test coverage?
    skip_reason  : Describe why test coverage was skipped.
```

### Editor support

**`config.toml`** — add the `#:schema` hint. [Taplo](https://taplo.tamasfe.dev) (via [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml)) provides autocomplete and inline validation.

**`flow.mmd`** — `.mmd` extension gives Mermaid syntax highlighting and diagram preview in VS Code via the [Mermaid Preview](https://marketplace.visualstudio.com/items?itemName=bierner.markdown-mermaid) extension.

```sh
code --install-extension tamasfe.even-better-toml
code --install-extension bierner.markdown-mermaid
```

### `config.toml` fields

See [`schemas/checklist-config.schema.json`](schemas/checklist-config.schema.json) — source of truth for all fields, types, and allowed values.

### `reset` values

| Value | Behavior |
|---|---|
| `session` | Resets when `sessionId` changes. Default. |
| `always` | Resets on every new invocation of the trigger. |

### `match_input` expressions

`match_input` is a [CEL (Common Expression Language)](https://cel.dev) expression evaluated against the incoming event. Omit it to match all invocations of `on_event` + `on_tool`.

Evaluated by [`cel-interpreter`](https://crates.io/crates/cel-interpreter) — full CEL spec applies.

#### Variables

| Variable | Resolves to |
|---|---|
| `input.<key>` | Field from `HookEvent.input` — e.g. `input.command`, `input.path` |
| `output.<key>` | Field from `HookEvent.output` (for `tool:after` events) |
| `event.tool` | Normalized tool name |
| `event.event` | Event type |
| `event.caller` | Caller kind — `"claude-code"`, `"cursor"`, etc. |

#### Examples

```toml
# push that is not a dry run
match_input = "input.command.contains('git push') && !input.command.contains('--dry-run')"

# push or tag (regex)
match_input = "input.command.matches('^git (push|tag)')"

# path touches /etc
match_input = "input.path.startsWith('/etc')"

# any caller except claude-code
match_input = "event.caller != 'claude-code'"

# destructive command from any caller
match_input = "(input.command.contains('rm') || input.command.contains('drop')) && event.caller != 'claude-code'"

# set membership
match_input = "event.caller in ['cursor', 'windsurf']"
```

#### Absent fields

CEL treats missing map keys as `null`. A condition on a missing field (e.g. `input.path` when the event has no `path`) evaluates to `false` without error — the checklist does not activate.

---

## Multi-agent Safety

Multiple agents can work on the same repo simultaneously. Two problems must be solved:

**1. Script isolation** — each scope gets its own directory under `.steplock/sessions/`. The dir name is the scope key — session ID for `reset = "session"`, branch name for `reset = "branch"`. `ack.sh` and `preview.sh` live there and read `state.json` from the same dir — no params needed from the agent.

**2. State races** — `ack.sh` writes `state.json` atomically via temp + `mv`. On POSIX `mv` is atomic same-filesystem — a concurrent reader always sees a complete file.

```
.steplock/
├── checklists/
│   └── git-push-quality-gate/
│       ├── config.toml
│       └── flow.mmd
├── sessions/
│   ├── session-abc123/
│   │   ├── ack.sh
│   │   ├── preview.sh
│   │   └── state.json
│   └── session-def456/
│       ├── ack.sh
│       ├── preview.sh
│       └── state.json
└── audit.log
```

Commit `.steplock/checklists/` — these are your checklist definitions. Gitignore everything else.

---

## Sessions

### Source

`HookEvent.sessionId` — polyhook inherits it from the AI tool (Claude Code, Cursor, etc.). steplock never generates session IDs for known callers.

**Fallback for unknown callers** — if `sessionId` is empty, steplock generates a UUID on first invocation and writes it to `.steplock/session`. Subsequent hook calls in the same agent lineage read it back.

### Lifecycle

**Init** — lazy. First hook invocation for an unseen scope key creates the dir and initializes `state.json` with the start state.

**Cleanup** — polyhook emits `session:stop`. steplock removes `.steplock/sessions/<session-id>/`.

---

## State

Each scope dir contains one `state.json`. It is the single source of truth — read by steplock on each hook invocation, written by `ack.sh` after each acknowledgment.

```json
{
  "checklist":     "git-push-quality-gate",
  "current_state": "clean_code",
  "next_state":    "test_coverage",
  "transitions":   ["test_coverage"],
  "visited":       []
}
```

See [`schemas/session-state.schema.json`](schemas/session-state.schema.json) — source of truth for all fields and types.

steplock writes `current_state`, `next_state`, `transitions` each block. `ack.sh` writes `visited` and advances `current_state`.

---

## Agent Interaction

When a checklist item is pending, the hook:

1. Updates `state.json` in the scope dir with current node, next node, valid transitions
2. Ensures `ack.sh` and `preview.sh` exist in the scope dir (written once, never regenerated)
3. Returns a `block` response with the item text and a clean `sh` command

**Linear state** (single outgoing transition) — steplock advances automatically. Block message:

```
When finished, run: sh .steplock/sessions/session-abc123/ack.sh
Then retry your original command.
```

**Branching state** (multiple outgoing transitions) — steplock prompts with available next states. Agent passes chosen state as `$1`. Block message:

```
Did you increase test coverage by at least a little?

When finished, run one of:
  sh .steplock/sessions/session-abc123/ack.sh test_coverage   — Yes, coverage increased
  sh .steplock/sessions/session-abc123/ack.sh skip_reason     — No, provide reason below
Then retry your original command.
```

### `allow_preview_request = false` (default)

Items revealed one at a time.

```
agent: git push origin main
hook:  BLOCKED — Did you write clean, readable code?
                 When finished, run: sh .steplock/sessions/session-abc123/ack.sh
                 Then retry your original command.

agent: [reviews code]
agent: sh .steplock/sessions/session-abc123/ack.sh
agent: git push origin main
hook:  BLOCKED — Did you increase test coverage by at least a little?
                 When finished, run: sh .steplock/sessions/session-abc123/ack.sh
                 Then retry your original command.

agent: [checks coverage]
agent: sh .steplock/sessions/session-abc123/ack.sh
agent: git push origin main
hook:  APPROVED
```

### `allow_preview_request = true`

First block includes a hint. Agent runs `preview.sh` from the same session dir — no args needed.

```
agent: git push origin main
hook:  BLOCKED — Did you write clean, readable code?
                 When finished, run: sh .steplock/sessions/session-abc123/ack.sh
                 Then retry your original command.
                 (Tip: run sh .steplock/sessions/session-abc123/preview.sh to see all items first.)

agent: sh .steplock/sessions/session-abc123/preview.sh
stdout: Checklist: git-push-quality-gate (4 items)
          [ ] Did you write clean, readable code?
          [ ] Did you increase test coverage by at least a little?
          [ ] Did you update relevant documentation?
          [ ] Did you check for hardcoded secrets or credentials?

agent: [addresses all items]
agent: sh .steplock/sessions/session-abc123/ack.sh
agent: git push origin main
hook:  BLOCKED — Did you increase test coverage by at least a little?
                 ...
```

Use `allow_preview_request = true` when checklist items are interdependent and the agent benefits from seeing the full scope before starting.

---

## Schema

`checklist-config.schema.json` is the source of truth for `config.toml` shape.

```
checklist-config.schema.json
    │
    └── typify → src/config_types.rs   (Rust structs, generated at build time)
```

**Compile-time** — generated Rust structs catch unknown fields, wrong enum values, wrong types at startup.

**Editor** — schema published to [SchemaStore](https://www.schemastore.org). `#:schema` hint in `config.toml` activates Taplo validation and autocomplete.

Fields with constrained values become JSON Schema enums:

- `on_event` — `"tool:before" | "tool:after" | "session:start" | "session:stop" | "agent:stop" | "notification"`
- `on_tool` — all normalized polyhook tool names (sourced from polyhook's `tools.toml`)
- `reset` — `"session" | "always"`

`match_input` stays `string` — CEL can't be expressed as JSON Schema; parse errors surface at startup. `flow.mmd` is validated by steplock's Mermaid parser at startup.

---

## Architecture

steplock is a separate project that wraps the polyhook SDK. It does not modify polyhook's `core` or WASM module.

Single entry point — the hook binary invoked by the AI tool:

```sh
# your hook script — invoked by the AI tool on each event
steplock
```

Flow:

```
steplock
    │
    ├── scans .steplock/checklists/*/config.toml + flow.mmd
    ├── calls polyhook.read()              ← normalized HookEvent
    ├── evaluates match_input CEL expression (per checklist)
    ├── scope dir exists? → create .steplock/sessions/<scope-key>/ if not
    ├── write .steplock/sessions/<scope-key>/state.json     (current state — updated each block)
    ├── write .steplock/sessions/<scope-key>/ack.sh         (generic, only if not exists)
    ├── write .steplock/sessions/<scope-key>/preview.sh     (generic, only if not exists + allow_preview_request)
    ├── pending items?
    │     yes → calls polyhook.respond(block, message + sh .steplock/sessions/<scope-key>/ack.sh)
    └── no pending items?
          → calls polyhook.respond(approve)
```

`.steplock/sessions/<scope-key>/state.json` — single source of truth, updated by steplock on each block and by `ack.sh` on each ack. See [`schemas/session-state.schema.json`](schemas/session-state.schema.json).

`.steplock/sessions/<scope-key>/ack.sh` — generic, written once per session:

```sh
#!/bin/sh
DIR="$(cd "$(dirname "$0")" && pwd)"
STATE="$DIR/state.json"
TMP="$STATE.tmp.$$"

CURRENT=$(jq -r '.current_state'      "$STATE")
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
```

Requires `jq`. Atomic write via temp + `mv` (PID-suffixed temp name avoids collision between concurrent agents).

Or embed as a library and call `steplock::run()` before your own logic.

---

## Language Support

Ships as a standalone CLI binary (works with any language) and as a native library for each polyhook SDK language.

| Distribution | Usage |
|---|---|
| CLI binary | Drop-in hook script, zero code required |
| Rust crate | `steplock` — embed in existing hook binary |
| TypeScript | `@steplock/sdk` — wrap your hook handler |
| Go | `github.com/polyhook/steplock` |
| Python | `steplock` |
| C# | `Steplock` |

---

## Relation to polyhook

| | polyhook | steplock |
|---|---|---|
| Scope | Normalize hook events across tools | Gate actions behind stateful checklists |
| State | Stateless | Stateful (disk-backed) |
| Config | None | `steplock.toml` |
| Layer | Core SDK | Built on top of polyhook |
| WASM | Yes — detection + serde | No — pure host logic |
### Multiple checklists on the same event

When two `[[checklist]]` blocks both match the same event, steplock processes them in declaration order. The first incomplete checklist blocks. Once it reaches `[*]`, the next checklist's first state blocks on the subsequent invocation.

### Idempotent ack

If `ack.sh` runs when the session is already complete or `current_state` is null (e.g. agent ran it twice), it exits 0 with a message and makes no writes:

```
steplock: nothing to acknowledge for 'git-push-quality-gate' (session already complete)
```

---

## Open Questions


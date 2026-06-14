# Architecture

## Multi-agent Safety

Multiple agents can work on the same repo simultaneously. Two problems must be solved:

**1. Script isolation** — each scope gets its own directory under `.steplock/sessions/`. The dir name is the scope key — session ID for `reset = "session"`. `ack.sh` and `preview.sh` live there and read `state.json` from the same dir — no params needed from the agent.

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

**Fallback for unknown callers** — if `sessionId` is empty, steplock generates a UUID on first invocation and writes it to `.steplock/sessions/fallback-id`. Subsequent hook calls in the same agent lineage read it back.

### Scope key by `reset`

| `reset`   | Scope key             | Dir                                | Cleaned up when      |
| --------- | --------------------- | ---------------------------------- | -------------------- |
| `session` | `HookEvent.sessionId` | `.steplock/sessions/<session-id>/` | `session:stop` fires |
| `always`  | —                     | no dir, no file                    | —                    |

`reset = "always"` requires no state at all — every trigger invocation starts fresh.

### Lifecycle

**Init** — lazy. First hook invocation for an unseen scope key creates the dir and initializes `state.json` with the start state.

**Cleanup** — polyhook emits `session:stop`. steplock removes `.steplock/sessions/<session-id>/`.

---

## State

Each scope dir contains one `state.json`. It is the single source of truth — read by steplock on each hook invocation, written by `ack.sh` after each acknowledgment. See [`schemas/session-state.schema.json`](schemas/session-state.schema.json).

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

`.steplock/sessions/<scope-key>/ack.sh` — generic, written once per session. See [`core/scripts/ack.sh`](core/scripts/ack.sh).

Requires `jq`. Atomic write via temp + `mv` (PID-suffixed temp name avoids collision between concurrent agents).

Or embed as a library and call `steplock::run()` before your own logic.

---

## Relation to polyhook

|        | polyhook                           | steplock                                |
| ------ | ---------------------------------- | --------------------------------------- |
| Scope  | Normalize hook events across tools | Gate actions behind stateful checklists |
| State  | Stateless                          | Stateful (disk-backed)                  |
| Config | None                               | `.steplock/checklists/*/config.toml`    |
| Layer  | Core SDK                           | Built on top of polyhook                |
| WASM   | Yes — detection + serde            | No — pure host logic                    |

---

## Decisions

### Checklist output

steplock writes to two channels that don't touch the hook stdin/stdout protocol:

- **Stderr** — one progress line per event (block issued, ack received, approved). Visible to the human watching the terminal in real time.
- **`.steplock/audit.log`** — append-only JSONL, one event per line. Useful in CI where no human watches stderr.

```jsonl
{"event":"block","checklist":"git-push-quality-gate","state":"clean_code","session":"session-abc123","ts":"2026-06-05T10:00:00Z"}
{"event":"ack","checklist":"git-push-quality-gate","state":"clean_code","session":"session-abc123","ts":"2026-06-05T10:01:12Z"}
{"event":"approved","checklist":"git-push-quality-gate","session":"session-abc123","ts":"2026-06-05T10:03:45Z"}
```

### Multiple checklists on the same event

When two `[[checklist]]` blocks both match the same event, steplock processes them in declaration order. The first incomplete checklist blocks. Once it reaches `[*]`, the next checklist's first state blocks on the subsequent invocation.

### Idempotent ack

If `ack.sh` runs when the session is already complete or `current_state` is null (e.g. agent ran it twice), it exits 0 with a message and makes no writes:

```
steplock: nothing to acknowledge for 'git-push-quality-gate' (session already complete)
```

---

## Behaviour Notes

**Flow changes mid-session** — if `flow.mmd` is updated while an agent has an active session, `current_state` may no longer exist in the new diagram. steplock detects this via empty transitions (the current state has no outgoing edges in the updated graph) and skips the checklist silently rather than erroring. The session remains in the old state file; on the next invocation the updated flow applies.

**Preview for branching flows** — `preview.sh` prints a flat ordered list of checklist items. For branching flows, the list reflects the topological order of all states; branch alternatives appear as siblings without hierarchy markers. Items already visited are shown as `[x]`.

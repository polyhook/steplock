# Architecture

## Multi-agent Safety

Multiple agents can work on the same repo simultaneously. Two problems must be solved:

**1. Script isolation** — each scope gets its own directory under `.steplock/sessions/`. The dir name is the scope key — session ID for `reset = "session"`. `state.json` lives there; `steplock step done` takes the session id from the block message.

**2. State races** — steplock writes `state.json` atomically via temp + `mv`. On POSIX `mv` is atomic same-filesystem — a concurrent reader always sees a complete file.

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

Each scope dir contains one `state.json`. It is the single source of truth — read by steplock on each hook invocation, written by `steplock step done` after each recorded step. See [`schemas/session-state.schema.json`](schemas/session-state.schema.json).

steplock writes `current_state`, `next_state`, `transitions` each block. `steplock step done` writes `visited` and advances `current_state`.

---

## Agent Interaction

When a checklist item is pending, the hook:

1. Updates `state.json` in the scope dir with current node, next node, valid transitions
2. Returns a `block` response: the item text as a checklist step, and the `steplock step done` command that records the result

The agent never runs a script that approves its own gate. It does the step, then records what it did and saw with `steplock step done`. steplock checks that the named step is the current one, that the outcome is valid, and that the evidence is not empty. It writes the evidence to `audit.log` as a `step_done` event. A step cannot be skipped: naming any other step fails and changes nothing.

`steplock step done` finds the session in the project `.steplock/` (found from the current directory) first, then in the global directory. Gates never block a plain `steplock step ...` command, so evidence that quotes a gated command (for example `git push`) does not block the record. A command with shell control or substitution characters (`;&|<>$`, backtick, newline) is gated as usual.

**Linear state** (single outgoing transition) — block message:

```
[git-push-quality-gate: 1/4] Did you write clean, readable code?

Checklist step 1 of 4. Do what it asks, then record the result:
  steplock step done git-push-quality-gate clean_code --session session-abc123 --evidence "<what you did and what you saw>"
Then retry your original command.
```

**Branching state** (multiple outgoing transitions) — the agent picks the outcome with `--next`:

```
Checklist step 2 of 4. Do what it asks, then record the result:
Pick the outcome that applies:
  steplock step done git-push-quality-gate coverage --session session-abc123 --next test_coverage --evidence "..."   — Yes, coverage increased
  steplock step done git-push-quality-gate coverage --session session-abc123 --next skip_reason --evidence "..."     — No, provide reason below
Then retry your original command.
```

### `allow_preview_request = false` (default)

Items revealed one at a time.

```
agent: git push origin main
hook:  BLOCKED — [git-push-quality-gate: 1/4] Did you write clean, readable code?
                 ... steplock step done git-push-quality-gate clean_code --session session-abc123 --evidence "..."

agent: [reviews code]
agent: steplock step done git-push-quality-gate clean_code --session session-abc123 --evidence "read the diff; renamed two helpers"
agent: git push origin main
hook:  BLOCKED — [git-push-quality-gate: 2/4] Did you increase test coverage by at least a little?
...
agent: git push origin main
hook:  APPROVED
```

### `allow_preview_request = true`

The first block also lists every step of the checklist, so the agent sees the full scope before it starts:

```
All steps in this checklist:
  1. Did you write clean, readable code?
  2. Did you increase test coverage by at least a little?
  3. Did you update relevant documentation?
  4. Did you check for hardcoded secrets or credentials?
```

### Deprecated: `ack.sh` and `preview.sh`

steplock 0.2 and earlier told the agent to run `sh .steplock/sessions/<session>/<checklist>/ack.sh`. Agent safety classifiers (for example Claude Code auto mode) read that as the agent approving its own gate and refuse it. steplock still writes `ack.sh` and `preview.sh` to each session dir, so old sessions and scripts keep working, but block messages no longer mention them. `ack.sh` needs `jq` and records no evidence. Use `steplock step done` instead; `ack.sh` will be removed in a future release.

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
    ├── write .steplock/sessions/<scope-key>/ack.sh         (deprecated, kept for old sessions)
    ├── write .steplock/sessions/<scope-key>/preview.sh     (deprecated, only if allow_preview_request)
    ├── pending items?
    │     yes → calls polyhook.respond(block, step text + `steplock step done ...` command)
    └── no pending items?
          → calls polyhook.respond(approve)
```

`.steplock/sessions/<scope-key>/state.json` — single source of truth, updated by steplock on each block and by `steplock step done` on each recorded step. See [`schemas/session-state.schema.json`](schemas/session-state.schema.json).

`.steplock/sessions/<scope-key>/ack.sh` — deprecated, see [Deprecated: `ack.sh` and `preview.sh`](#deprecated-acksh-and-previewsh). Requires `jq`.

Atomic write via temp + `mv` (PID-suffixed temp name avoids collision between concurrent agents).

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

- **Stderr** — one progress line per event (block issued, approved). Visible to the human watching the terminal in real time.
- **`.steplock/audit.log`** — append-only JSONL, one event per line. Useful in CI where no human watches stderr.

```jsonl
{"event":"block","checklist":"git-push-quality-gate","state":"clean_code","session":"session-abc123","ts":"2026-06-05T10:00:00Z"}
{"event":"step_done","checklist":"git-push-quality-gate","state":"clean_code","session":"session-abc123","evidence":"read the diff; renamed two helpers","ts":"2026-06-05T10:01:12Z"}
{"event":"approved","checklist":"git-push-quality-gate","session":"session-abc123","ts":"2026-06-05T10:03:45Z"}
```

### Multiple checklists on the same event

When two checklist directories both match the same event, steplock processes them in alphabetical order (sorted by directory name). The first incomplete checklist blocks. Once it reaches `[*]`, the next checklist's first state blocks on the subsequent invocation.

### Global checklists

steplock evaluates two steplock directories: the project `.steplock/` and the global directory (`$STEPLOCK_GLOBAL_DIR`, else `$XDG_CONFIG_HOME/steplock`, else `~/.config/steplock`). Both use the same layout.

- **Order** — all project checklists first, then global checklists. The first incomplete match blocks.
- **Override** — a global checklist is skipped when the project has a checklist directory with the same name. This lets a project replace a global gate, or turn it off with an empty directory.
- **State** — each checklist stores sessions and audit events in the directory it came from. Global session state lives in the global directory, so no files are written into projects that have no `.steplock/`.
- **Same directory** — when the global directory resolves to the project `.steplock/` (for example, `STEPLOCK_GLOBAL_DIR=~/.steplock` and the project is `~`), it is evaluated once.
- **`session:stop`** — cleans the session directory in both locations.

### Recording a step twice

`steplock step done` on a complete checklist exits 0 and makes no writes. Naming a step that is not the current one (for example, recording the same step twice) fails with the current step's name and makes no writes.

---

## Behaviour Notes

**Flow changes mid-session** — if `flow.mmd` is updated while an agent has an active session, `current_state` may no longer exist in the new diagram. steplock detects this via empty transitions (the current state has no outgoing edges in the updated graph) and skips the checklist silently rather than erroring. The session remains in the old state file; on the next invocation the updated flow applies.

**Preview for branching flows** — the step list (and the deprecated `preview.sh`) is a flat ordered list of checklist items. For branching flows, the list reflects the topological order of all states; branch alternatives appear as siblings without hierarchy markers. `preview.sh` marks items already visited as `[x]`.

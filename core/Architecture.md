# steplock-core Architecture

## Call flow

```
run(event, repo_root)
    │
    ├── scan .steplock/checklists/*/ (alphabetical)
    │
    └── for each checklist_dir:
            │
            ├── parse config.toml     → ChecklistConfig
            ├── check on_event / on_tool match
            ├── eval match_input CEL  → bool
            ├── parse flow.mmd        → FlowGraph
            │
            ├─ reset = "always":
            │       block on initial state, no state file
            │
            └─ reset = "session" | "branch":
                    │
                    ├── get_scope_key → session-id or branch name
                    ├── create .steplock/sessions/<scope-key>/ if absent
                    ├── load state.json (or init from flow.initial)
                    │
                    ├── is_complete? → continue to next checklist
                    │
                    ├── resolve raw_transitions for current_state
                    ├── empty transitions? → skip (flow changed, stale state)
                    │
                    ├── save_state (atomic)
                    ├── ensure_ack_sh
                    ├── ensure_preview_sh (if allow_preview_request)
                    │
                    └── return Block { message }
    │
    └── (all checklists pass) → return Approve
```

## FlowGraph

Built from `stateDiagram-v2` Mermaid syntax by `flow::parse_mmd`. Only two constructs are parsed:

- **Transitions**: `X --> Y` (including `[*] --> X` and `X --> [*]`)
- **Labels**: `X : Label text`

`[*]` is a pseudo-node — it marks the start (initial transitions) and end (terminal transitions). It is never stored as a real state. `FlowGraph.order` is a BFS topological order of all real states, used by `preview.sh` generation.

## Scope key resolution

`get_scope_key` is called for `reset = "session"` and `reset = "branch"`:

- **session**: uses `event.session_id` if non-empty; otherwise reads or generates a UUID in `.steplock/sessions/fallback-id`.
- **branch**: reads `.git/HEAD` walking up from the repo root. Falls back to `"unknown-branch"` if no `.git` found.

## Session state file

`.steplock/sessions/<scope-key>/state.json` is the single source of truth. steplock-core writes it (via `save_state`) and `ack.sh` writes it (via `jq + mv`). Both use atomic writes to avoid corruption under concurrent agent access.

Fields written by `save_state`:

| Field           | Written by    | Description |
| --------------- | ------------- | ----------- |
| `checklist`     | init          | Checklist identifier |
| `current_state` | each block    | Node the agent is currently gated on |
| `next_state`    | each block    | Auto-advance target (`null` for branching states) |
| `transitions`   | each block    | Valid next states for `ack.sh` validation |
| `visited`       | `ack.sh`      | Nodes already acknowledged |

## Script generation

`ack.sh` and `preview.sh` are written once per session directory and never regenerated. `ack.sh` is a static template (embedded at compile time via `include_str!`). `preview.sh` is generated from the `FlowGraph` — one `check` call per state in topological order.

## Audit log

`.steplock/audit.log` is append-only JSONL. `audit::append` is called on each block event. Failures are silently ignored — audit logging never blocks the hook.

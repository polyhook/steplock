# steplock

Rust library implementing the steplock gate logic. Used by the `steplock` CLI binary and embeddable in custom hook binaries.

## Modules

| Module       | Responsibility |
| ------------ | -------------- |
| `run`        | Entry point — evaluates all checklists and returns `HookResponse` |
| `config`     | Parses `config.toml` (`ChecklistConfig`) |
| `flow`       | Parses `flow.mmd` into a `FlowGraph` (Mermaid `stateDiagram-v2`) |
| `state`      | `SessionState` — read/write `state.json` atomically |
| `cel_eval`   | Evaluates `match_input` CEL expressions against a `HookEvent` |
| `scripts`    | Writes `ack.sh` and `preview.sh` to the session directory |
| `audit`      | Appends JSONL events to `.steplock/audit.log` |
| `error`      | `SteplockError` enum and `Result<T>` alias |
| `validate`   | `validate_checklists` — checks all `config.toml`/`flow.mmd` pairs under a checklists dir, used by `steplock validate` |

## Entry point

```rust
use steplock::{run, HookEvent, HookResponse};

let event = HookEvent { /* ... */ };
match run(&event, repo_root)? {
    HookResponse::Block { message } => { /* return block to hook protocol */ }
    HookResponse::Approve => { /* allow the action */ }
}
```

`run` scans `.steplock/checklists/*/` in alphabetical order. The first incomplete checklist that matches the event returns `Block`. If none match or all are complete, returns `Approve`.

## `HookEvent`

All fields come from the polyhook layer — steplock does not read from stdin itself.

| Field        | Type                          | Description |
| ------------ | ----------------------------- | ----------- |
| `event`      | `String`                      | polyhook event type (`"tool:before"`, etc.) |
| `tool`       | `String`                      | Normalized tool name (`"bash"`, `"write_file"`, etc.) |
| `input`      | `HashMap<String, Value>`      | Tool inputs |
| `output`     | `HashMap<String, Value>`      | Tool outputs (for `tool:after`) |
| `session_id` | `String`                      | Session ID from the AI tool; empty for unknown callers |
| `caller`     | `String`                      | AI tool identifier (`"claude-code"`, `"cursor"`, etc.) |

## State management

`SessionState` is read and written by steplock on each invocation. `ack.sh` also writes it (via `jq` + atomic `mv`). Both paths are safe for concurrent access — `save_state` uses a PID-suffixed temp file and POSIX `mv` (atomic same-filesystem).

## CEL evaluation

`match_input` expressions are compiled at each invocation (not cached). Three top-level variables are bound: `input`, `output`, `event`. Missing keys evaluate to `null` (CEL semantics) — a condition on a missing field evaluates to `false` without error.

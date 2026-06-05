## Schemas

`checklist-config.schema.json` — shape of `config.toml`.
`session-state.schema.json` — shape of `.steplock/sessions/<scope-key>/state.json`.

Both schemas are read by editor tooling (Taplo, etc.) via the `#:schema` hint — they are not used at runtime.

---

## `config.toml` fields

| Field                  | Type    | Required | Default     | Description |
| ---------------------- | ------- | -------- | ----------- | ----------- |
| `on_event`             | string  | yes      | —           | polyhook event type that triggers this checklist |
| `on_tool`              | string  | yes      | —           | Normalized polyhook tool name to match |
| `match_input`          | string  | no       | (match all) | CEL expression evaluated against the event |
| `reset`                | string  | no       | `"session"` | When to reset checklist progress |
| `allow_preview_request`| boolean | no       | `false`     | Let the agent preview all items before starting |

### `on_event` values

`"tool:before"` `"tool:after"` `"session:start"` `"session:stop"` `"agent:stop"` `"notification"`

### `reset` values

| Value     | Scope key             | State dir                         | Reset when           |
| --------- | --------------------- | --------------------------------- | -------------------- |
| `session` | `HookEvent.sessionId` | `.steplock/sessions/<session-id>/`| `session:stop` fires |
| `branch`  | git branch name       | `.steplock/sessions/<branch>/`    | branch changes       |
| `always`  | —                     | no dir, no file                   | every invocation     |

---

## `match_input` expressions

`match_input` is a [CEL (Common Expression Language)](https://cel.dev) expression evaluated against the incoming event. Omit it to match all invocations of `on_event` + `on_tool`.

Evaluated by [`cel-interpreter`](https://crates.io/crates/cel-interpreter) — full CEL spec applies.

### Variables

| Variable       | Resolves to                                                        |
| -------------- | ------------------------------------------------------------------ |
| `input.<key>`  | Field from `HookEvent.input` — e.g. `input.command`, `input.path` |
| `output.<key>` | Field from `HookEvent.output` (for `tool:after` events)            |
| `event.tool`   | Normalized tool name                                               |
| `event.event`  | Event type                                                         |
| `event.caller` | Caller kind — `"claude-code"`, `"cursor"`, etc.                    |

### Examples

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

### Absent fields

CEL treats missing map keys as `null`. A condition on a missing field (e.g. `input.path` when the event has no `path`) evaluates to `false` without error — the checklist does not activate.

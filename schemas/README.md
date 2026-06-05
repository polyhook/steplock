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

## `config.toml` fields

See [`checklist-config.schema.json`](checklist-config.schema.json) — source of truth for all fields, types, and allowed values.

## `reset` values

| Value     | Behavior                                       |
| --------- | ---------------------------------------------- |
| `session` | Resets when `sessionId` changes. Default.      |
| `always`  | Resets on every new invocation of the trigger. |

## `match_input` expressions

`match_input` is a [CEL (Common Expression Language)](https://cel.dev) expression evaluated against the incoming event. Omit it to match all invocations of `on_event` + `on_tool`.

Evaluated by [`cel-interpreter`](https://crates.io/crates/cel-interpreter) — full CEL spec applies.

### Variables

| Variable       | Resolves to                                                       |
| -------------- | ----------------------------------------------------------------- |
| `input.<key>`  | Field from `HookEvent.input` — e.g. `input.command`, `input.path` |
| `output.<key>` | Field from `HookEvent.output` (for `tool:after` events)           |
| `event.tool`   | Normalized tool name                                              |
| `event.event`  | Event type                                                        |
| `event.caller` | Caller kind — `"claude-code"`, `"cursor"`, etc.                   |

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

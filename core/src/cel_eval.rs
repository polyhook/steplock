use std::collections::HashMap;
use std::sync::Arc;

use cel_interpreter::objects::{Key, Map, Value};
use cel_interpreter::{Context, Program};

use crate::error::{Result, SteplockError};
use crate::state::HookEvent;

/// Returns `true` if `expr` evaluates to a truthy value against `event`.
/// Returns `true` when `expr` is `None` (no filter = match all).
///
/// # Errors
///
/// Returns [`crate::SteplockError::Cel`] if the CEL expression fails to compile or evaluate.
pub fn matches_event(event: &HookEvent, expr: &Option<String>) -> Result<bool> {
    let expr = match expr {
        None => return Ok(true),
        Some(e) if e.trim().is_empty() => return Ok(true),
        Some(e) => e,
    };

    let program = Program::compile(expr).map_err(|e| SteplockError::Cel {
        expr: expr.clone(),
        message: e.to_string(),
    })?;

    let mut ctx = Context::default();

    ctx.add_variable(
        "event",
        make_map([
            ("tool", cel_str(&event.tool)),
            ("event", cel_str(&event.event)),
            ("caller", cel_str(&event.caller)),
        ]),
    )
    .unwrap();

    ctx.add_variable("input", input_with_words(&event.input))
        .unwrap();
    ctx.add_variable("output", json_obj_to_cel(&event.output))
        .unwrap();

    let result = program.execute(&ctx).map_err(|e| SteplockError::Cel {
        expr: expr.clone(),
        message: e.to_string(),
    })?;

    Ok(is_truthy(&result))
}

/// Builds the `input` CEL map from event input, adding `command_words` when `command` is present.
///
/// `command_words` is a list of whitespace-split tokens, allowing expressions like
/// `input.command_words.exists(x, x == 'push')` to match subcommands without false positives
/// from file paths or commit messages.
fn input_with_words(obj: &HashMap<String, serde_json::Value>) -> Value {
    let mut map: HashMap<Key, Value> = obj
        .iter()
        .map(|(k, v)| (Key::String(Arc::new(k.clone())), json_to_cel(v)))
        .collect();

    if let Some(serde_json::Value::String(cmd)) = obj.get("command") {
        let words: Vec<Value> = cmd.split_whitespace().map(cel_str).collect();
        map.insert(
            Key::String(Arc::new("command_words".to_owned())),
            Value::List(Arc::new(words)),
        );
    }

    Value::Map(Map { map: Arc::new(map) })
}

fn cel_str(s: &str) -> Value {
    Value::String(Arc::new(s.to_owned()))
}

fn make_map<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    let map: HashMap<Key, Value> = pairs
        .into_iter()
        .map(|(k, v)| (Key::String(Arc::new(k.to_owned())), v))
        .collect();
    Value::Map(Map { map: Arc::new(map) })
}

fn json_obj_to_cel(obj: &HashMap<String, serde_json::Value>) -> Value {
    let map: HashMap<Key, Value> = obj
        .iter()
        .map(|(k, v)| (Key::String(Arc::new(k.clone())), json_to_cel(v)))
        .collect();
    Value::Map(Map { map: Arc::new(map) })
}

fn json_to_cel(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => cel_str(s),
        serde_json::Value::Array(arr) => {
            Value::List(Arc::new(arr.iter().map(json_to_cel).collect()))
        }
        serde_json::Value::Object(obj) => {
            let map: HashMap<Key, Value> = obj
                .iter()
                .map(|(k, v)| (Key::String(Arc::new(k.clone())), json_to_cel(v)))
                .collect();
            Value::Map(Map { map: Arc::new(map) })
        }
    }
}

const fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event_with_cmd(cmd: &str) -> HookEvent {
        let mut input = HashMap::new();
        input.insert("command".to_owned(), json!(cmd));
        HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: "s1".to_owned(),
            caller: "claude-code".to_owned(),
        }
    }

    fn event_with_path(path: &str) -> HookEvent {
        let mut input = HashMap::new();
        input.insert("path".to_owned(), json!(path));
        HookEvent {
            event: "tool:before".to_owned(),
            tool: "write_file".to_owned(),
            input,
            output: HashMap::new(),
            session_id: "s1".to_owned(),
            caller: "cursor".to_owned(),
        }
    }

    #[test]
    fn none_expr_matches_all() {
        let ev = event_with_cmd("ls");
        assert!(matches_event(&ev, &None).unwrap());
    }

    #[test]
    fn empty_expr_matches_all() {
        let ev = event_with_cmd("ls");
        assert!(matches_event(&ev, &Some("  ".to_owned())).unwrap());
    }

    #[test]
    fn expr_true_for_matching_command() {
        let ev = event_with_cmd("git push origin main");
        assert!(
            matches_event(&ev, &Some("input.command.contains('git push')".to_owned())).unwrap()
        );
    }

    #[test]
    fn expr_false_for_non_matching_command() {
        let ev = event_with_cmd("ls -la");
        assert!(
            !matches_event(&ev, &Some("input.command.contains('git push')".to_owned())).unwrap()
        );
    }

    #[test]
    fn expr_uses_event_tool() {
        let ev = event_with_cmd("anything");
        assert!(matches_event(&ev, &Some("event.tool == 'bash'".to_owned())).unwrap());
    }

    #[test]
    fn expr_uses_event_caller() {
        let ev = event_with_path("/etc/hosts");
        assert!(matches_event(&ev, &Some("event.caller == 'cursor'".to_owned())).unwrap());
    }

    #[test]
    fn expr_uses_event_event_field() {
        let ev = event_with_cmd("ls");
        assert!(matches_event(&ev, &Some("event.event == 'tool:before'".to_owned())).unwrap());
    }

    #[test]
    fn expr_path_starts_with() {
        let ev = event_with_path("/etc/hosts");
        assert!(matches_event(&ev, &Some("input.path.startsWith('/etc')".to_owned())).unwrap());
    }

    #[test]
    fn invalid_cel_returns_error() {
        let ev = event_with_cmd("ls");
        let err = matches_event(&ev, &Some("!!!invalid".to_owned()));
        assert!(err.is_err());
    }

    #[test]
    fn json_null_input_converts() {
        let mut input = HashMap::new();
        input.insert("val".to_owned(), serde_json::Value::Null);
        let ev = HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: "s".to_owned(),
            caller: "test".to_owned(),
        };
        // null is falsy — expr checking tool name should still work
        assert!(matches_event(&ev, &Some("event.tool == 'bash'".to_owned())).unwrap());
    }

    #[test]
    fn json_bool_input_converts() {
        let mut input = HashMap::new();
        input.insert("flag".to_owned(), json!(true));
        let ev = HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: "s".to_owned(),
            caller: "test".to_owned(),
        };
        assert!(matches_event(&ev, &Some("input.flag".to_owned())).unwrap());
    }

    #[test]
    fn json_float_input_converts() {
        let mut input = HashMap::new();
        input.insert("val".to_owned(), json!(1.23f64));
        let ev = HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: "s".to_owned(),
            caller: "test".to_owned(),
        };
        assert!(matches_event(&ev, &Some("event.tool == 'bash'".to_owned())).unwrap());
    }

    #[test]
    fn json_array_input_converts() {
        let mut input = HashMap::new();
        input.insert("items".to_owned(), json!(["a", "b"]));
        let ev = HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: "s".to_owned(),
            caller: "test".to_owned(),
        };
        assert!(matches_event(&ev, &Some("event.tool == 'bash'".to_owned())).unwrap());
    }

    #[test]
    fn json_nested_object_converts() {
        let mut input = HashMap::new();
        input.insert("meta".to_owned(), json!({"key": "value"}));
        let ev = HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: "s".to_owned(),
            caller: "test".to_owned(),
        };
        assert!(matches_event(&ev, &Some("event.tool == 'bash'".to_owned())).unwrap());
    }

    #[test]
    fn output_map_is_accessible() {
        let mut output = HashMap::new();
        output.insert("exit_code".to_owned(), json!(0));
        let ev = HookEvent {
            event: "tool:after".to_owned(),
            tool: "bash".to_owned(),
            input: HashMap::new(),
            output,
            session_id: "s".to_owned(),
            caller: "test".to_owned(),
        };
        assert!(matches_event(&ev, &Some("event.tool == 'bash'".to_owned())).unwrap());
    }

    #[test]
    fn non_bool_truthy_result_returns_true() {
        // CEL expression returning a string (non-bool, non-null) → truthy
        let ev = event_with_cmd("ls");
        assert!(matches_event(&ev, &Some("event.tool".to_owned())).unwrap());
    }

    #[test]
    fn null_result_returns_false() {
        // CEL `null` literal returns Value::Null → falsy
        let ev = event_with_cmd("ls");
        assert!(!matches_event(&ev, &Some("null".to_owned())).unwrap());
    }

    #[test]
    fn command_words_matches_push_subcommand() {
        let ev = event_with_cmd("git -C /some/path push origin main");
        assert!(matches_event(
            &ev,
            &Some("input.command_words.exists(x, x == 'push')".to_owned())
        )
        .unwrap());
    }

    #[test]
    fn command_words_does_not_match_path_containing_push() {
        let ev = event_with_cmd("git add .steplock/checklists/pre-push/config.toml");
        assert!(!matches_event(
            &ev,
            &Some("input.command_words.exists(x, x == 'push')".to_owned())
        )
        .unwrap());
    }

    #[test]
    fn command_words_matches_commit_with_dash_c_flag() {
        let ev = event_with_cmd("git -C /repo commit -m \"fix bug\"");
        assert!(matches_event(
            &ev,
            &Some("input.command_words.exists(x, x == 'commit')".to_owned())
        )
        .unwrap());
    }

    #[test]
    fn command_words_does_not_match_path_containing_commit() {
        let ev = event_with_cmd("git add .steplock/checklists/pre-commit/config.toml");
        assert!(!matches_event(
            &ev,
            &Some("input.command_words.exists(x, x == 'commit')".to_owned())
        )
        .unwrap());
    }
}

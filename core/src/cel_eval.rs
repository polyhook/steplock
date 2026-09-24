//! CEL (Common Expression Language) evaluation for `match_input` config fields.
use std::collections::HashMap;
use std::panic;
use std::sync::Arc;

use cel_interpreter::objects::{Key, Map, Value};
use cel_interpreter::{Context, Program};

use crate::error::{Result, SteplockError};
use crate::state::HookEvent;

/// Returns true if `expr` evaluates to a truthy value against `event`.
/// Returns true when `expr` is None (no filter = match all).
///
/// # Errors
///
/// Returns `Err` if `expr` fails to compile or fails to execute as a CEL expression,
/// including if the underlying parser panics on a malformed expression.
pub fn matches_event(event: &HookEvent, expr: &Option<String>) -> Result<bool> {
    let expr = match expr {
        None => return Ok(true),
        Some(e) if e.trim().is_empty() => return Ok(true),
        Some(e) => e,
    };

    let compile_result = panic::catch_unwind(|| Program::compile(expr));
    let program = match compile_result {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            return Err(SteplockError::Cel {
                expr: expr.clone(),
                message: e.to_string(),
            })
        }
        Err(_) => {
            return Err(SteplockError::Cel {
                expr: expr.clone(),
                message: "CEL expression caused an internal parse error".to_owned(),
            })
        }
    };

    let mut ctx = Context::default();

    ctx.add_variable_from_value(
        "event",
        make_map([
            ("tool", cel_str(&event.tool)),
            ("event", cel_str(&event.event)),
            ("caller", cel_str(&event.caller)),
        ]),
    );

    ctx.add_variable_from_value("input", input_with_words(&event.input));
    ctx.add_variable_from_value("output", json_obj_to_cel(&event.output));

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
        serde_json::Value::Number(n) => n
            .as_i64()
            .map_or_else(|| Value::Float(n.as_f64().unwrap_or(0.0)), Value::Int),
        serde_json::Value::String(s) => cel_str(s),
        serde_json::Value::Array(arr) => {
            Value::List(Arc::new(arr.iter().map(json_to_cel).collect()))
        }
        serde_json::Value::Object(obj) => {
            let map: HashMap<Key, Value> = obj
                .iter()
                .map(|(k, val)| (Key::String(Arc::new(k.clone())), json_to_cel(val)))
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
#[allow(clippy::unwrap_used)]
#[path = "cel_eval_tests.rs"]
mod tests;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "cel_eval_proptest_tests.rs"]
mod proptest_tests;

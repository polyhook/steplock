//! `steplock step done`: the agent records one finished checklist step, with evidence.
//!
//! This replaces running `ack.sh`. The agent names the step it finished and says what
//! it did and saw. steplock checks that the step is the current one, advances the
//! checklist, and writes the evidence to the audit log.
use std::path::{Path, PathBuf};

use crate::audit;
use crate::error::{Result, SteplockError};
use crate::state::{load_state, save_state, HookEvent};

/// Path of `state.json` for `checklist` in session `session` under `steplock_dir`.
#[must_use]
pub fn state_path(steplock_dir: &Path, session: &str, checklist: &str) -> PathBuf {
    steplock_dir
        .join("sessions")
        .join(session)
        .join(checklist)
        .join("state.json")
}

/// Record that the agent finished `step` of `checklist` in `session`.
///
/// `next` picks the outcome when the step has more than one. Returns the state the
/// checklist moved to (`"[*]"` when every step is done). Calling it again for a
/// complete checklist returns `"[*]"` and changes nothing.
///
/// # Errors
///
/// Returns `Err` when `evidence` is blank, no session state exists, `step` is not the
/// current step, `next` is missing for a step with more than one outcome or is not a
/// valid outcome, or reading or writing `state.json` fails.
pub fn record_step_done(
    steplock_dir: &Path,
    session: &str,
    checklist: &str,
    step: &str,
    next: Option<&str>,
    evidence: &str,
) -> Result<String> {
    let evidence = evidence.trim();
    if evidence.is_empty() {
        return Err(SteplockError::State(
            "--evidence is empty: say what you did and what you saw".to_owned(),
        ));
    }
    let path = state_path(steplock_dir, session, checklist);
    if !path.exists() {
        return Err(SteplockError::State(format!(
            "no checklist '{checklist}' in session '{session}' at {}",
            path.display()
        )));
    }
    let mut state = load_state(&path)?;
    if state.is_complete() {
        return Ok(state.current_state);
    }
    if state.current_state != step {
        return Err(SteplockError::State(format!(
            "'{step}' is not the current step of '{checklist}'; the current step is '{}'",
            state.current_state
        )));
    }
    let outcomes = state.transitions.join(", ");
    let target = match next {
        Some(n) => n.to_owned(),
        None => state.next_state.clone().ok_or_else(|| {
            SteplockError::State(format!(
                "step '{step}' has more than one outcome; pass --next with one of: {outcomes}"
            ))
        })?,
    };
    if !state.transitions.contains(&target) {
        return Err(SteplockError::State(format!(
            "'{target}' is not an outcome of step '{step}'; valid outcomes: {outcomes}"
        )));
    }

    state.visited.push(step.to_owned());
    state.transitions = state
        .flow_transitions
        .get(&target)
        .cloned()
        .unwrap_or_default();
    state.next_state = state
        .transitions
        .first()
        .cloned()
        .filter(|_| state.transitions.len() == 1);
    state.current_state.clone_from(&target);
    save_state(&path, &state)?;
    audit::append_step_done(steplock_dir, checklist, step, session, evidence);
    Ok(target)
}

/// `true` when `event` is a plain `steplock step ...` shell command.
///
/// Gates never block it: its evidence text may quote a gated command (for example
/// `git push`), and blocking the record command would stop the checklist forever.
/// Only a single plain command qualifies: any shell control or substitution character
/// (`;&|<>$` backtick, newline) means the command can run more than steplock.
#[must_use]
pub fn is_step_command(event: &HookEvent) -> bool {
    let Some(serde_json::Value::String(cmd)) = event.input.get("command") else {
        return false;
    };
    cmd.trim_start().starts_with("steplock step ")
        && !cmd.contains([';', '&', '|', '<', '>', '$', '`', '\n', '\r'])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "step_tests.rs"]
mod tests;

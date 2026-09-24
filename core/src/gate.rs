//! Checklist gate: decides whether one checklist blocks a hook event.
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::audit;
use crate::cel_eval;
use crate::config::{parse_config, Reset};
use crate::error::{Result, SteplockError};
use crate::flow::{parse_mmd, FlowGraph};
use crate::scripts;
use crate::state::{init_state, load_state, save_state, HookEvent, HookResponse, SessionState};

/// Evaluate one checklist. Returns `Some(Block)` when it blocks the event, `None` otherwise.
pub(crate) fn evaluate_checklist(
    event: &HookEvent,
    steplock_dir: &Path,
    checklist_dir: &Path,
) -> Result<Option<HookResponse>> {
    let checklist_name = checklist_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_owned();

    let config_path = checklist_dir.join("config.toml");
    let flow_path = checklist_dir.join("flow.mmd");

    if !config_path.exists() || !flow_path.exists() {
        return Ok(None);
    }

    let config_str = fs::read_to_string(&config_path)?;
    let config = parse_config(config_path.to_str().unwrap_or("config.toml"), &config_str)?;

    if config.on_event != event.event {
        return Ok(None);
    }
    if !config.on_tool.is_empty() && config.on_tool != event.tool {
        return Ok(None);
    }

    if !cel_eval::matches_event(event, &config.match_input)? {
        return Ok(None);
    }

    let flow_str = fs::read_to_string(&flow_path)?;
    let flow = parse_mmd(flow_path.to_str().unwrap_or("flow.mmd"), &flow_str)?;

    let initial_state = flow.initial.first().ok_or_else(|| SteplockError::Mermaid {
        path: flow_path.to_str().unwrap_or("flow.mmd").to_owned(),
        message: "no initial state found".to_owned(),
    })?;

    match config.reset {
        Reset::Always => Ok(Some(block_reset_always(
            steplock_dir,
            &checklist_name,
            initial_state,
            &flow,
        ))),
        Reset::Session => block_reset_session(
            event,
            steplock_dir,
            &checklist_name,
            initial_state,
            &flow,
            config.allow_preview_request,
        ),
    }
}

fn block_reset_always(
    steplock_dir: &Path,
    checklist_name: &str,
    initial_state: &str,
    flow: &FlowGraph,
) -> HookResponse {
    let transitions: Vec<String> = flow
        .transitions
        .get(initial_state)
        .cloned()
        .unwrap_or_default();
    let next_state = transitions
        .first()
        .cloned()
        .filter(|_| transitions.len() == 1);
    let state = SessionState {
        checklist: checklist_name.to_owned(),
        current_state: initial_state.to_owned(),
        next_state,
        transitions,
        visited: vec![],
    };
    audit::append(
        steplock_dir,
        "block",
        checklist_name,
        initial_state,
        "always",
    );
    let message = build_block_message(&state, flow, None);
    eprintln!("steplock: block [{checklist_name}] state={initial_state}");
    HookResponse::Block { message }
}

fn block_reset_session(
    event: &HookEvent,
    steplock_dir: &Path,
    checklist_name: &str,
    initial_state: &str,
    flow: &FlowGraph,
    allow_preview: bool,
) -> Result<Option<HookResponse>> {
    let scope_key = get_scope_key(event, steplock_dir)?;
    let session_dir = steplock_dir
        .join("sessions")
        .join(&scope_key)
        .join(checklist_name);
    fs::create_dir_all(&session_dir)?;

    let state_path = session_dir.join("state.json");
    let mut state = if state_path.exists() {
        load_state(&state_path)?
    } else {
        init_state(checklist_name, initial_state)
    };

    // Checklist complete — approve this attempt and reset state so the
    // next invocation starts the checklist fresh.
    if state.is_complete() {
        audit::append(steplock_dir, "complete", checklist_name, "[*]", &scope_key);
        save_state(&state_path, &init_state(checklist_name, initial_state))?;
        return Ok(None);
    }

    // Raw transitions including [*] — stored in state.json for ack.sh validation.
    let raw_transitions: Vec<String> = flow
        .transitions
        .get(&state.current_state)
        .cloned()
        .unwrap_or_default();

    if raw_transitions.is_empty() {
        // State unknown in flow — skip silently (flow changed mid-session).
        return Ok(None);
    }

    // next_state: auto-advance when only one transition (may be "[*]").
    state.next_state = raw_transitions
        .first()
        .cloned()
        .filter(|_| raw_transitions.len() == 1);
    state.transitions = raw_transitions;

    save_state(&state_path, &state)?;
    scripts::ensure_ack_sh(&session_dir)?;
    if allow_preview {
        scripts::ensure_preview_sh(&session_dir, checklist_name, flow)?;
    }

    audit::append(
        steplock_dir,
        "block",
        checklist_name,
        &state.current_state,
        &scope_key,
    );
    eprintln!(
        "steplock: block [{}] state={} session={}",
        checklist_name, state.current_state, scope_key
    );

    let message = build_block_message(&state, flow, Some(&session_dir));
    Ok(Some(HookResponse::Block { message }))
}

fn get_scope_key(event: &HookEvent, steplock_dir: &Path) -> Result<String> {
    if !event.session_id.is_empty() {
        return Ok(event.session_id.clone());
    }
    let fallback_path = steplock_dir.join("sessions").join("fallback-id");
    if fallback_path.exists() {
        let id = fs::read_to_string(&fallback_path)?;
        return Ok(id.trim().to_owned());
    }
    let id = uuid::Uuid::new_v4().to_string();
    fs::create_dir_all(steplock_dir.join("sessions"))?;
    fs::write(&fallback_path, &id)?;
    Ok(id)
}

/// `session_dir` is `None` for `reset=always` checklists (no persistent ack.sh).
fn build_block_message(
    state: &SessionState,
    flow: &FlowGraph,
    session_dir: Option<&Path>,
) -> String {
    let label = flow
        .labels
        .get(&state.current_state)
        .map_or(state.current_state.as_str(), String::as_str);

    let checklist = &state.checklist;
    let step = state.visited.len() + 1;
    let total = flow.order.len();
    let mut msg = format!("[{checklist}: {step}/{total}] {label}");
    msg.push_str("\n\n");

    let visible: Vec<&String> = state
        .transitions
        .iter()
        .filter(|s| s.as_str() != "[*]")
        .collect();

    if let Some(dir) = session_dir {
        let ack = dir.join("ack.sh");
        let ack_path = ack.display();
        if visible.len() <= 1 {
            let _ = write!(
                msg,
                "When finished, run: sh {ack_path}\nThen retry your original command."
            );
        } else {
            msg.push_str("When finished, run one of:\n");
            for next in &visible {
                let next_label = flow.labels.get(*next).map_or(next.as_str(), String::as_str);
                let _ = writeln!(msg, "  sh {ack_path} {next}   — {next_label}");
            }
            msg.push_str("Then retry your original command.");
        }

        if state.visited.is_empty() {
            let preview = dir.join("preview.sh");
            if preview.exists() {
                let _ = write!(
                    msg,
                    "\n(Tip: run sh {} to see all items first.)",
                    preview.display()
                );
            }
        }
    } else {
        // reset=always: no persistent ack.sh — agent confirms in conversation then retries.
        msg.push_str("When done, retry your original command.");
    }

    msg
}

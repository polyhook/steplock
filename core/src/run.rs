use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::audit;
use crate::cel_eval;
use crate::config::{parse_config, Reset};
use crate::error::Result;
use crate::flow::{parse_mmd, FlowGraph};
use crate::scripts;
use crate::state::{init_state, load_state, save_state, HookEvent, HookResponse, SessionState};

/// Run the full steplock gate logic.
///
/// `repo_root` — the directory that contains `.steplock/`.
/// Returns `HookResponse::Approve` if no checklist blocks, or
/// `HookResponse::Block { message }` with the gate message.
///
/// # Errors
///
/// Returns an error if any checklist config, flow file, or session state cannot
/// be read or written, or if a CEL expression fails to evaluate.
pub fn run(event: &HookEvent, repo_root: &Path) -> Result<HookResponse> {
    let steplock_dir = repo_root.join(".steplock");

    if event.event == "session:stop" {
        cleanup_session(&steplock_dir, &event.session_id)?;
        return Ok(HookResponse::Approve);
    }

    let checklists_dir = steplock_dir.join("checklists");

    if !checklists_dir.exists() {
        return Ok(HookResponse::Approve);
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(&checklists_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    entries.sort(); // deterministic declaration order

    for checklist_dir in entries {
        let checklist_name = checklist_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();

        let config_path = checklist_dir.join("config.toml");
        let flow_path = checklist_dir.join("flow.mmd");

        if !config_path.exists() || !flow_path.exists() {
            continue;
        }

        let config_str = fs::read_to_string(&config_path)?;
        let config = parse_config(config_path.to_str().unwrap_or("config.toml"), &config_str)?;

        // Check on_event and on_tool match
        if config.on_event != event.event {
            continue;
        }
        if !config.on_tool.is_empty() && config.on_tool != event.tool {
            continue;
        }

        // Evaluate CEL match_input
        if !cel_eval::matches_event(event, &config.match_input)? {
            continue;
        }

        let flow_str = fs::read_to_string(&flow_path)?;
        let flow = parse_mmd(flow_path.to_str().unwrap_or("flow.mmd"), &flow_str)?;

        let initial_state = flow
            .initial
            .first()
            .expect("parse_mmd guarantees at least one initial state");

        match config.reset {
            Reset::Always => {
                // No state persistence — block on initial state every time
                let transitions: Vec<String> = flow
                    .transitions
                    .get(initial_state)
                    .cloned()
                    .unwrap_or_default();
                let next_state = if transitions.len() == 1 {
                    Some(transitions[0].clone())
                } else {
                    None
                };
                let state = SessionState {
                    checklist: checklist_name.clone(),
                    current_state: initial_state.clone(),
                    next_state,
                    transitions,
                    visited: vec![],
                };
                audit::append(
                    &steplock_dir,
                    "block",
                    &checklist_name,
                    initial_state,
                    "always",
                );
                // No session dir for reset=always — use a placeholder path in message
                let tmp_dir = steplock_dir.join("sessions").join("always");
                let message =
                    build_block_message(&state, &flow, &tmp_dir, config.allow_preview_request);
                eprintln!("steplock: block [{checklist_name}] state={initial_state}");
                return Ok(HookResponse::Block { message });
            }

            Reset::Session => {
                let scope_key = get_scope_key(event, &steplock_dir)?;
                let session_dir = steplock_dir
                    .join("sessions")
                    .join(&scope_key)
                    .join(&checklist_name);
                fs::create_dir_all(&session_dir)?;

                let state_path = session_dir.join("state.json");
                let mut state = if state_path.exists() {
                    load_state(&state_path)?
                } else {
                    init_state(&checklist_name, initial_state)
                };

                // Checklist complete — approve this attempt and reset state so the
                // next invocation starts the checklist fresh.
                if state.is_complete() {
                    save_state(&state_path, &init_state(&checklist_name, initial_state))?;
                    continue;
                }

                // Raw transitions including [*] — stored in state.json for ack.sh validation.
                let raw_transitions: Vec<String> = flow
                    .transitions
                    .get(&state.current_state)
                    .cloned()
                    .unwrap_or_default();

                if raw_transitions.is_empty() {
                    // State unknown in flow — skip silently (flow changed mid-session).
                    continue;
                }

                // next_state: auto-advance when only one transition (may be "[*]").
                state.next_state = if raw_transitions.len() == 1 {
                    Some(raw_transitions[0].clone())
                } else {
                    None
                };
                state.transitions = raw_transitions;

                save_state(&state_path, &state)?;
                scripts::ensure_ack_sh(&session_dir)?;
                if config.allow_preview_request {
                    scripts::ensure_preview_sh(&session_dir, &checklist_name, &flow)?;
                }

                audit::append(
                    &steplock_dir,
                    "block",
                    &checklist_name,
                    &state.current_state,
                    &scope_key,
                );
                eprintln!(
                    "steplock: block [{}] state={} session={}",
                    checklist_name, state.current_state, scope_key
                );

                let message =
                    build_block_message(&state, &flow, &session_dir, config.allow_preview_request);
                return Ok(HookResponse::Block { message });
            }
        }
    }

    Ok(HookResponse::Approve)
}

fn cleanup_session(steplock_dir: &Path, session_id: &str) -> Result<()> {
    if !steplock_dir.exists() {
        return Ok(());
    }
    let scope_key = if session_id.is_empty() {
        let fallback_path = steplock_dir.join("sessions").join("fallback-id");
        if !fallback_path.exists() {
            return Ok(());
        }
        fs::read_to_string(&fallback_path).map(|s| s.trim().to_owned())?
    } else {
        session_id.to_owned()
    };
    let scope_dir = steplock_dir.join("sessions").join(&scope_key);
    if !scope_dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&scope_dir)?;
    eprintln!("steplock: cleaned up session {scope_key}");
    Ok(())
}

fn get_scope_key(event: &HookEvent, steplock_dir: &Path) -> Result<String> {
    if !event.session_id.is_empty() {
        return Ok(event.session_id.clone());
    }
    // Fallback: generate/read a UUID
    let fallback_path = steplock_dir.join("sessions").join("fallback-id");
    if fallback_path.exists() {
        let id = fs::read_to_string(&fallback_path)?;
        return Ok(id.trim().to_owned());
    }
    let id = uuid::Uuid::new_v4().to_string();
    fs::create_dir_all(fallback_path.parent().unwrap())?;
    fs::write(&fallback_path, &id)?;
    Ok(id)
}

fn build_block_message(
    state: &SessionState,
    flow: &FlowGraph,
    session_dir: &Path,
    allow_preview: bool,
) -> String {
    let label = flow
        .labels
        .get(&state.current_state)
        .map_or(state.current_state.as_str(), |s| s.as_str());

    let ack = session_dir.join("ack.sh");
    let ack_path = ack.display();

    let mut msg = label.to_owned();
    msg.push_str("\n\n");

    // Real next states visible to the agent (exclude pseudo [*] node).
    let visible: Vec<&String> = state
        .transitions
        .iter()
        .filter(|s| s.as_str() != "[*]")
        .collect();

    if visible.len() <= 1 {
        // Linear — single transition (or terminal → [*])
        write!(
            msg,
            "When finished, run: sh {ack_path}\nThen retry your original command."
        )
        .unwrap();
    } else {
        // Branching — show all real options
        msg.push_str("When finished, run one of:\n");
        for next in &visible {
            let next_label = flow.labels.get(*next).map_or(next.as_str(), String::as_str);
            writeln!(msg, "  sh {ack_path} {next}   — {next_label}").unwrap();
        }
        msg.push_str("Then retry your original command.");
    }

    if allow_preview && state.visited.is_empty() {
        let preview = session_dir.join("preview.sh");
        let preview_path = preview.display();
        write!(
            msg,
            "\n(Tip: run sh {preview_path} to see all items first.)"
        )
        .unwrap();
    }

    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_event(event: &str, tool: &str, cmd: &str, session: &str) -> HookEvent {
        let mut input = HashMap::new();
        input.insert(
            "command".to_owned(),
            serde_json::Value::String(cmd.to_owned()),
        );
        HookEvent {
            event: event.to_owned(),
            tool: tool.to_owned(),
            input,
            output: HashMap::new(),
            session_id: session.to_owned(),
            caller: "claude-code".to_owned(),
        }
    }

    fn setup_checklist(root: &Path) {
        let cl_dir = root.join(".steplock/checklists/quality-gate");
        fs::create_dir_all(&cl_dir).unwrap();

        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "session"
"#,
        )
        .unwrap();

        fs::write(
            cl_dir.join("flow.mmd"),
            r#"stateDiagram-v2
    [*] --> clean_code
    clean_code --> [*]
    clean_code: Did you write clean code?
"#,
        )
        .unwrap();
    }

    #[test]
    fn approves_non_matching_event() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let event = make_event("tool:before", "bash", "ls -la", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn blocks_on_matching_event() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let event = make_event("tool:before", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));
    }

    #[test]
    fn approves_and_resets_state_when_complete() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());

        // State at [*] = checklist complete from a prior ack sequence
        let session_dir = tmp.path().join(".steplock/sessions/sess-1/quality-gate");
        fs::create_dir_all(&session_dir).unwrap();
        let state = SessionState {
            checklist: "quality-gate".to_owned(),
            current_state: "[*]".to_owned(),
            next_state: None,
            transitions: vec![],
            visited: vec!["clean_code".to_owned()],
        };
        save_state(&session_dir.join("state.json"), &state).unwrap();

        // This attempt is approved (checklist was already satisfied)
        let event = make_event("tool:before", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));

        // State is now reset so the NEXT attempt starts fresh
        let next_state = load_state(&session_dir.join("state.json")).unwrap();
        assert_eq!(next_state.current_state, "clean_code");
        assert!(next_state.visited.is_empty());
    }

    #[test]
    fn approves_when_no_checklists_dir() {
        let tmp = TempDir::new().unwrap();
        let event = make_event("tool:before", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn approves_on_event_type_mismatch() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let event = make_event("tool:after", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn approves_on_tool_mismatch() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let event = make_event(
            "tool:before",
            "write_file",
            "git push origin main",
            "sess-1",
        );
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn skips_checklist_dir_missing_files() {
        let tmp = TempDir::new().unwrap();
        // Create dir but no config.toml / flow.mmd
        let cl_dir = tmp.path().join(".steplock/checklists/empty-gate");
        fs::create_dir_all(&cl_dir).unwrap();
        let event = make_event("tool:before", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn reset_always_blocks_every_time() {
        let tmp = TempDir::new().unwrap();
        let cl_dir = tmp.path().join(".steplock/checklists/always-gate");
        fs::create_dir_all(&cl_dir).unwrap();
        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "always"
"#,
        )
        .unwrap();
        fs::write(
            cl_dir.join("flow.mmd"),
            r#"stateDiagram-v2
    [*] --> check
    check --> [*]
    check: Did you check?
"#,
        )
        .unwrap();

        let event = make_event("tool:before", "bash", "git push origin main", "sess-x");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));

        // Second invocation still blocks (no state persistence)
        let resp2 = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp2, HookResponse::Block { .. }));
    }

    #[test]
    fn block_message_contains_label() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let event = make_event("tool:before", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        if let HookResponse::Block { message } = resp {
            assert!(message.contains("Did you write clean code?"));
        } else {
            panic!("expected block");
        }
    }

    #[test]
    fn block_message_contains_ack_sh_path() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let event = make_event("tool:before", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        if let HookResponse::Block { message } = resp {
            assert!(message.contains("ack.sh"));
        } else {
            panic!("expected block");
        }
    }

    #[test]
    fn fallback_session_id_generated_when_empty() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        // Empty session_id triggers fallback UUID generation
        let mut input = HashMap::new();
        input.insert(
            "command".to_owned(),
            serde_json::Value::String("git push".to_owned()),
        );
        let event = HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: String::new(),
            caller: "unknown".to_owned(),
        };
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));
        // fallback-id file created
        assert!(tmp.path().join(".steplock/sessions/fallback-id").exists());

        // Second invocation reuses the same fallback ID
        let resp2 = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp2, HookResponse::Block { .. }));
    }

    #[test]
    fn allow_preview_request_adds_tip() {
        let tmp = TempDir::new().unwrap();
        let cl_dir = tmp.path().join(".steplock/checklists/preview-gate");
        fs::create_dir_all(&cl_dir).unwrap();
        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "session"
allow_preview_request = true
"#,
        )
        .unwrap();
        fs::write(
            cl_dir.join("flow.mmd"),
            r#"stateDiagram-v2
    [*] --> check
    check --> [*]
    check: Did you check?
"#,
        )
        .unwrap();

        let event = make_event(
            "tool:before",
            "bash",
            "git push origin main",
            "sess-preview",
        );
        let resp = run(&event, tmp.path()).unwrap();
        if let HookResponse::Block { message } = resp {
            assert!(message.contains("preview.sh"));
        } else {
            panic!("expected block");
        }
    }

    #[test]
    fn branching_flow_block_message_lists_options() {
        let tmp = TempDir::new().unwrap();
        let cl_dir = tmp.path().join(".steplock/checklists/branch-gate");
        fs::create_dir_all(&cl_dir).unwrap();
        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "session"
"#,
        )
        .unwrap();
        fs::write(
            cl_dir.join("flow.mmd"),
            r#"stateDiagram-v2
    [*] --> check
    check --> pass
    check --> skip
    pass --> [*]
    skip --> [*]
    check: Did you check?
    pass: Yes, it passed
    skip: No, skipped because
"#,
        )
        .unwrap();

        let event = make_event("tool:before", "bash", "git push origin main", "sess-branch");
        let resp = run(&event, tmp.path()).unwrap();
        if let HookResponse::Block { message } = resp {
            assert!(message.contains("pass"));
            assert!(message.contains("skip"));
            assert!(message.contains("run one of:"));
        } else {
            panic!("expected block");
        }
    }

    #[test]
    fn reset_always_with_branching_flow_shows_no_next_state() {
        let tmp = TempDir::new().unwrap();
        let cl_dir = tmp.path().join(".steplock/checklists/always-branch");
        fs::create_dir_all(&cl_dir).unwrap();
        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "always"
"#,
        )
        .unwrap();
        fs::write(
            cl_dir.join("flow.mmd"),
            r#"stateDiagram-v2
    [*] --> check
    check --> pass
    check --> skip
    pass --> [*]
    skip --> [*]
    check: Did you check?
    pass: Yes
    skip: No
"#,
        )
        .unwrap();

        let event = make_event("tool:before", "bash", "git push origin main", "sess-ab");
        let resp = run(&event, tmp.path()).unwrap();
        if let HookResponse::Block { message } = resp {
            // branching → options shown
            assert!(message.contains("pass") || message.contains("skip"));
        } else {
            panic!("expected block");
        }
    }

    #[test]
    fn session_stop_removes_scope_dir() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());

        // First block creates the session dir
        let event = make_event("tool:before", "bash", "git push origin main", "sess-stop");
        let _ = run(&event, tmp.path()).unwrap();
        let scope_dir = tmp.path().join(".steplock/sessions/sess-stop");
        assert!(scope_dir.exists());

        // session:stop removes the scope dir
        let stop = HookEvent {
            event: "session:stop".to_owned(),
            tool: String::new(),
            input: HashMap::new(),
            output: HashMap::new(),
            session_id: "sess-stop".to_owned(),
            caller: "claude-code".to_owned(),
        };
        let resp = run(&stop, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
        assert!(!scope_dir.exists());
    }

    #[test]
    fn session_stop_approves_when_no_scope_dir() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let stop = HookEvent {
            event: "session:stop".to_owned(),
            tool: String::new(),
            input: HashMap::new(),
            output: HashMap::new(),
            session_id: "nonexistent-session".to_owned(),
            caller: "claude-code".to_owned(),
        };
        let resp = run(&stop, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn session_stop_approves_when_no_steplock_dir() {
        let tmp = TempDir::new().unwrap();
        let stop = HookEvent {
            event: "session:stop".to_owned(),
            tool: String::new(),
            input: HashMap::new(),
            output: HashMap::new(),
            session_id: "sess-x".to_owned(),
            caller: "claude-code".to_owned(),
        };
        let resp = run(&stop, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn session_stop_uses_fallback_id_when_session_id_empty() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());

        // First block with empty session_id creates fallback-id and a scope dir
        let mut input = HashMap::new();
        input.insert(
            "command".to_owned(),
            serde_json::Value::String("git push".to_owned()),
        );
        let no_session = HookEvent {
            event: "tool:before".to_owned(),
            tool: "bash".to_owned(),
            input,
            output: HashMap::new(),
            session_id: String::new(),
            caller: "unknown".to_owned(),
        };
        let _ = run(&no_session, tmp.path()).unwrap();

        let fallback_id =
            fs::read_to_string(tmp.path().join(".steplock/sessions/fallback-id")).unwrap();
        let fallback_id = fallback_id.trim();
        let scope_dir = tmp.path().join(".steplock/sessions").join(fallback_id);
        assert!(scope_dir.exists());

        // session:stop with empty session_id uses fallback-id to clean up
        let stop = HookEvent {
            event: "session:stop".to_owned(),
            tool: String::new(),
            input: HashMap::new(),
            output: HashMap::new(),
            session_id: String::new(),
            caller: "unknown".to_owned(),
        };
        let resp = run(&stop, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
        assert!(!scope_dir.exists());
    }

    // Simulates what ack.sh does: advance current_state to next and add cur to visited.
    fn simulate_ack(state_path: &Path, next: &str) {
        let mut s = load_state(state_path).unwrap();
        s.visited.push(s.current_state.clone());
        s.current_state = next.to_owned();
        s.next_state = None;
        save_state(state_path, &s).unwrap();
    }

    #[test]
    fn two_step_lifecycle_block_ack_block_ack_approve() {
        let tmp = TempDir::new().unwrap();
        let cl_dir = tmp.path().join(".steplock/checklists/ddd-gate");
        fs::create_dir_all(&cl_dir).unwrap();
        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "session"
"#,
        )
        .unwrap();
        fs::write(
            cl_dir.join("flow.mmd"),
            r#"stateDiagram-v2
    [*] --> step_one
    step_one --> step_two
    step_two --> [*]
    step_one : Did you do step one?
    step_two : Did you do step two?
"#,
        )
        .unwrap();

        let event = make_event("tool:before", "bash", "git push origin main", "sess-lc");
        let state_path = tmp
            .path()
            .join(".steplock/sessions/sess-lc/ddd-gate/state.json");

        // Run 1 — no state yet; blocks on step_one, creates state with transitions
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));
        let s = load_state(&state_path).unwrap();
        assert_eq!(s.current_state, "step_one");
        assert_eq!(s.transitions, vec!["step_two"]);

        // Simulate ack of step_one → step_two
        simulate_ack(&state_path, "step_two");

        // Run 2 — blocks on step_two, transitions updated to ["[*]"]
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));
        let s = load_state(&state_path).unwrap();
        assert_eq!(s.current_state, "step_two");
        assert_eq!(s.transitions, vec!["[*]"]);
        assert!(s.visited.contains(&"step_one".to_owned()));

        // Simulate ack of step_two → [*]
        simulate_ack(&state_path, "[*]");

        // Run 3 — checklist complete, approves and resets state
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
        let s = load_state(&state_path).unwrap();
        assert_eq!(s.current_state, "step_one"); // reset to initial
        assert!(s.visited.is_empty());
    }

    #[test]
    fn state_persists_transitions_on_repeated_blocks() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let event = make_event("tool:before", "bash", "git push origin main", "sess-rep");
        let state_path = tmp
            .path()
            .join(".steplock/sessions/sess-rep/quality-gate/state.json");

        // Each call re-computes and writes the same transitions
        run(&event, tmp.path()).unwrap();
        let s1 = load_state(&state_path).unwrap();
        run(&event, tmp.path()).unwrap();
        let s2 = load_state(&state_path).unwrap();
        assert_eq!(s1.transitions, s2.transitions);
        assert_eq!(s1.current_state, s2.current_state);
    }

    #[test]
    fn unknown_current_state_in_flow_skips_checklist() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());

        // Inject a state.json with a current_state not in flow
        let session_dir = tmp
            .path()
            .join(".steplock/sessions/sess-stale/quality-gate");
        fs::create_dir_all(&session_dir).unwrap();
        let state = SessionState {
            checklist: "quality-gate".to_owned(),
            current_state: "nonexistent_state".to_owned(),
            next_state: None,
            transitions: vec![],
            visited: vec![],
        };
        save_state(&session_dir.join("state.json"), &state).unwrap();

        let event = make_event("tool:before", "bash", "git push origin main", "sess-stale");
        let resp = run(&event, tmp.path()).unwrap();
        // Flow doesn't know this state → skip → approve
        assert!(matches!(resp, HookResponse::Approve));
    }
}

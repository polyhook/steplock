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
pub fn run(event: &HookEvent, repo_root: &Path) -> Result<HookResponse> {
    let steplock_dir = repo_root.join(".steplock");
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
                eprintln!(
                    "steplock: block [{}] state={}",
                    checklist_name, initial_state
                );
                return Ok(HookResponse::Block { message });
            }

            Reset::Session | Reset::Branch => {
                let scope_key = get_scope_key(event, &config.reset, &steplock_dir)?;
                let session_dir = steplock_dir.join("sessions").join(&scope_key);
                fs::create_dir_all(&session_dir)?;

                let state_path = session_dir.join("state.json");
                let mut state = if state_path.exists() {
                    load_state(&state_path)?
                } else {
                    init_state(&checklist_name, initial_state)
                };

                // Skip if this checklist is complete
                if state.is_complete() {
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

fn get_scope_key(event: &HookEvent, reset: &Reset, steplock_dir: &Path) -> Result<String> {
    match reset {
        Reset::Branch => {
            // Use git branch name as scope key
            let branch = read_git_branch(steplock_dir);
            Ok(branch.unwrap_or_else(|| "unknown-branch".to_owned()))
        }
        _ => {
            // Session-based scope
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
    }
}

fn read_git_branch(steplock_dir: &Path) -> Option<String> {
    // Walk up from steplock_dir to find .git/HEAD
    let mut dir = steplock_dir.parent()?;
    loop {
        let head = dir.join(".git").join("HEAD");
        if head.exists() {
            let content = fs::read_to_string(&head).ok()?;
            let branch = content
                .strip_prefix("ref: refs/heads/")
                .map(|s| s.trim().to_owned())
                .or_else(|| Some(content.trim()[..8.min(content.trim().len())].to_owned()))?;
            return Some(branch);
        }
        dir = dir.parent()?;
    }
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
        .map(|s| s.as_str())
        .unwrap_or(&state.current_state);

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
        msg.push_str(&format!(
            "When finished, run: sh {}\nThen retry your original command.",
            ack_path
        ));
    } else {
        // Branching — show all real options
        msg.push_str("When finished, run one of:\n");
        for next in &visible {
            let next_label = flow.labels.get(*next).map(|s| s.as_str()).unwrap_or(next);
            msg.push_str(&format!("  sh {} {}   — {}\n", ack_path, next, next_label));
        }
        msg.push_str("Then retry your original command.");
    }

    if allow_preview && state.visited.is_empty() {
        let preview = session_dir.join("preview.sh");
        msg.push_str(&format!(
            "\n(Tip: run sh {} to see all items first.)",
            preview.display()
        ));
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
    fn approves_after_all_states_visited() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());

        // Manually create state.json already at [*]
        let session_dir = tmp.path().join(".steplock/sessions/sess-1");
        fs::create_dir_all(&session_dir).unwrap();
        let state = SessionState {
            checklist: "quality-gate".to_owned(),
            current_state: "[*]".to_owned(),
            next_state: None,
            transitions: vec![],
            visited: vec!["clean_code".to_owned()],
        };
        save_state(&session_dir.join("state.json"), &state).unwrap();

        let event = make_event("tool:before", "bash", "git push origin main", "sess-1");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
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
            session_id: "".to_owned(),
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
    fn reset_branch_uses_git_branch_as_scope() {
        let tmp = TempDir::new().unwrap();
        let cl_dir = tmp.path().join(".steplock/checklists/branch-gate");
        fs::create_dir_all(&cl_dir).unwrap();
        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "branch"
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

        // Create a fake .git/HEAD so read_git_branch returns a branch name
        let git_dir = tmp.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature-branch\n").unwrap();

        let event = make_event("tool:before", "bash", "git push origin main", "sess-b");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));

        // Scope dir should use branch name
        let scope_dir = tmp.path().join(".steplock/sessions/feature-branch");
        assert!(scope_dir.exists());
    }

    #[test]
    fn reset_branch_fallback_when_no_git() {
        let tmp = TempDir::new().unwrap();
        let cl_dir = tmp.path().join(".steplock/checklists/branch-gate2");
        fs::create_dir_all(&cl_dir).unwrap();
        fs::write(
            cl_dir.join("config.toml"),
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "branch"
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

        // No .git dir — falls back to "unknown-branch"
        let event = make_event("tool:before", "bash", "git push origin main", "sess-nb");
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));
        assert!(tmp
            .path()
            .join(".steplock/sessions/unknown-branch")
            .exists());
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
    fn unknown_current_state_in_flow_skips_checklist() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());

        // Inject a state.json with a current_state not in flow
        let session_dir = tmp.path().join(".steplock/sessions/sess-stale");
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

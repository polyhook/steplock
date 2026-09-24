//! Unit tests for `run`.
use super::*;
use crate::state::{load_state, save_state, SessionState};
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
        r"stateDiagram-v2
    [*] --> clean_code
    clean_code --> [*]
    clean_code: Did you write clean code?
",
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
        r"stateDiagram-v2
    [*] --> check
    check --> [*]
    check: Did you check?
",
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
fn block_message_contains_checklist_name() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    let event = make_event("tool:before", "bash", "git push origin main", "sess-name");
    let resp = run(&event, tmp.path()).unwrap();
    if let HookResponse::Block { message } = resp {
        // The checklist dir is "quality-gate" — it must appear in the message prefix
        assert!(
            message.starts_with("[quality-gate:"),
            "expected [quality-gate: prefix, got: {message}"
        );
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
fn block_message_shows_step_progress() {
    let tmp = TempDir::new().unwrap();
    // 3-step linear flow: a → b → c → [*]
    let cl_dir = tmp.path().join(".steplock/checklists/progress-gate");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
            cl_dir.join("config.toml"),
            "on_event = \"tool:before\"\non_tool = \"bash\"\nmatch_input = \"input.command.contains('git push')\"\nreset = \"session\"\n",
        )
        .unwrap();
    fs::write(
            cl_dir.join("flow.mmd"),
            "stateDiagram-v2\n    [*] --> a\n    a --> b\n    b --> c\n    c --> [*]\n    a: Step A\n    b: Step B\n    c: Step C\n",
        )
        .unwrap();

    // First block: step 1/3
    let event = make_event("tool:before", "bash", "git push", "sess-prog");
    let resp = run(&event, tmp.path()).unwrap();
    if let HookResponse::Block { message } = resp {
        assert!(
            message.contains("1/3"),
            "expected 1/3 in message, got: {message}"
        );
    } else {
        panic!("expected block");
    }

    // Advance state manually to simulate ack
    let state_path = tmp
        .path()
        .join(".steplock/sessions/sess-prog/progress-gate/state.json");
    let mut state = load_state(&state_path).unwrap();
    state.visited.push(state.current_state.clone());
    state.current_state = "b".to_owned();
    state.next_state = Some("c".to_owned());
    state.transitions = vec!["c".to_owned()];
    save_state(&state_path, &state).unwrap();

    // Second block: step 2/3
    let resp2 = run(&event, tmp.path()).unwrap();
    if let HookResponse::Block { message } = resp2 {
        assert!(
            message.contains("2/3"),
            "expected 2/3 in message, got: {message}"
        );
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
        r"stateDiagram-v2
    [*] --> check
    check --> [*]
    check: Did you check?
",
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
        r"stateDiagram-v2
    [*] --> check
    check --> pass
    check --> skip
    pass --> [*]
    skip --> [*]
    check: Did you check?
    pass: Yes, it passed
    skip: No, skipped because
",
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
fn reset_always_with_allow_preview_does_not_show_preview_tip() {
    let tmp = TempDir::new().unwrap();
    let cl_dir = tmp.path().join(".steplock/checklists/always-preview");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("config.toml"),
        r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "always"
allow_preview_request = true
"#,
    )
    .unwrap();
    fs::write(
        cl_dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> check\n    check --> [*]\n    check: Did you check?\n",
    )
    .unwrap();

    let event = make_event("tool:before", "bash", "git push origin main", "sess-ap");
    let resp = run(&event, tmp.path()).unwrap();
    if let HookResponse::Block { message } = resp {
        // preview.sh is never written for reset=always, so tip must not appear
        assert!(
            !message.contains("preview.sh"),
            "should not reference non-existent preview.sh: {message}"
        );
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
        r"stateDiagram-v2
    [*] --> check
    check --> pass
    check --> skip
    pass --> [*]
    skip --> [*]
    check: Did you check?
    pass: Yes
    skip: No
",
    )
    .unwrap();

    let event = make_event("tool:before", "bash", "git push origin main", "sess-ab");
    let resp = run(&event, tmp.path()).unwrap();
    if let HookResponse::Block { message } = resp {
        // reset=always: no ack.sh, so no branch options — just the question + retry prompt
        assert!(message.contains("Did you check?"));
        assert!(message.contains("retry your original command"));
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
    run(&event, tmp.path()).unwrap();
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
    run(&no_session, tmp.path()).unwrap();

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
            "stateDiagram-v2\n    [*] --> step_one\n    step_one --> step_two\n    step_two --> [*]\n    step_one : Did you do step one?\n    step_two : Did you do step two?\n",
        )
        .unwrap();

    let event = make_event("tool:before", "bash", "git push origin main", "sess-lc");
    let state_path = tmp
        .path()
        .join(".steplock/sessions/sess-lc/ddd-gate/state.json");

    // Run 1 — no state yet; blocks on step_one, creates state with transitions
    let resp1 = run(&event, tmp.path()).unwrap();
    assert!(matches!(resp1, HookResponse::Block { .. }));
    let s1 = load_state(&state_path).unwrap();
    assert_eq!(s1.current_state, "step_one");
    assert_eq!(s1.transitions, vec!["step_two"]);

    // Simulate ack of step_one → step_two
    simulate_ack(&state_path, "step_two");

    // Run 2 — blocks on step_two, transitions updated to ["[*]"]
    let resp2 = run(&event, tmp.path()).unwrap();
    assert!(matches!(resp2, HookResponse::Block { .. }));
    let s2 = load_state(&state_path).unwrap();
    assert_eq!(s2.current_state, "step_two");
    assert_eq!(s2.transitions, vec!["[*]"]);
    assert!(s2.visited.contains(&"step_one".to_owned()));

    // Simulate ack of step_two → [*]
    simulate_ack(&state_path, "[*]");

    // Run 3 — checklist complete, approves and resets state
    let resp3 = run(&event, tmp.path()).unwrap();
    assert!(matches!(resp3, HookResponse::Approve));
    let s3 = load_state(&state_path).unwrap();
    assert_eq!(s3.current_state, "step_one"); // reset to initial
    assert!(s3.visited.is_empty());
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

#[test]
fn complete_event_written_to_audit_log() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());

    // Put state at [*] so the gate sees a completed checklist
    let session_dir = tmp
        .path()
        .join(".steplock/sessions/sess-audit/quality-gate");
    fs::create_dir_all(&session_dir).unwrap();
    let state = SessionState {
        checklist: "quality-gate".to_owned(),
        current_state: "[*]".to_owned(),
        next_state: None,
        transitions: vec![],
        visited: vec!["clean_code".to_owned()],
    };
    save_state(&session_dir.join("state.json"), &state).unwrap();

    let event = make_event("tool:before", "bash", "git push origin main", "sess-audit");
    let resp = run(&event, tmp.path()).unwrap();
    assert!(matches!(resp, HookResponse::Approve));

    let log_path = tmp.path().join(".steplock/audit.log");
    assert!(log_path.exists(), "audit.log should exist");
    let content = fs::read_to_string(&log_path).unwrap();
    let entry: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(
        entry.get("event").and_then(|v| v.as_str()),
        Some("complete")
    );
    assert_eq!(
        entry.get("checklist").and_then(|v| v.as_str()),
        Some("quality-gate")
    );
    assert_eq!(
        entry.get("session").and_then(|v| v.as_str()),
        Some("sess-audit")
    );
}

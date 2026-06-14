//! Integration tests for the steplock gate runner.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use steplock_core::state::{load_state, save_state};
use steplock_core::{HookEvent, HookResponse};

fn push_event(session: &str) -> HookEvent {
    let mut input = HashMap::new();
    input.insert(
        "command".to_owned(),
        serde_json::Value::String("git push origin main".to_owned()),
    );
    HookEvent::new(
        "tool:before".to_owned(),
        "bash".to_owned(),
        input,
        HashMap::new(),
        session.to_owned(),
        "claude-code".to_owned(),
    )
}

fn write_checklist(root: &Path, name: &str, steps: &[(&str, &str)]) {
    let cl_dir = root.join(".steplock/checklists").join(name);
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("config.toml"),
        "on_event = \"tool:before\"\non_tool = \"bash\"\nreset = \"session\"\n",
    )
    .unwrap();

    let mut mmd = "stateDiagram-v2\n".to_owned();
    let mut prev = "[*]";
    for (id, _label) in steps {
        writeln!(mmd, "    {prev} --> {id}").unwrap();
        prev = id;
    }
    writeln!(mmd, "    {prev} --> [*]").unwrap();
    for (id, label) in steps {
        writeln!(mmd, "    {id}: {label}").unwrap();
    }
    fs::write(cl_dir.join("flow.mmd"), mmd).unwrap();
}

/// Simulate what ack.sh does: advance `current_state` to `next`.
fn ack(root: &Path, checklist: &str, session: &str, next: &str) {
    let state_path = root
        .join(".steplock/sessions")
        .join(session)
        .join(checklist)
        .join("state.json");
    let mut state = load_state(&state_path).unwrap();
    state.visited.push(state.current_state.clone());
    state.current_state = next.to_owned();
    state.next_state = None;
    state.transitions = vec![];
    save_state(&state_path, &state).unwrap();
}

// ── Sequential blocking ────────────────────────────────────────────────────

#[test]
fn two_step_checklist_blocks_in_order_then_approves() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_checklist(
        tmp.path(),
        "quality",
        &[("step_a", "Step A"), ("step_b", "Step B")],
    );
    let event = push_event("s1");

    // First call: blocked at step_a
    match steplock_core::run(&event, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step A")),
        HookResponse::Approve => panic!("expected block at step_a"),
        _ => panic!("unexpected variant"),
    }

    // Ack step_a → advance to step_b
    ack(tmp.path(), "quality", "s1", "step_b");

    // Second call: blocked at step_b
    match steplock_core::run(&event, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step B")),
        HookResponse::Approve => panic!("expected block at step_b"),
        _ => panic!("unexpected variant"),
    }

    // Ack step_b → advance to terminal [*]
    ack(tmp.path(), "quality", "s1", "[*]");

    // Third call: approved (checklist complete) + state reset
    let resp = steplock_core::run(&event, tmp.path()).unwrap();
    assert!(matches!(resp, HookResponse::Approve));

    // Next call starts fresh at step_a
    match steplock_core::run(&event, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step A")),
        HookResponse::Approve => panic!("expected fresh block after reset"),
        _ => panic!("unexpected variant"),
    }
}

// ── Session isolation ──────────────────────────────────────────────────────

#[test]
fn two_sessions_are_isolated() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_checklist(
        tmp.path(),
        "quality",
        &[("step_a", "Step A"), ("step_b", "Step B")],
    );

    let ev_a = push_event("sess-a");
    let ev_b = push_event("sess-b");

    // Both sessions block initially at step_a
    assert!(matches!(
        steplock_core::run(&ev_a, tmp.path()).unwrap(),
        HookResponse::Block { .. }
    ));
    assert!(matches!(
        steplock_core::run(&ev_b, tmp.path()).unwrap(),
        HookResponse::Block { .. }
    ));

    // Advance only sess-a to step_b
    ack(tmp.path(), "quality", "sess-a", "step_b");

    // sess-a is at step_b, sess-b still at step_a
    match steplock_core::run(&ev_a, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step B")),
        HookResponse::Approve => panic!("sess-a should be at step_b"),
        _ => panic!("unexpected variant"),
    }
    match steplock_core::run(&ev_b, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step A")),
        HookResponse::Approve => panic!("sess-b should still be at step_a"),
        _ => panic!("unexpected variant"),
    }
}

// ── Multi-checklist ────────────────────────────────────────────────────────

#[test]
fn first_matching_checklist_alphabetically_blocks() {
    let tmp = tempfile::TempDir::new().unwrap();
    // "a-gate" sorts before "z-gate" — "a-gate" should block first
    write_checklist(tmp.path(), "a-gate", &[("check_a", "Check A")]);
    write_checklist(tmp.path(), "z-gate", &[("check_z", "Check Z")]);
    let event = push_event("s-multi");

    match steplock_core::run(&event, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Check A")),
        HookResponse::Approve => panic!("expected block from a-gate"),
        _ => panic!("unexpected variant"),
    }
}

// ── CEL match_input end-to-end ─────────────────────────────────────────────

#[test]
fn cel_match_input_gates_only_matching_commands() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cl_dir = tmp.path().join(".steplock/checklists/pre-push");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("config.toml"),
        "on_event = \"tool:before\"\non_tool = \"bash\"\nmatch_input = \"input.command_words.exists(x, x == 'push')\"\nreset = \"session\"\n",
    )
    .unwrap();
    fs::write(
        cl_dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> verify\n    verify --> [*]\n    verify: Verify before pushing\n",
    )
    .unwrap();

    let mut input_ls = HashMap::new();
    input_ls.insert(
        "command".to_owned(),
        serde_json::Value::String("ls -la".to_owned()),
    );
    let ls_event = HookEvent::new(
        "tool:before".to_owned(),
        "bash".to_owned(),
        input_ls,
        HashMap::new(),
        "s-cel".to_owned(),
        "claude-code".to_owned(),
    );
    // ls command: no "push" word → approve
    assert!(matches!(
        steplock_core::run(&ls_event, tmp.path()).unwrap(),
        HookResponse::Approve
    ));

    let mut input_path = HashMap::new();
    input_path.insert(
        "command".to_owned(),
        // "push" appears only in a path, not as a word
        serde_json::Value::String("git add .steplock/checklists/pre-push/config.toml".to_owned()),
    );
    let path_event = HookEvent::new(
        "tool:before".to_owned(),
        "bash".to_owned(),
        input_path,
        HashMap::new(),
        "s-cel".to_owned(),
        "claude-code".to_owned(),
    );
    // "push" in path but not a standalone word → approve
    assert!(matches!(
        steplock_core::run(&path_event, tmp.path()).unwrap(),
        HookResponse::Approve
    ));

    // Real push command → block
    assert!(matches!(
        steplock_core::run(&push_event("s-cel"), tmp.path()).unwrap(),
        HookResponse::Block { .. }
    ));
}

// ── State persistence ──────────────────────────────────────────────────────

#[test]
fn state_json_readable_as_session_state() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_checklist(tmp.path(), "gate", &[("step_a", "Step A")]);
    let event = push_event("s-persist");
    let _: steplock_core::HookResponse = steplock_core::run(&event, tmp.path()).unwrap();

    let state_path = tmp
        .path()
        .join(".steplock/sessions/s-persist/gate/state.json");
    let state = load_state(&state_path).unwrap();
    assert_eq!(state.checklist, "gate");
    assert_eq!(state.current_state, "step_a");
    assert!(!state.transitions.is_empty());
    assert!(state.visited.is_empty());
    assert!(!state.is_complete());
}

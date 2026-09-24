//! Integration tests for the steplock gate runner.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use steplock::state::{load_state, save_state};
use steplock::{HookEvent, HookResponse};

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
    match steplock::run(&event, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step A")),
        HookResponse::Approve => panic!("expected block at step_a"),
        _ => panic!("unexpected variant"),
    }

    // Ack step_a → advance to step_b
    ack(tmp.path(), "quality", "s1", "step_b");

    // Second call: blocked at step_b
    match steplock::run(&event, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step B")),
        HookResponse::Approve => panic!("expected block at step_b"),
        _ => panic!("unexpected variant"),
    }

    // Ack step_b → advance to terminal [*]
    ack(tmp.path(), "quality", "s1", "[*]");

    // Third call: approved (checklist complete) + state reset
    let resp = steplock::run(&event, tmp.path()).unwrap();
    assert!(matches!(resp, HookResponse::Approve));

    // Next call starts fresh at step_a
    match steplock::run(&event, tmp.path()).unwrap() {
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
        steplock::run(&ev_a, tmp.path()).unwrap(),
        HookResponse::Block { .. }
    ));
    assert!(matches!(
        steplock::run(&ev_b, tmp.path()).unwrap(),
        HookResponse::Block { .. }
    ));

    // Advance only sess-a to step_b
    ack(tmp.path(), "quality", "sess-a", "step_b");

    // sess-a is at step_b, sess-b still at step_a
    match steplock::run(&ev_a, tmp.path()).unwrap() {
        HookResponse::Block { message } => assert!(message.contains("Step B")),
        HookResponse::Approve => panic!("sess-a should be at step_b"),
        _ => panic!("unexpected variant"),
    }
    match steplock::run(&ev_b, tmp.path()).unwrap() {
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

    match steplock::run(&event, tmp.path()).unwrap() {
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
        steplock::run(&ls_event, tmp.path()).unwrap(),
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
        steplock::run(&path_event, tmp.path()).unwrap(),
        HookResponse::Approve
    ));

    // Real push command → block
    assert!(matches!(
        steplock::run(&push_event("s-cel"), tmp.path()).unwrap(),
        HookResponse::Block { .. }
    ));
}

// ── State persistence ──────────────────────────────────────────────────────

#[test]
fn state_json_readable_as_session_state() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_checklist(tmp.path(), "gate", &[("step_a", "Step A")]);
    let event = push_event("s-persist");
    let _: steplock::HookResponse = steplock::run(&event, tmp.path()).unwrap();

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

// ── Global checklists ──────────────────────────────────────────────────────

/// Write a one-step checklist into a bare steplock dir (`<dir>/checklists/<name>/`).
fn write_global_checklist(steplock_dir: &Path, name: &str, label: &str) {
    let cl_dir = steplock_dir.join("checklists").join(name);
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("config.toml"),
        "on_event = \"tool:before\"\non_tool = \"bash\"\nreset = \"session\"\n",
    )
    .unwrap();
    fs::write(
        cl_dir.join("flow.mmd"),
        format!("stateDiagram-v2\n    [*] --> g\n    g --> [*]\n    g: {label}\n"),
    )
    .unwrap();
}

fn block_message(resp: HookResponse) -> String {
    match resp {
        HookResponse::Block { message } => message,
        HookResponse::Approve => panic!("expected block"),
        _ => panic!("unexpected variant"),
    }
}

#[test]
fn global_checklist_blocks_when_project_has_none() {
    let project = tempfile::TempDir::new().unwrap();
    let global = tempfile::TempDir::new().unwrap();
    write_global_checklist(global.path(), "gate", "Global step");

    let resp =
        steplock::run_with_global(&push_event("s1"), project.path(), Some(global.path())).unwrap();
    assert!(
        block_message(resp).contains("Global step"),
        "global checklist must block"
    );
    assert!(
        global.path().join("sessions/s1/gate/state.json").exists(),
        "state must be stored in the global dir"
    );
}

#[test]
fn project_checklists_run_before_global() {
    let project = tempfile::TempDir::new().unwrap();
    let global = tempfile::TempDir::new().unwrap();
    write_checklist(project.path(), "project-gate", &[("p", "Project step")]);
    write_global_checklist(global.path(), "global-gate", "Global step");
    let event = push_event("s1");

    let first = steplock::run_with_global(&event, project.path(), Some(global.path())).unwrap();
    assert!(
        block_message(first).contains("Project step"),
        "project checklist must block first"
    );

    ack(project.path(), "project-gate", "s1", "[*]");
    let second = steplock::run_with_global(&event, project.path(), Some(global.path())).unwrap();
    assert!(
        block_message(second).contains("Global step"),
        "global checklist must block after the project one completes"
    );
}

#[test]
fn project_checklist_shadows_global_with_same_name() {
    let project = tempfile::TempDir::new().unwrap();
    let global = tempfile::TempDir::new().unwrap();
    write_checklist(project.path(), "gate", &[("p", "Project step")]);
    write_global_checklist(global.path(), "gate", "Global step");
    let event = push_event("s1");

    ack_after_block(project.path(), global.path(), &event);
    let resp = steplock::run_with_global(&event, project.path(), Some(global.path())).unwrap();
    assert!(
        matches!(resp, HookResponse::Approve),
        "shadowed global checklist must not run"
    );
}

/// Block once on the project `gate` checklist, then ack it to completion.
fn ack_after_block(project: &Path, global: &Path, event: &HookEvent) {
    let resp = steplock::run_with_global(event, project, Some(global)).unwrap();
    assert!(
        block_message(resp).contains("Project step"),
        "project checklist must win over same-name global"
    );
    ack(project, "gate", "s1", "[*]");
}

#[test]
fn empty_project_dir_disables_same_name_global_checklist() {
    let project = tempfile::TempDir::new().unwrap();
    let global = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(project.path().join(".steplock/checklists/gate")).unwrap();
    write_global_checklist(global.path(), "gate", "Global step");

    let resp =
        steplock::run_with_global(&push_event("s1"), project.path(), Some(global.path())).unwrap();
    assert!(
        matches!(resp, HookResponse::Approve),
        "empty same-name project dir must disable the global checklist"
    );
}

#[test]
fn global_dir_equal_to_project_dir_is_evaluated_once() {
    let project = tempfile::TempDir::new().unwrap();
    write_checklist(project.path(), "gate", &[("p", "Project step")]);
    let steplock_dir = project.path().join(".steplock");
    let event = push_event("s1");

    let first = steplock::run_with_global(&event, project.path(), Some(&steplock_dir)).unwrap();
    assert!(block_message(first).contains("Project step"), "blocks once");
    ack(project.path(), "gate", "s1", "[*]");
    let second = steplock::run_with_global(&event, project.path(), Some(&steplock_dir)).unwrap();
    assert!(
        matches!(second, HookResponse::Approve),
        "same dir must not be evaluated twice"
    );
}

#[test]
fn session_stop_cleans_global_sessions() {
    let project = tempfile::TempDir::new().unwrap();
    let global = tempfile::TempDir::new().unwrap();
    write_global_checklist(global.path(), "gate", "Global step");
    steplock::run_with_global(&push_event("s1"), project.path(), Some(global.path())).unwrap();
    assert!(
        global.path().join("sessions/s1").exists(),
        "session created"
    );

    let stop = HookEvent::new(
        "session:stop".to_owned(),
        String::new(),
        HashMap::new(),
        HashMap::new(),
        "s1".to_owned(),
        "claude-code".to_owned(),
    );
    steplock::run_with_global(&stop, project.path(), Some(global.path())).unwrap();
    assert!(
        !global.path().join("sessions/s1").exists(),
        "session:stop must clean the global session dir"
    );
}

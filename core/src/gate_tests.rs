//! Unit tests for `gate`.
use super::*;
use crate::state::save_state;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

const LINEAR_FLOW: &str = r"stateDiagram-v2
    [*] --> clean_code
    clean_code --> tests_pass
    tests_pass --> [*]
    clean_code: Did you write clean code?
    tests_pass: Do the tests pass?
";

const BRANCHING_FLOW: &str = r"stateDiagram-v2
    [*] --> review
    review --> ship
    review --> abort
    ship --> [*]
    abort --> [*]
    review: Reviewed the diff?
    ship: Ship it
    abort: Abandon the change
";

fn make_event(session: &str) -> HookEvent {
    let mut input = HashMap::new();
    input.insert(
        "command".to_owned(),
        serde_json::Value::String("git push".to_owned()),
    );
    HookEvent {
        event: "tool:before".to_owned(),
        tool: "bash".to_owned(),
        input,
        output: HashMap::new(),
        session_id: session.to_owned(),
        caller: "claude-code".to_owned(),
    }
}

/// Writes `.steplock/checklists/<name>/` and returns `(steplock_dir, checklist_dir)`.
fn setup(root: &Path, name: &str, reset: &str, flow: &str) -> (PathBuf, PathBuf) {
    let steplock_dir = root.join(".steplock");
    let checklist_dir = steplock_dir.join("checklists").join(name);
    fs::create_dir_all(&checklist_dir).unwrap();
    fs::write(
        checklist_dir.join("config.toml"),
        format!(
            r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "{reset}"
"#
        ),
    )
    .unwrap();
    fs::write(checklist_dir.join("flow.mmd"), flow).unwrap();
    (steplock_dir, checklist_dir)
}

fn state_path(steplock_dir: &Path, scope: &str, checklist: &str) -> PathBuf {
    steplock_dir
        .join("sessions")
        .join(scope)
        .join(checklist)
        .join("state.json")
}

fn block_message(resp: Option<HookResponse>) -> String {
    match resp {
        Some(HookResponse::Block { message }) => message,
        other => panic!("expected a block, got {other:?}"),
    }
}

#[test]
fn empty_session_id_creates_and_reuses_a_fallback_scope_key() {
    let tmp = TempDir::new().unwrap();
    let (steplock_dir, checklist_dir) = setup(tmp.path(), "gate", "session", LINEAR_FLOW);
    let event = make_event("");

    evaluate_checklist(&event, &steplock_dir, &checklist_dir).unwrap();
    let fallback_path = steplock_dir.join("sessions").join("fallback-id");
    let scope = fs::read_to_string(&fallback_path)
        .unwrap()
        .trim()
        .to_owned();
    assert!(!scope.is_empty(), "fallback id should be persisted");
    assert!(state_path(&steplock_dir, &scope, "gate").exists());

    // A second event with no session id must land in the same scope, so
    // checklist progress survives callers that do not supply a session id.
    evaluate_checklist(&event, &steplock_dir, &checklist_dir).unwrap();
    let scope_again = fs::read_to_string(&fallback_path)
        .unwrap()
        .trim()
        .to_owned();
    assert_eq!(scope, scope_again);
}

#[test]
fn single_transition_state_precomputes_next_state() {
    let tmp = TempDir::new().unwrap();
    let (steplock_dir, checklist_dir) = setup(tmp.path(), "gate", "session", LINEAR_FLOW);

    evaluate_checklist(&make_event("sess-1"), &steplock_dir, &checklist_dir).unwrap();

    let state = load_state(&state_path(&steplock_dir, "sess-1", "gate")).unwrap();
    assert_eq!(state.current_state, "clean_code");
    assert_eq!(state.next_state.as_deref(), Some("tests_pass"));
    assert_eq!(state.transitions, vec!["tests_pass".to_owned()]);
    // ack.sh needs the whole graph to advance beyond the current state.
    assert!(state.flow_transitions.contains_key("tests_pass"));
}

#[test]
fn branching_state_leaves_next_state_unset_and_lists_every_choice() {
    let tmp = TempDir::new().unwrap();
    let (steplock_dir, checklist_dir) = setup(tmp.path(), "gate", "session", BRANCHING_FLOW);

    let resp = evaluate_checklist(&make_event("sess-1"), &steplock_dir, &checklist_dir).unwrap();
    let message = block_message(resp);

    let state = load_state(&state_path(&steplock_dir, "sess-1", "gate")).unwrap();
    assert_eq!(
        state.next_state, None,
        "an ambiguous branch must not auto-advance"
    );
    assert_eq!(
        state.transitions,
        vec!["ship".to_owned(), "abort".to_owned()]
    );

    assert!(message.contains("run one of"), "message was: {message}");
    assert!(message.contains("ack.sh ship"), "message was: {message}");
    assert!(message.contains("ack.sh abort"), "message was: {message}");
}

#[test]
fn state_missing_from_the_flow_is_skipped() {
    let tmp = TempDir::new().unwrap();
    let (steplock_dir, checklist_dir) = setup(tmp.path(), "gate", "session", LINEAR_FLOW);
    let path = state_path(&steplock_dir, "sess-1", "gate");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Simulates flow.mmd being edited mid-session: the stored state is gone.
    save_state(&path, &init_state("gate", "state_that_was_removed")).unwrap();

    let resp = evaluate_checklist(&make_event("sess-1"), &steplock_dir, &checklist_dir).unwrap();
    assert!(resp.is_none(), "an unknown state must not block forever");
}

#[test]
fn step_counter_counts_visited_states() {
    let tmp = TempDir::new().unwrap();
    let (steplock_dir, checklist_dir) = setup(tmp.path(), "gate", "session", LINEAR_FLOW);
    let path = state_path(&steplock_dir, "sess-1", "gate");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut state = init_state("gate", "tests_pass");
    state.visited = vec!["clean_code".to_owned()];
    save_state(&path, &state).unwrap();

    let message = block_message(
        evaluate_checklist(&make_event("sess-1"), &steplock_dir, &checklist_dir).unwrap(),
    );
    assert!(
        message.starts_with("[gate: 2/2] Do the tests pass?"),
        "message was: {message}"
    );
}

#[test]
fn reset_always_blocks_without_writing_session_state() {
    let tmp = TempDir::new().unwrap();
    let (steplock_dir, checklist_dir) = setup(tmp.path(), "gate", "always", LINEAR_FLOW);

    let message = block_message(
        evaluate_checklist(&make_event("sess-1"), &steplock_dir, &checklist_dir).unwrap(),
    );
    assert!(message.contains("When done, retry your original command."));
    assert!(!message.contains("ack.sh"), "message was: {message}");
    assert!(
        !steplock_dir.join("sessions").exists(),
        "reset=always keeps no session state"
    );
}

#[test]
fn completed_checklist_approves_once_and_restarts() {
    let tmp = TempDir::new().unwrap();
    let (steplock_dir, checklist_dir) = setup(tmp.path(), "gate", "session", LINEAR_FLOW);
    let path = state_path(&steplock_dir, "sess-1", "gate");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut state = init_state("gate", "[*]");
    state.visited = vec!["clean_code".to_owned(), "tests_pass".to_owned()];
    save_state(&path, &state).unwrap();

    let resp = evaluate_checklist(&make_event("sess-1"), &steplock_dir, &checklist_dir).unwrap();
    assert!(resp.is_none(), "a complete checklist approves the attempt");

    let reset = load_state(&path).unwrap();
    assert_eq!(reset.current_state, "clean_code");
    assert!(reset.visited.is_empty());

    // The next attempt blocks again from the top.
    let next = evaluate_checklist(&make_event("sess-1"), &steplock_dir, &checklist_dir).unwrap();
    assert!(matches!(next, Some(HookResponse::Block { .. })));
}

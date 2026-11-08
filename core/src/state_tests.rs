//! Unit tests for `state`.
use super::*;
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn init_state_sets_fields() {
    let s = init_state("my-checklist", "first_step");
    assert_eq!(s.checklist, "my-checklist");
    assert_eq!(s.current_state, "first_step");
    assert!(s.next_state.is_none());
    assert!(s.transitions.is_empty());
    assert!(s.visited.is_empty());
}

#[test]
fn is_complete_false_when_active() {
    let s = init_state("cl", "step_one");
    assert!(!s.is_complete());
}

#[test]
fn is_complete_true_at_end() {
    let s = SessionState {
        checklist: "cl".to_owned(),
        current_state: "[*]".to_owned(),
        next_state: None,
        transitions: vec![],
        flow_transitions: HashMap::new(),
        visited: vec!["step_one".to_owned()],
    };
    assert!(s.is_complete());
}

#[test]
fn save_and_load_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("state.json");
    let s = SessionState {
        checklist: "gate".to_owned(),
        current_state: "check_one".to_owned(),
        next_state: Some("check_two".to_owned()),
        transitions: vec!["check_two".to_owned()],
        flow_transitions: HashMap::new(),
        visited: vec!["prev".to_owned()],
    };
    save_state(&path, &s).unwrap();
    let loaded = load_state(&path).unwrap();
    assert_eq!(loaded.checklist, s.checklist);
    assert_eq!(loaded.current_state, s.current_state);
    assert_eq!(loaded.next_state, s.next_state);
    assert_eq!(loaded.transitions, s.transitions);
    assert_eq!(loaded.visited, s.visited);
}

#[test]
fn load_state_error_on_missing_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("nonexistent.json");
    load_state(&path).unwrap_err();
}

#[test]
fn save_state_overwrites_and_leaves_no_temp_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("state.json");
    save_state(&path, &init_state("gate", "first")).unwrap();
    save_state(&path, &init_state("gate", "second")).unwrap();
    assert_eq!(
        load_state(&path).unwrap().current_state,
        "second",
        "second save must win"
    );
    let names: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec!["state.json"], "temp file must be renamed away");
}

#[test]
fn save_state_failure_removes_temp_file() {
    let tmp = TempDir::new().unwrap();
    // Renaming a file over a non-empty directory fails on every platform.
    let path = tmp.path().join("state.json");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("keep"), "x").unwrap();
    save_state(&path, &init_state("gate", "first")).unwrap_err();
    let names: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        vec!["state.json"],
        "temp file must be cleaned up on failure"
    );
}

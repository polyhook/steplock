//! Unit tests for `validate`.
use std::path::Path;

use super::*;
use std::fs;
use tempfile::TempDir;

fn write_checklist(dir: &Path, name: &str, config: &str, flow: &str) {
    let cl = dir.join(name);
    fs::create_dir_all(&cl).unwrap();
    fs::write(cl.join("config.toml"), config).unwrap();
    fs::write(cl.join("flow.mmd"), flow).unwrap();
}

const GOOD_CONFIG: &str = r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "session"
"#;

const GOOD_FLOW: &str =
    "stateDiagram-v2\n    [*] --> check\n    check --> [*]\n    check : Check it\n";

#[test]
fn valid_checklist_returns_no_errors() {
    let tmp = TempDir::new().unwrap();
    write_checklist(tmp.path(), "my-gate", GOOD_CONFIG, GOOD_FLOW);
    let errs = validate_checklists(tmp.path());
    assert!(errs.is_empty());
}

#[test]
fn bad_config_toml_is_reported() {
    let tmp = TempDir::new().unwrap();
    write_checklist(tmp.path(), "bad-gate", "not valid toml !!!", GOOD_FLOW);
    let errs = validate_checklists(tmp.path());
    assert_eq!(errs.len(), 1);
    assert!(errs.first().is_some_and(|(l, _)| l.contains("config.toml")));
}

#[test]
fn bad_flow_mmd_is_reported() {
    let tmp = TempDir::new().unwrap();
    write_checklist(tmp.path(), "bad-gate", GOOD_CONFIG, "not a mermaid diagram");
    let errs = validate_checklists(tmp.path());
    assert_eq!(errs.len(), 1);
    assert!(errs.first().is_some_and(|(l, _)| l.contains("flow.mmd")));
}

#[test]
fn missing_config_toml_is_reported() {
    let tmp = TempDir::new().unwrap();
    let cl = tmp.path().join("missing-config");
    fs::create_dir_all(&cl).unwrap();
    fs::write(cl.join("flow.mmd"), GOOD_FLOW).unwrap();
    // no config.toml
    let errs = validate_checklists(tmp.path());
    assert_eq!(errs.len(), 1);
    assert!(errs.first().is_some_and(|(l, _)| l.contains("config.toml")));
}

#[test]
fn multiple_bad_checklists_all_reported() {
    let tmp = TempDir::new().unwrap();
    write_checklist(tmp.path(), "bad-1", "not toml", "not mermaid");
    write_checklist(tmp.path(), "bad-2", "not toml", "not mermaid");
    let errs = validate_checklists(tmp.path());
    assert_eq!(errs.len(), 4); // 2 config + 2 flow errors
}

#[test]
fn empty_checklists_dir_returns_no_errors() {
    let tmp = TempDir::new().unwrap();
    let errs = validate_checklists(tmp.path());
    assert!(errs.is_empty());
}

#[test]
fn nonexistent_dir_returns_no_errors() {
    let tmp = TempDir::new().unwrap();
    let errs = validate_checklists(&tmp.path().join("does-not-exist"));
    assert!(errs.is_empty());
}

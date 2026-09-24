//! Unit tests for `audit`.
use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn appends_jsonl_line() {
    let tmp = TempDir::new().unwrap();
    append(tmp.path(), "block", "my-checklist", "step_one", "sess-abc");
    let content = fs::read_to_string(tmp.path().join("audit.log")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(parsed["event"], "block");
    assert_eq!(parsed["checklist"], "my-checklist");
    assert_eq!(parsed["state"], "step_one");
    assert_eq!(parsed["session"], "sess-abc");
    assert!(parsed["ts"].is_string());
}

#[test]
fn appends_multiple_lines() {
    let tmp = TempDir::new().unwrap();
    append(tmp.path(), "block", "cl", "s1", "sess");
    append(tmp.path(), "ack", "cl", "s1", "sess");
    let content = fs::read_to_string(tmp.path().join("audit.log")).unwrap();
    assert_eq!(content.lines().count(), 2);
}

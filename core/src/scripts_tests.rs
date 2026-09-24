//! Unit tests for `scripts`.
use super::*;
use crate::flow::parse_mmd;
use std::fs;
use tempfile::TempDir;

const SIMPLE_MMD: &str = r"stateDiagram-v2
    [*] --> step_one
    step_one --> [*]
    step_one : Do the first thing
";

const TWO_STEP_MMD: &str = r"stateDiagram-v2
    [*] --> step_one
    step_one --> step_two
    step_two --> [*]
    step_one : First step
    step_two : Second step
";

#[test]
fn ensure_ack_sh_creates_file() {
    let tmp = TempDir::new().unwrap();
    ensure_ack_sh(tmp.path()).unwrap();
    let path = tmp.path().join("ack.sh");
    assert!(path.exists());
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("state.json"));
}

#[test]
fn ack_sh_handles_complete_session() {
    assert!(ACK_SH.contains("session already complete"));
    assert!(ACK_SH.contains("[*]"));
}

#[test]
fn ack_sh_appends_audit_event() {
    assert!(ACK_SH.contains("audit.log"));
    assert!(ACK_SH.contains("\"ack\""));
}

#[test]
fn ensure_ack_sh_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("ack.sh");
    fs::write(&path, "custom content").unwrap();
    ensure_ack_sh(tmp.path()).unwrap();
    // Should not overwrite existing file
    assert_eq!(fs::read_to_string(&path).unwrap(), "custom content");
}

#[test]
fn ensure_preview_sh_creates_file() {
    let tmp = TempDir::new().unwrap();
    let flow = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
    ensure_preview_sh(tmp.path(), "my-checklist", &flow).unwrap();
    let path = tmp.path().join("preview.sh");
    assert!(path.exists());
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("my-checklist"));
    assert!(content.contains("Do the first thing"));
}

#[test]
fn ensure_preview_sh_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("preview.sh");
    fs::write(&path, "custom").unwrap();
    let flow = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
    ensure_preview_sh(tmp.path(), "checklist", &flow).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "custom");
}

#[test]
fn preview_sh_singular_item() {
    let flow = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
    let script = build_preview_sh("my-gate", &flow);
    assert!(script.contains("1 item)"));
    assert!(!script.contains("1 items)"));
}

#[test]
fn preview_sh_plural_items() {
    let flow = parse_mmd("test.mmd", TWO_STEP_MMD).unwrap();
    let script = build_preview_sh("my-gate", &flow);
    assert!(script.contains("2 items)"));
}

#[test]
fn preview_sh_escapes_single_quotes() {
    let mmd = "stateDiagram-v2\n    [*] --> s\n    s --> [*]\n    s : It's fine\n";
    let flow = parse_mmd("test.mmd", mmd).unwrap();
    let script = build_preview_sh("checklist", &flow);
    assert!(script.contains("It'\\''s fine"));
}

#[test]
fn preview_sh_uses_state_name_as_fallback_label() {
    let mmd = "stateDiagram-v2\n    [*] --> unlabeled\n    unlabeled --> [*]\n";
    let flow = parse_mmd("test.mmd", mmd).unwrap();
    let script = build_preview_sh("checklist", &flow);
    assert!(script.contains("'unlabeled' 'unlabeled'"));
}

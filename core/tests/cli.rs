//! CLI integration tests for the `steplock` binary.
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

const STEPLOCK: &str = env!("CARGO_BIN_EXE_steplock");

fn hook_event(tool_name: &str, command: &str, session: &str) -> String {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": tool_name,
        "tool_input": { "command": command },
        "tool_output": {},
        "session_id": session
    })
    .to_string()
}

fn checklist(root: &std::path::Path, on_event: &str, on_tool: &str, match_input: Option<&str>) {
    let dir = root.join(".steplock/checklists/gate");
    fs::create_dir_all(&dir).unwrap();
    let mut cfg =
        format!("on_event = \"{on_event}\"\non_tool = \"{on_tool}\"\nreset = \"session\"\n");
    if let Some(expr) = match_input {
        cfg.push_str(&format!("match_input = \"{expr}\"\n"));
    }
    fs::write(dir.join("config.toml"), cfg).unwrap();
    fs::write(
        dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> check\n    check --> [*]\n    check: Did you check?\n",
    )
    .unwrap();
}

fn run_steplock(root: &std::path::Path, stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(STEPLOCK)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn steplock");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn version_flag_prints_version() {
    let output = Command::new(STEPLOCK)
        .arg("--version")
        .output()
        .expect("failed to run steplock");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("steplock "),
        "expected 'steplock X.Y.Z', got: {stdout}"
    );
    assert!(output.status.success());
}

#[test]
fn short_version_flag() {
    let output = Command::new(STEPLOCK)
        .arg("-V")
        .output()
        .expect("failed to run steplock");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("steplock "));
}

#[test]
fn unknown_arg_exits_nonzero() {
    let output = Command::new(STEPLOCK)
        .arg("--unknown-flag")
        .output()
        .expect("failed to run steplock");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn init_creates_checklists_dir() {
    let tmp = TempDir::new().unwrap();
    let output = Command::new(STEPLOCK)
        .arg("init")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run steplock init");
    assert!(output.status.success(), "init should succeed");
    assert!(tmp.path().join(".steplock/checklists").is_dir());
    let gitignore = fs::read_to_string(tmp.path().join(".steplock/.gitignore")).unwrap();
    assert!(gitignore.contains("sessions/"));
    assert!(gitignore.contains("audit.log"));
}

#[test]
fn init_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let out1 = Command::new(STEPLOCK)
        .arg("init")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let out2 = Command::new(STEPLOCK)
        .arg("init")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(out1.status.success());
    assert!(out2.status.success());
}

#[test]
fn hook_approves_when_no_checklists_dir() {
    let tmp = TempDir::new().unwrap();
    let stdin = hook_event("bash", "ls -la", "sess1");
    let (code, stdout, _stderr) = run_steplock(tmp.path(), &stdin);
    assert_eq!(code, 0);
    // polyhook approve response contains "approve"
    // polyhook approve response is an empty JSON object: {}
    assert!(
        stdout.trim() == "{}" || stdout.is_empty(),
        "expected approve response, got: {stdout}"
    );
}

#[test]
fn hook_approves_non_matching_command() {
    let tmp = TempDir::new().unwrap();
    checklist(
        tmp.path(),
        "tool:before",
        "bash",
        Some("input.command.contains('git push')"),
    );
    let stdin = hook_event("bash", "ls -la", "sess1");
    let (code, _stdout, _stderr) = run_steplock(tmp.path(), &stdin);
    assert_eq!(code, 0);
}

#[test]
fn hook_blocks_matching_command() {
    let tmp = TempDir::new().unwrap();
    checklist(
        tmp.path(),
        "tool:before",
        "bash",
        Some("input.command.contains('git push')"),
    );
    let stdin = hook_event("bash", "git push origin main", "sess1");
    let (code, stdout, _stderr) = run_steplock(tmp.path(), &stdin);
    assert_eq!(
        code, 0,
        "steplock exits 0 on block (response goes to stdout)"
    );
    assert!(
        stdout.to_lowercase().contains("block") || stdout.contains("Did you check"),
        "expected block response, got: {stdout}"
    );
}

#[test]
fn hook_invalid_json_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    checklist(tmp.path(), "tool:before", "bash", None);
    let (code, _stdout, stderr) = run_steplock(tmp.path(), "not valid json");
    assert_ne!(code, 0, "invalid input should exit non-zero");
    assert!(
        stderr.contains("steplock"),
        "error message should mention steplock"
    );
}

#[test]
fn hook_finds_steplock_dir_in_parent() {
    let tmp = TempDir::new().unwrap();
    checklist(
        tmp.path(),
        "tool:before",
        "bash",
        Some("input.command.contains('git push')"),
    );
    // Run from a subdirectory — steplock should walk up to find .steplock/
    let subdir = tmp.path().join("a/b/c");
    fs::create_dir_all(&subdir).unwrap();
    let stdin = hook_event("bash", "git push origin main", "sess1");

    let mut child = Command::new(STEPLOCK)
        .current_dir(&subdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.to_lowercase().contains("block") || stdout.contains("Did you check"),
        "should block even from subdirectory; got: {stdout}"
    );
}

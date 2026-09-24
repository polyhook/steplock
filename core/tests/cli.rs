//! CLI integration tests for the `steplock` binary.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
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

fn checklist(root: &Path, on_event: &str, on_tool: &str, match_input: Option<&str>) {
    let dir = root.join(".steplock/checklists/gate");
    fs::create_dir_all(&dir).unwrap();
    let mut cfg =
        format!("on_event = \"{on_event}\"\non_tool = \"{on_tool}\"\nreset = \"session\"\n");
    if let Some(expr) = match_input {
        writeln!(cfg, "match_input = \"{expr}\"").unwrap();
    }
    fs::write(dir.join("config.toml"), cfg).unwrap();
    fs::write(
        dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> check\n    check --> [*]\n    check: Did you check?\n",
    )
    .unwrap();
}

fn run_steplock(root: &Path, stdin: &str) -> (i32, String, String) {
    run_steplock_with_global(root, stdin, "")
}

/// Run the hook in `dir` with `STEPLOCK_GLOBAL_DIR` set to `global` (`""` disables global
/// checklists) and `stdin` as the hook event. Returns `(exit code, stdout, stderr)`.
fn run_steplock_with_global(dir: &Path, stdin: &str, global: &str) -> (i32, String, String) {
    let mut child = Command::new(STEPLOCK)
        .current_dir(dir)
        .env("STEPLOCK_GLOBAL_DIR", global)
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

/// Run a `steplock` subcommand with `STEPLOCK_GLOBAL_DIR` set to `global`.
fn run_subcommand_with_global(args: &[&str], dir: &Path, global: &Path) -> Output {
    Command::new(STEPLOCK)
        .args(args)
        .current_dir(dir)
        .env("STEPLOCK_GLOBAL_DIR", global)
        .output()
        .expect("failed to run steplock")
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

    let (_code, stdout, _stderr) = run_steplock(&subdir, &stdin);
    assert!(
        stdout.to_lowercase().contains("block") || stdout.contains("Did you check"),
        "should block even from subdirectory; got: {stdout}"
    );
}

// ── Global checklists ──────────────────────────────────────────────────────

/// Write a `git push` checklist named `name` into `steplock_dir/checklists/`.
fn global_checklist(steplock_dir: &Path, name: &str, question: &str) {
    let dir = steplock_dir.join("checklists").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.toml"),
        "on_event = \"tool:before\"\non_tool = \"bash\"\nmatch_input = \"input.command.contains('git push')\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("flow.mmd"),
        format!("stateDiagram-v2\n    [*] --> q\n    q --> [*]\n    q: {question}\n"),
    )
    .unwrap();
}

#[test]
fn hook_blocks_with_global_checklist_in_project_without_steplock() {
    let project = TempDir::new().unwrap();
    let global = TempDir::new().unwrap();
    global_checklist(global.path(), "push-gate", "Global push question?");

    let stdin = hook_event("bash", "git push origin main", "sess-g");
    let (code, stdout, _stderr) =
        run_steplock_with_global(project.path(), &stdin, global.path().to_str().unwrap());
    assert_eq!(code, 0, "block response exits 0");
    assert!(
        stdout.contains("Global push question?"),
        "expected global checklist block, got: {stdout}"
    );
    assert!(
        global
            .path()
            .join("sessions/sess-g/push-gate/state.json")
            .exists(),
        "global session state must live in the global dir"
    );
    assert!(
        !project.path().join(".steplock").exists(),
        "project dir must stay untouched"
    );
}

#[test]
fn hook_ignores_global_checklist_when_disabled() {
    let project = TempDir::new().unwrap();
    let stdin = hook_event("bash", "git push origin main", "sess-g");
    let (code, stdout, _stderr) = run_steplock_with_global(project.path(), &stdin, "");
    assert_eq!(code, 0, "approve exits 0");
    assert!(
        stdout.trim() == "{}" || stdout.is_empty(),
        "expected approve, got: {stdout}"
    );
}

#[test]
fn init_global_scaffolds_global_dir() {
    let global = TempDir::new().unwrap();
    let target = global.path().join("steplock");
    let output = run_subcommand_with_global(&["init", "--global"], global.path(), &target);
    assert!(output.status.success(), "init --global should succeed");
    assert!(
        target.join("checklists/example-gate/config.toml").exists(),
        "sample checklist expected in global dir"
    );
    assert!(
        !target.join(".gitignore").exists(),
        "global dir is not a repo; no .gitignore"
    );
}

#[test]
fn init_global_fails_when_disabled() {
    let dir = TempDir::new().unwrap();
    let output = run_subcommand_with_global(&["init", "--global"], dir.path(), Path::new(""));
    assert_eq!(
        output.status.code(),
        Some(1),
        "disabled global dir must fail"
    );
}

#[test]
fn validate_reports_invalid_global_checklist() {
    let project = TempDir::new().unwrap();
    let global = TempDir::new().unwrap();
    let bad = global.path().join("checklists/bad");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("config.toml"), "not valid toml").unwrap();
    fs::write(
        bad.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> s\n    s --> [*]\n    s: Step\n",
    )
    .unwrap();
    let output = run_subcommand_with_global(&["validate"], project.path(), global.path());
    assert_eq!(
        output.status.code(),
        Some(1),
        "invalid global checklist fails"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[global:bad/config.toml]"),
        "error label must name the global checklist, got: {stderr}"
    );
}

#[test]
fn clean_global_removes_global_sessions() {
    let global = TempDir::new().unwrap();
    let session = global.path().join("sessions/s1/gate");
    fs::create_dir_all(&session).unwrap();
    let output = run_subcommand_with_global(&["clean", "--global"], global.path(), global.path());
    assert!(output.status.success(), "clean --global should succeed");
    assert!(
        !global.path().join("sessions/s1").exists(),
        "global session dir must be removed"
    );
}

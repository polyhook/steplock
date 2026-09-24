//! Unit tests for `main`.
use super::*;
use clap::CommandFactory;
use polyhook::parse;
use std::fs;
use std::iter;
use tempfile::TempDir;

fn claude_stdin(cmd: &str, session: &str) -> String {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": cmd },
        "tool_output": {},
        "session_id": session
    })
    .to_string()
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
    [*] --> check
    check --> [*]
    check: Did you check?
",
    )
    .unwrap();
}

#[test]
fn polyhook_event_maps_correctly() {
    let stdin = claude_stdin("git push origin main", "s1");
    let ph_event = parse::parse_event(stdin.as_bytes()).unwrap();
    let event = polyhook_to_hook_event(ph_event);
    assert_eq!(event.event, "tool:before");
    assert_eq!(event.tool, "bash");
    assert_eq!(event.session_id, "s1");
    assert_eq!(event.caller, "claude-code");
    assert_eq!(
        event.input.get("command").and_then(|v| v.as_str()),
        Some("git push origin main")
    );
}

#[test]
fn run_app_approves_non_matching_command() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    let stdin = claude_stdin("ls -la", "s1");
    let resp = run_app(stdin.as_bytes(), tmp.path(), None).unwrap();
    assert!(matches!(resp, polyhook::HookResponse::ApproveResponse(_)));
}

#[test]
fn run_app_blocks_matching_command() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    let stdin = claude_stdin("git push origin main", "s1");
    let resp = run_app(stdin.as_bytes(), tmp.path(), None).unwrap();
    assert!(matches!(resp, polyhook::HookResponse::BlockResponse(_)));
}

#[test]
fn run_app_error_on_invalid_input() {
    let tmp = TempDir::new().unwrap();
    let err = run_app(b"not valid json".as_ref(), tmp.path(), None);
    assert!(err.is_err());
    assert!(err
        .unwrap_err()
        .contains("steplock: failed to read hook input"));
}

#[test]
fn run_app_error_on_invalid_cel_expression() {
    let tmp = TempDir::new().unwrap();
    let cl_dir = tmp.path().join(".steplock/checklists/bad-gate");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("config.toml"),
        r#"on_event = "tool:before"
on_tool = "bash"
match_input = "!!!invalid cel!!!"
reset = "session"
"#,
    )
    .unwrap();
    fs::write(
        cl_dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> check\n    check --> [*]\n    check: Check\n",
    )
    .unwrap();
    let stdin = claude_stdin("anything", "s1");
    let err = run_app(stdin.as_bytes(), tmp.path(), None);
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("steplock: error:"));
}

#[test]
fn find_repo_root_finds_steplock_dir() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join(".steplock")).unwrap();
    let root = find_repo_root_from(tmp.path()).unwrap();
    assert_eq!(root, tmp.path());
}

#[test]
fn find_repo_root_walks_up() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join(".steplock")).unwrap();
    let subdir = tmp.path().join("a/b/c");
    fs::create_dir_all(&subdir).unwrap();
    let root = find_repo_root_from(&subdir).unwrap();
    assert_eq!(root, tmp.path());
}

#[test]
fn find_repo_root_returns_none_when_not_found() {
    let tmp = TempDir::new().unwrap();
    let result = find_repo_root_from(tmp.path());
    assert!(result.is_none());
}

#[test]
fn init_creates_checklists_dir_and_gitignore() {
    let tmp = TempDir::new().unwrap();
    run_init(tmp.path()).unwrap();
    assert!(tmp.path().join(".steplock/checklists").is_dir());
    let gitignore = fs::read_to_string(tmp.path().join(".steplock/.gitignore")).unwrap();
    assert!(gitignore.contains("sessions/"));
    assert!(gitignore.contains("audit.log"));
}

#[test]
fn init_scaffolds_sample_checklist() {
    let tmp = TempDir::new().unwrap();
    run_init(tmp.path()).unwrap();
    let sample = tmp.path().join(".steplock/checklists/example-gate");
    assert!(sample.join("config.toml").exists());
    assert!(sample.join("flow.mmd").exists());
    let cfg = fs::read_to_string(sample.join("config.toml")).unwrap();
    assert!(cfg.contains("git push"));
    let flow = fs::read_to_string(sample.join("flow.mmd")).unwrap();
    assert!(flow.contains("stateDiagram-v2"));
}

#[test]
fn init_sample_checklist_is_valid() {
    let tmp = TempDir::new().unwrap();
    run_init(tmp.path()).unwrap();
    let stdin = claude_stdin("git push origin main", "s1");
    let resp = run_app(stdin.as_bytes(), tmp.path(), None).unwrap();
    assert!(matches!(resp, polyhook::HookResponse::BlockResponse(_)));
}

#[test]
fn init_is_idempotent_when_checklists_exists() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".steplock/checklists")).unwrap();
    run_init(tmp.path()).unwrap();
}

#[test]
fn clean_removes_session_dirs() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".steplock/sessions/sess-abc/gate")).unwrap();
    fs::create_dir_all(tmp.path().join(".steplock/sessions/sess-xyz/gate")).unwrap();
    run_clean(tmp.path()).unwrap();
    assert!(!tmp.path().join(".steplock/sessions/sess-abc").exists());
    assert!(!tmp.path().join(".steplock/sessions/sess-xyz").exists());
}

#[test]
fn clean_removes_fallback_id_file() {
    let tmp = TempDir::new().unwrap();
    let sessions = tmp.path().join(".steplock/sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join("fallback-id"), "some-uuid").unwrap();
    run_clean(tmp.path()).unwrap();
    assert!(!sessions.join("fallback-id").exists());
}

#[test]
fn clean_is_noop_when_no_sessions_dir() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".steplock/checklists")).unwrap();
    run_clean(tmp.path()).unwrap();
}

#[test]
fn clean_is_noop_when_no_steplock_dir() {
    let tmp = TempDir::new().unwrap();
    run_clean(tmp.path()).unwrap();
}

#[test]
fn clean_leaves_sessions_dir_intact() {
    let tmp = TempDir::new().unwrap();
    let sessions = tmp.path().join(".steplock/sessions");
    fs::create_dir_all(sessions.join("sess-1/gate")).unwrap();
    run_clean(tmp.path()).unwrap();
    assert!(sessions.exists());
}

#[test]
fn validate_returns_true_when_no_checklists_dir() {
    let tmp = TempDir::new().unwrap();
    assert!(run_validate(tmp.path(), None).unwrap());
}

#[test]
fn validate_returns_true_when_checklists_empty() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".steplock/checklists")).unwrap();
    assert!(run_validate(tmp.path(), None).unwrap());
}

#[test]
fn validate_returns_true_for_valid_checklist() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    assert!(run_validate(tmp.path(), None).unwrap());
}

#[test]
fn validate_returns_false_when_config_toml_missing() {
    let tmp = TempDir::new().unwrap();
    let cl_dir = tmp.path().join(".steplock/checklists/no-config");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> s\n    s --> [*]\n    s: Step\n",
    )
    .unwrap();
    assert!(!run_validate(tmp.path(), None).unwrap());
}

#[test]
fn validate_returns_false_when_flow_mmd_missing() {
    let tmp = TempDir::new().unwrap();
    let cl_dir = tmp.path().join(".steplock/checklists/no-flow");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("config.toml"),
        "on_event = \"tool:before\"\nreset = \"session\"\n",
    )
    .unwrap();
    assert!(!run_validate(tmp.path(), None).unwrap());
}

#[test]
fn validate_returns_false_for_invalid_config_toml() {
    let tmp = TempDir::new().unwrap();
    let cl_dir = tmp.path().join(".steplock/checklists/bad-config");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(cl_dir.join("config.toml"), "not valid toml !!!").unwrap();
    fs::write(
        cl_dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> s\n    s --> [*]\n    s: Step\n",
    )
    .unwrap();
    assert!(!run_validate(tmp.path(), None).unwrap());
}

#[test]
fn validate_returns_false_for_invalid_flow_mmd() {
    let tmp = TempDir::new().unwrap();
    let cl_dir = tmp.path().join(".steplock/checklists/bad-flow");
    fs::create_dir_all(&cl_dir).unwrap();
    fs::write(
        cl_dir.join("config.toml"),
        "on_event = \"tool:before\"\nreset = \"session\"\n",
    )
    .unwrap();
    fs::write(cl_dir.join("flow.mmd"), "stateDiagram-v2\n    a --> b\n").unwrap();
    assert!(!run_validate(tmp.path(), None).unwrap());
}

#[test]
fn validate_continues_checking_all_checklists_after_failure() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    let bad_dir = tmp.path().join(".steplock/checklists/0-bad");
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(bad_dir.join("config.toml"), "not valid").unwrap();
    fs::write(
        bad_dir.join("flow.mmd"),
        "stateDiagram-v2\n    [*] --> s\n    s --> [*]\n    s: Step\n",
    )
    .unwrap();
    assert!(!run_validate(tmp.path(), None).unwrap());
}

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(iter::once("steplock").chain(args.iter().copied())).unwrap()
}

#[test]
fn cli_no_args_runs_hook() {
    assert!(
        parse(&[]).command.is_none(),
        "no subcommand means hook mode"
    );
}

#[test]
fn cli_parses_init_and_global_flag() {
    assert!(
        matches!(
            parse(&["init"]).command,
            Some(CliCommand::Init { global: false })
        ),
        "plain init"
    );
    assert!(
        matches!(
            parse(&["init", "--global"]).command,
            Some(CliCommand::Init { global: true })
        ),
        "init --global"
    );
}

#[test]
fn cli_parses_validate_and_clean() {
    assert!(
        matches!(parse(&["validate"]).command, Some(CliCommand::Validate)),
        "validate"
    );
    assert!(
        matches!(
            parse(&["clean", "--global"]).command,
            Some(CliCommand::Clean { global: true })
        ),
        "clean --global"
    );
}

#[test]
fn cli_rejects_unknown_arguments() {
    let err = Cli::try_parse_from(["steplock", "--unknown-flag"]).unwrap_err();
    assert!(err.use_stderr(), "usage errors go to stderr and exit 1");
}

#[test]
fn cli_help_and_version_are_not_errors() {
    for flag in ["--help", "-h", "--version", "-V"] {
        let err = Cli::try_parse_from(["steplock", flag]).unwrap_err();
        assert!(!err.use_stderr(), "{flag} prints to stdout and exits 0");
    }
}

#[test]
fn cli_definition_is_valid() {
    Cli::command().debug_assert();
}

fn hermes_stdin(cmd: &str, session: &str) -> String {
    serde_json::json!({
        "hook_event_name": "pre_tool_call",
        "tool_name": "terminal",
        "tool_input": { "command": cmd },
        "session_id": session,
        "cwd": "/tmp/project",
        "extra": { "task_id": "t1", "tool_call_id": "c1" }
    })
    .to_string()
}

/// Serialize through the same path `run_hook` uses, so the assertion covers the caller
/// context that `run_app` records while reading stdin.
fn respond_json(resp: &polyhook::HookResponse) -> serde_json::Value {
    let mut buf = Vec::new();
    polyhook::respond_to(&mut buf, resp).unwrap();
    serde_json::from_slice(&buf).unwrap()
}

#[test]
fn claude_code_block_denies_the_tool_call_not_the_session() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    let stdin = claude_stdin("git push origin main", "s1");
    let json = respond_json(&run_app(stdin.as_bytes(), tmp.path(), None).unwrap());
    assert!(
        json.get("decision").is_none(),
        "legacy session-level block: {json}"
    );
    let decision = json
        .get("hookSpecificOutput")
        .and_then(|o| o.get("permissionDecision"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(
        decision,
        Some("deny"),
        "expected PreToolUse deny, got {json}"
    );
}

#[test]
fn hermes_terminal_command_blocks_in_hermes_format() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    let stdin = hermes_stdin("git push origin main", "hermes-s1");
    let json = respond_json(&run_app(stdin.as_bytes(), tmp.path(), None).unwrap());
    assert_eq!(
        json.get("action").and_then(serde_json::Value::as_str),
        Some("block"),
        "expected Hermes block, got {json}"
    );
    let message = json
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        message.contains("ack.sh"),
        "block message must carry the ack command: {json}"
    );
    assert!(
        tmp.path()
            .join(".steplock/sessions/hermes-s1/quality-gate/state.json")
            .exists(),
        "Hermes session_id must scope the session state"
    );
}

#[test]
fn hermes_non_matching_command_approves() {
    let tmp = TempDir::new().unwrap();
    setup_checklist(tmp.path());
    let stdin = hermes_stdin("ls -la", "hermes-s1");
    let json = respond_json(&run_app(stdin.as_bytes(), tmp.path(), None).unwrap());
    assert_eq!(
        json,
        serde_json::json!({}),
        "Hermes approve is an empty object"
    );
}

#[test]
fn hermes_session_end_cleans_global_session() {
    let project = TempDir::new().unwrap();
    let global = TempDir::new().unwrap();
    setup_checklist(global.path().join("x").as_path());
    // Global dir layout is `<global>/checklists/`, so move the sample under it.
    fs::rename(global.path().join("x/.steplock"), global.path().join("g")).unwrap();
    let global_dir = global.path().join("g");

    let push = hermes_stdin("git push", "hermes-s2");
    run_app(push.as_bytes(), project.path(), Some(&global_dir)).unwrap();
    assert!(
        global_dir.join("sessions/hermes-s2").exists(),
        "global session created"
    );

    let end = serde_json::json!({
        "hook_event_name": "on_session_end",
        "session_id": "hermes-s2",
        "cwd": "/tmp/project",
        "extra": {}
    })
    .to_string();
    run_app(end.as_bytes(), project.path(), Some(&global_dir)).unwrap();
    assert!(
        !global_dir.join("sessions/hermes-s2").exists(),
        "on_session_end must clean the global session"
    );
}

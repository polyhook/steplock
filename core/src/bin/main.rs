//! `steplock` CLI binary — reads polyhook events from stdin and enforces quality-gate checklists.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

use polyhook::parse;
use steplock::{global_steplock_dir, run_with_global, HookEvent, HookResponse};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("steplock {}", env!("CARGO_PKG_VERSION"));
        }
        [flag] if flag == "--help" || flag == "-h" => {
            print_help();
        }
        [cmd] if cmd == "init" => {
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if let Err(e) = run_init(&cwd) {
                eprintln!("steplock: init failed: {e}");
                process::exit(1);
            }
        }
        [cmd, flag] if cmd == "init" && flag == "--global" => {
            if let Err(e) = init_steplock_dir(&require_global_dir(), false) {
                eprintln!("steplock: init failed: {e}");
                process::exit(1);
            }
        }
        [cmd] if cmd == "validate" => {
            let dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let root = find_repo_root_from(&dir).unwrap_or(dir);
            match run_validate(&root, global_steplock_dir().as_deref()) {
                Ok(true) => {}
                Ok(false) => process::exit(1),
                Err(e) => {
                    eprintln!("steplock: validate failed: {e}");
                    process::exit(1);
                }
            }
        }
        [cmd] if cmd == "clean" => {
            if let Err(e) = run_clean(&env::current_dir().unwrap_or_else(|_| PathBuf::from("."))) {
                eprintln!("steplock: clean failed: {e}");
                process::exit(1);
            }
        }
        [cmd, flag] if cmd == "clean" && flag == "--global" => {
            if let Err(e) = clean_sessions(&require_global_dir()) {
                eprintln!("steplock: clean failed: {e}");
                process::exit(1);
            }
        }
        [] => run_hook(),
        _ => {
            eprintln!("steplock: unknown arguments");
            eprintln!("Run 'steplock --help' for usage.");
            process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        "steplock {}

Stateful quality gate for AI coding agents.

USAGE:
    steplock               Read hook event from stdin and respond (used by polyhook)
    steplock init          Create .steplock/checklists/ in the current directory
    steplock init --global Create checklists/ in the global steplock directory
    steplock validate      Check all project and global checklist configs for errors
    steplock clean         Remove all session state (forces checklists to restart)
    steplock clean --global
                           Remove all session state in the global steplock directory
    steplock --version     Print version

CHECKLIST FILES:
    .steplock/checklists/<name>/config.toml   Gate trigger and reset configuration
    .steplock/checklists/<name>/flow.mmd      Mermaid stateDiagram-v2 checklist flow

GLOBAL CHECKLISTS:
    Checklists in <global>/checklists/<name>/ apply to every project. They run after
    the project checklists. A project checklist with the same name replaces the global one.
    <global> is $STEPLOCK_GLOBAL_DIR, else $XDG_CONFIG_HOME/steplock, else
    ~/.config/steplock. Set STEPLOCK_GLOBAL_DIR=\"\" to turn global checklists off.

For more information: https://github.com/polyhook/steplock",
        env!("CARGO_PKG_VERSION")
    );
}

/// Validate all checklists in `.steplock/checklists/` and in the global steplock directory.
/// Returns `Ok(true)` if all valid, `Ok(false)` if any checklist failed validation (errors
/// already printed), or `Err` on I/O.
fn run_validate(repo_root: &Path, global_dir: Option<&Path>) -> io::Result<bool> {
    let project_ok = validate_dir(&repo_root.join(".steplock").join("checklists"), "")?;
    let global_ok = match global_dir {
        Some(global) => validate_dir(&global.join("checklists"), "global")?,
        None => true,
    };
    Ok(project_ok && global_ok)
}

/// Validate one `checklists/` directory. `scope` names it in messages (`""` or `"global"`).
fn validate_dir(checklists_dir: &Path, scope: &str) -> io::Result<bool> {
    let shown = checklists_dir.display();
    let (words, label_prefix) = if scope.is_empty() {
        (String::new(), String::new())
    } else {
        (format!("{scope} "), format!("{scope}:"))
    };
    if !checklists_dir.exists() {
        println!("steplock: no {words}checklists found at {shown}");
        return Ok(true);
    }

    let errors = steplock::validate_checklists(checklists_dir);
    if errors.is_empty() {
        println!("steplock: all {words}checklists valid ({shown})");
        Ok(true)
    } else {
        for (label, err) in &errors {
            eprintln!("steplock: [{label_prefix}{label}] error: {err}");
        }
        Ok(false)
    }
}

/// Global steplock directory, or exit with an error when it is disabled or unknown.
fn require_global_dir() -> PathBuf {
    global_steplock_dir().unwrap_or_else(|| {
        eprintln!(
            "steplock: no global steplock directory \
             (set STEPLOCK_GLOBAL_DIR, XDG_CONFIG_HOME or HOME)"
        );
        process::exit(1);
    })
}

fn run_hook() {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo_root = find_repo_root_from(&cwd).unwrap_or(cwd);

    let global_dir = global_steplock_dir();
    let response = match run_app(io::stdin(), &repo_root, global_dir.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            process::exit(2);
        }
    };

    if let Err(e) = polyhook::respond(&response) {
        eprintln!("steplock: failed to write response: {e}");
        process::exit(2);
    }
}

const SAMPLE_CONFIG: &str = r#"on_event = "tool:before"
on_tool = "bash"
match_input = "input.command.contains('git push')"
reset = "session"
"#;

const SAMPLE_FLOW: &str = "stateDiagram-v2\n    [*] --> tests_pass\n    tests_pass --> reviewed\n    reviewed --> [*]\n    tests_pass : Tests pass locally\n    reviewed : Code reviewed\n";

/// Create `.steplock/checklists/` and a `.steplock/.gitignore` in `dir`.
/// Also writes a ready-to-use sample checklist so `git push` is blocked immediately.
fn run_init(dir: &Path) -> io::Result<()> {
    init_steplock_dir(&dir.join(".steplock"), true)
}

/// Create `checklists/` with a sample checklist in `steplock_dir`.
/// With `gitignore`, also writes a `.gitignore` for session state and the audit log.
fn init_steplock_dir(steplock_dir: &Path, gitignore: bool) -> io::Result<()> {
    let checklists_dir = steplock_dir.join("checklists");
    if checklists_dir.exists() {
        println!("steplock: {} already exists", checklists_dir.display());
        return Ok(());
    }
    fs::create_dir_all(&checklists_dir)?;
    if gitignore {
        fs::write(steplock_dir.join(".gitignore"), "sessions/\naudit.log\n")?;
    }
    let sample_dir = checklists_dir.join("example-gate");
    fs::create_dir_all(&sample_dir)?;
    fs::write(sample_dir.join("config.toml"), SAMPLE_CONFIG)?;
    fs::write(sample_dir.join("flow.mmd"), SAMPLE_FLOW)?;
    println!("steplock: initialized {}", checklists_dir.display());
    println!(
        "A sample checklist was written to {}.",
        sample_dir.display()
    );
    println!("It will block `git push` until two quality checks are acknowledged.");
    println!("Edit config.toml and flow.mmd to customize it, or add more checklists.");
    Ok(())
}

/// Remove all session directories under `.steplock/sessions/`.
///
/// AI agent sessions that crash or are killed never fire `session:stop`, so their
/// session directories accumulate indefinitely. `steplock clean` flushes them all.
/// The next hook invocation will start each checklist fresh.
fn run_clean(dir: &Path) -> io::Result<()> {
    let Some(root) = find_repo_root_from(dir) else {
        println!("steplock: no .steplock/ directory found — nothing to clean");
        return Ok(());
    };
    clean_sessions(&root.join(".steplock"))
}

/// Remove every session directory and the fallback id under `<steplock_dir>/sessions/`.
fn clean_sessions(steplock_dir: &Path) -> io::Result<()> {
    let sessions_dir = steplock_dir.join("sessions");
    if !sessions_dir.exists() {
        println!("steplock: no sessions to clean");
        return Ok(());
    }
    let mut removed = 0u32;
    for entry in fs::read_dir(&sessions_dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
            removed += 1;
        } else {
            fs::remove_file(&path)?;
        }
    }
    if removed == 0 {
        println!("steplock: no sessions to clean");
    } else {
        println!("steplock: removed {removed} session(s)");
    }
    Ok(())
}

/// Parse the hook event from `reader`, run the gate, and return the polyhook response.
/// Returns `Err(message)` when input is unreadable or the gate engine fails.
fn run_app(
    mut reader: impl Read,
    repo_root: &Path,
    global_dir: Option<&Path>,
) -> Result<polyhook::HookResponse, String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("steplock: failed to read hook input: {e}"))?;

    let ph_event = parse::parse_event(&bytes)
        .map_err(|e| format!("steplock: failed to read hook input: {e}"))?;

    let event = polyhook_to_hook_event(ph_event);

    match run_with_global(&event, repo_root, global_dir) {
        Ok(HookResponse::Block { message }) => Ok(polyhook::HookResponse::block(&message)),
        Ok(_) => Ok(polyhook::HookResponse::approve()),
        Err(e) => Err(format!("steplock: error: {e}")),
    }
}

fn polyhook_to_hook_event(e: polyhook::HookEvent) -> HookEvent {
    HookEvent::new(
        e.event.to_string(),
        e.tool.unwrap_or_default(),
        e.input.map(|m| m.into_iter().collect()).unwrap_or_default(),
        e.output
            .map(|m| m.into_iter().collect())
            .unwrap_or_default(),
        e.session_id,
        e.caller.to_string(),
    )
}

/// Walk up from `start` looking for a directory containing `.steplock/`.
fn find_repo_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".steplock").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
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
}

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

use steplock_core::{run, HookEvent, HookResponse};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("steplock {}", env!("CARGO_PKG_VERSION"));
        }
        [cmd] if cmd == "init" => {
            if let Err(e) = run_init(&env::current_dir().unwrap()) {
                eprintln!("steplock: init failed: {e}");
                process::exit(1);
            }
        }
        [cmd] if cmd == "clean" => {
            if let Err(e) = run_clean(&env::current_dir().unwrap()) {
                eprintln!("steplock: clean failed: {e}");
                process::exit(1);
            }
        }
        [] => run_hook(),
        _ => {
            eprintln!("steplock: unknown arguments");
            eprintln!("Usage: steplock [--version | init | clean]");
            process::exit(1);
        }
    }
}

fn run_hook() {
    let repo_root = find_repo_root_from(&env::current_dir().unwrap())
        .unwrap_or_else(|| env::current_dir().unwrap());

    let response = match run_app(std::io::stdin(), &repo_root) {
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

/// Create `.steplock/checklists/` and a `.steplock/.gitignore` in `dir`.
fn run_init(dir: &Path) -> std::io::Result<()> {
    let checklists_dir = dir.join(".steplock").join("checklists");
    if checklists_dir.exists() {
        println!("steplock: .steplock/checklists/ already exists");
        return Ok(());
    }
    fs::create_dir_all(&checklists_dir)?;
    fs::write(
        dir.join(".steplock").join(".gitignore"),
        "sessions/\naudit.log\n",
    )?;
    println!("steplock: initialized .steplock/checklists/");
    println!("Next: create a checklist in .steplock/checklists/<name>/");
    println!("      with config.toml and flow.mmd.");
    Ok(())
}

/// Remove all session directories under `.steplock/sessions/`.
///
/// AI agent sessions that crash or are killed never fire `session:stop`, so their
/// session directories accumulate indefinitely. `steplock clean` flushes them all.
/// The next hook invocation will start each checklist fresh.
fn run_clean(dir: &Path) -> std::io::Result<()> {
    let steplock_dir = match find_repo_root_from(dir) {
        Some(root) => root.join(".steplock"),
        None => {
            println!("steplock: no .steplock/ directory found — nothing to clean");
            return Ok(());
        }
    };
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
            // Remove fallback-id and any other stale files
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
fn run_app(mut reader: impl Read, repo_root: &Path) -> Result<polyhook::HookResponse, String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("steplock: failed to read hook input: {e}"))?;

    let ph_event = polyhook::parse::parse_event(&bytes)
        .map_err(|e| format!("steplock: failed to read hook input: {e}"))?;

    let event = polyhook_to_hook_event(ph_event);

    match run(&event, repo_root) {
        Ok(HookResponse::Block { message }) => Ok(polyhook::HookResponse::block(&message)),
        Ok(_) => Ok(polyhook::HookResponse::approve()),
        Err(e) => Err(format!("steplock: error: {e}")),
    }
}

fn polyhook_to_hook_event(e: polyhook::HookEvent) -> HookEvent {
    HookEvent {
        event: e.event.to_string(),
        tool: e.tool.unwrap_or_default(),
        input: e.input.map(|m| m.into_iter().collect()).unwrap_or_default(),
        output: e
            .output
            .map(|m| m.into_iter().collect())
            .unwrap_or_default(),
        session_id: e.session_id,
        caller: e.caller.to_string(),
    }
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
            r#"stateDiagram-v2
    [*] --> check
    check --> [*]
    check: Did you check?
"#,
        )
        .unwrap();
    }

    #[test]
    fn polyhook_event_maps_correctly() {
        let stdin = claude_stdin("git push origin main", "s1");
        let ph_event = polyhook::parse::parse_event(stdin.as_bytes()).unwrap();
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
        let resp = run_app(stdin.as_bytes(), tmp.path()).unwrap();
        assert!(matches!(resp, polyhook::HookResponse::ApproveResponse(_)));
    }

    #[test]
    fn run_app_blocks_matching_command() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let stdin = claude_stdin("git push origin main", "s1");
        let resp = run_app(stdin.as_bytes(), tmp.path()).unwrap();
        assert!(matches!(resp, polyhook::HookResponse::BlockResponse(_)));
    }

    #[test]
    fn run_app_error_on_invalid_input() {
        let tmp = TempDir::new().unwrap();
        let err = run_app(b"not valid json".as_ref(), tmp.path());
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
        let err = run_app(stdin.as_bytes(), tmp.path());
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
    fn init_is_idempotent_when_checklists_exists() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".steplock/checklists")).unwrap();
        // Second call should not error
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
        // No sessions/ dir — should succeed without error
        run_clean(tmp.path()).unwrap();
    }

    #[test]
    fn clean_is_noop_when_no_steplock_dir() {
        let tmp = TempDir::new().unwrap();
        // No .steplock/ at all — should succeed without error
        run_clean(tmp.path()).unwrap();
    }

    #[test]
    fn clean_leaves_sessions_dir_intact() {
        let tmp = TempDir::new().unwrap();
        let sessions = tmp.path().join(".steplock/sessions");
        fs::create_dir_all(sessions.join("sess-1/gate")).unwrap();
        run_clean(tmp.path()).unwrap();
        // sessions/ dir itself remains (only contents removed)
        assert!(sessions.exists());
    }
}

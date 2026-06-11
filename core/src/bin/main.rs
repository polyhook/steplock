use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

use steplock_core::{run, HookEvent, HookResponse};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            print_help();
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("steplock {VERSION}");
        }
        [cmd] if cmd == "init" => {
            let cwd = env::current_dir().unwrap();
            if let Err(e) = cmd_init(&cwd) {
                eprintln!("steplock init: {e}");
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
        "steplock {VERSION}
Stateful quality gates for AI coding agent tool calls.

USAGE:
    steplock [COMMAND]

    With no arguments, reads a polyhook event from stdin and responds.

COMMANDS:
    init    Create .steplock/checklists/ directory in the current repo

OPTIONS:
    -h, --help       Print this help
    -V, --version    Print version

DOCUMENTATION:
    https://github.com/polyhook/steplock"
    );
}

fn cmd_init(root: &Path) -> std::io::Result<()> {
    let checklists = root.join(".steplock/checklists");
    fs::create_dir_all(&checklists)?;
    println!("steplock: created {}", checklists.display());
    Ok(())
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
        Ok(HookResponse::Approve) => Ok(polyhook::HookResponse::approve()),
        Ok(HookResponse::Block { message }) => Ok(polyhook::HookResponse::block(&message)),
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
    fn version_string_is_nonempty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn print_help_does_not_panic() {
        print_help();
    }

    #[test]
    fn cmd_init_creates_checklists_dir() {
        let tmp = TempDir::new().unwrap();
        cmd_init(tmp.path()).unwrap();
        assert!(tmp.path().join(".steplock/checklists").is_dir());
    }

    #[test]
    fn cmd_init_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        cmd_init(tmp.path()).unwrap();
        cmd_init(tmp.path()).unwrap();
        assert!(tmp.path().join(".steplock/checklists").is_dir());
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
}

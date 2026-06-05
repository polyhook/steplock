use std::env;
use std::path::{Path, PathBuf};
use std::process;

use steplock_core::{run, HookEvent, HookResponse};

fn main() {
    let repo_root = find_repo_root_from(&env::current_dir().unwrap())
        .unwrap_or_else(|| env::current_dir().unwrap());

    let ph_event = match polyhook::read() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("steplock: failed to read hook input: {e}");
            process::exit(2);
        }
    };

    let event = polyhook_to_hook_event(ph_event);

    let response = match run(&event, &repo_root) {
        Ok(HookResponse::Approve) => polyhook::HookResponse::approve(),
        Ok(HookResponse::Block { message }) => polyhook::HookResponse::block(&message),
        Err(e) => {
            eprintln!("steplock: error: {e}");
            process::exit(2);
        }
    };

    if let Err(e) = polyhook::respond(&response) {
        eprintln!("steplock: failed to write response: {e}");
        process::exit(2);
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

    fn make_claude_stdin(cmd: &str, session: &str) -> String {
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
        let stdin = make_claude_stdin("git push origin main", "s1");
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
    fn approves_non_matching_command() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let stdin = make_claude_stdin("ls -la", "s1");
        let ph_event = polyhook::parse::parse_event(stdin.as_bytes()).unwrap();
        let event = polyhook_to_hook_event(ph_event);
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Approve));
    }

    #[test]
    fn blocks_matching_command() {
        let tmp = TempDir::new().unwrap();
        setup_checklist(tmp.path());
        let stdin = make_claude_stdin("git push origin main", "s1");
        let ph_event = polyhook::parse::parse_event(stdin.as_bytes()).unwrap();
        let event = polyhook_to_hook_event(ph_event);
        let resp = run(&event, tmp.path()).unwrap();
        assert!(matches!(resp, HookResponse::Block { .. }));
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
}

//! Minimal example: evaluate a single hook event against a `.steplock/` tree.
//!
//! Run with:
//! ```bash
//! cargo run --manifest-path core/Cargo.toml --example basic_gate
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use steplock_core::{HookEvent, HookResponse};

fn main() {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();

    write_checklist(root);

    let event = HookEvent {
        event: "tool:before".to_owned(),
        tool: "bash".to_owned(),
        input: {
            let mut m = HashMap::new();
            m.insert(
                "command".to_owned(),
                serde_json::Value::String("git push origin main".to_owned()),
            );
            m
        },
        output: HashMap::new(),
        session_id: "example-session".to_owned(),
        caller: "claude-code".to_owned(),
    };

    match steplock_core::run(&event, root).expect("run failed") {
        HookResponse::Block { message } => {
            println!("BLOCKED\n{message}");
        }
        HookResponse::Approve => {
            println!("APPROVED — no active gate");
        }
    }
}

fn write_checklist(root: &Path) {
    let dir = root.join(".steplock/checklists/pre-push");
    fs::create_dir_all(&dir).unwrap();

    fs::write(
        dir.join("config.toml"),
        r#"on_event = "tool:before"
on_tool  = "bash"
match_input = "input.command.contains('git push')"
reset = "session"
"#,
    )
    .unwrap();

    fs::write(
        dir.join("flow.mmd"),
        r#"stateDiagram-v2
    [*] --> tests_pass
    tests_pass --> [*]
    tests_pass : Did cargo test pass?
"#,
    )
    .unwrap();
}

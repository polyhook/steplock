//! `steplock` CLI binary — reads polyhook events from stdin and enforces quality-gate checklists.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, Subcommand};
use steplock::{global_steplock_dir, run_with_global, HookEvent, HookResponse};

/// Extra help text shown after the generated command list.
const AFTER_HELP: &str = "\
With no command, steplock reads a hook event from stdin and responds (used by polyhook).

CHECKLIST FILES:
    .steplock/checklists/<name>/config.toml   Gate trigger and reset configuration
    .steplock/checklists/<name>/flow.mmd      Mermaid stateDiagram-v2 checklist flow

GLOBAL CHECKLISTS:
    Checklists in <global>/checklists/<name>/ apply to every project. They run after
    the project checklists. A project checklist with the same name replaces the global one.
    <global> is $STEPLOCK_GLOBAL_DIR, else $XDG_CONFIG_HOME/steplock, else
    ~/.config/steplock. Set STEPLOCK_GLOBAL_DIR=\"\" to turn global checklists off.

For more information: https://github.com/polyhook/steplock";

/// Stateful quality gate for AI coding agents.
#[derive(Debug, Parser)]
#[command(name = "steplock", version, after_help = AFTER_HELP)]
struct Cli {
    /// Command to run. Omit it to handle a hook event from stdin.
    #[command(subcommand)]
    command: Option<CliCommand>,
}

/// `steplock` subcommands.
#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Create .steplock/checklists/ with a sample checklist in the current directory
    Init {
        /// Create checklists/ in the global steplock directory instead
        #[arg(long)]
        global: bool,
    },
    /// Check all project and global checklist configs for errors
    Validate,
    /// Remove all session state (forces checklists to restart)
    Clean {
        /// Remove session state in the global steplock directory instead
        #[arg(long)]
        global: bool,
    },
}

fn main() {
    let cli = Cli::try_parse().unwrap_or_else(|e| {
        // Help and version go to stdout and exit 0; usage errors exit 1 (not clap's 2,
        // which steplock reserves for hook failures).
        let code = i32::from(e.use_stderr());
        let _: io::Result<()> = e.print();
        process::exit(code);
    });
    match cli.command {
        None => run_hook(),
        Some(CliCommand::Init { global: false }) => {
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            exit_on_error("init", run_init(&cwd));
        }
        Some(CliCommand::Init { global: true }) => {
            exit_on_error("init", init_steplock_dir(&require_global_dir(), false));
        }
        Some(CliCommand::Validate) => {
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
        Some(CliCommand::Clean { global: false }) => {
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            exit_on_error("clean", run_clean(&cwd));
        }
        Some(CliCommand::Clean { global: true }) => {
            exit_on_error("clean", clean_sessions(&require_global_dir()));
        }
    }
}

/// Print `steplock: <command> failed: <error>` and exit 1 when `result` is an error.
fn exit_on_error(command: &str, result: io::Result<()>) {
    if let Err(e) = result {
        eprintln!("steplock: {command} failed: {e}");
        process::exit(1);
    }
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
    // Read through `polyhook::read_from`, not `parse::parse_event`: reading records the
    // detected caller (Claude Code, Hermes, Cursor, ...) that `polyhook::respond` needs to
    // answer in that agent's own wire format. Parsing raw bytes leaves it unset, so every
    // response falls back to the legacy Claude Code shape.
    let ph_event = polyhook::read_from(&mut reader)
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
#[path = "main_tests.rs"]
mod tests;

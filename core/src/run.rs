//! Core gate logic: evaluates checklists against incoming hook events.
use std::fs;
use std::path::Path;

use crate::catalog::checklist_dirs;
use crate::error::Result;
use crate::gate::evaluate_checklist;
use crate::state::{HookEvent, HookResponse};

/// Run the full steplock gate logic against the project checklists only.
///
/// `repo_root` — the directory that contains `.steplock/`.
/// Returns `HookResponse::Approve` if no checklist blocks, or
/// `HookResponse::Block { message }` with the gate message.
///
/// Equivalent to [`run_with_global`] with no global steplock directory.
///
/// # Errors
///
/// Returns `Err` on I/O failures (reading checklist files, writing state) or on invalid
/// checklist configuration (bad TOML, invalid Mermaid, invalid CEL expression).
pub fn run(event: &HookEvent, repo_root: &Path) -> Result<HookResponse> {
    run_with_global(event, repo_root, None)
}

/// Run the gate logic against the project checklists, then the global checklists.
///
/// `repo_root` — the directory that contains the project `.steplock/`.
/// `global_dir` — a steplock directory shared by every project (see
/// [`crate::global_config::global_steplock_dir`]). It has the same layout as `.steplock/`:
/// `checklists/`, `sessions/` and `audit.log`.
///
/// Project checklists are evaluated first. A global checklist is skipped when the project
/// has a checklist with the same name, so a project can override or disable it. Session
/// state for a global checklist lives in `global_dir`, not in the project.
///
/// # Errors
///
/// Returns `Err` on I/O failures (reading checklist files, writing state) or on invalid
/// checklist configuration (bad TOML, invalid Mermaid, invalid CEL expression).
pub fn run_with_global(
    event: &HookEvent,
    repo_root: &Path,
    global_dir: Option<&Path>,
) -> Result<HookResponse> {
    let project_dir = repo_root.join(".steplock");
    let global_dir =
        global_dir.filter(|g| !same_file::is_same_file(g, &project_dir).unwrap_or(false));

    if event.event == "session:stop" {
        cleanup_session(&project_dir, &event.session_id)?;
        if let Some(global) = global_dir {
            cleanup_session(global, &event.session_id)?;
        }
        return Ok(HookResponse::Approve);
    }

    let project_checklists = checklist_dirs(&project_dir)?;
    for checklist_dir in &project_checklists {
        if let Some(resp) = evaluate_checklist(event, &project_dir, checklist_dir)? {
            return Ok(resp);
        }
    }

    if let Some(global) = global_dir {
        let project_names: Vec<_> = project_checklists
            .iter()
            .filter_map(|p| p.file_name())
            .collect();
        for checklist_dir in checklist_dirs(global)? {
            let shadowed = checklist_dir
                .file_name()
                .is_some_and(|n| project_names.contains(&n));
            if shadowed {
                continue;
            }
            if let Some(resp) = evaluate_checklist(event, global, &checklist_dir)? {
                return Ok(resp);
            }
        }
    }

    Ok(HookResponse::Approve)
}

fn cleanup_session(steplock_dir: &Path, session_id: &str) -> Result<()> {
    if !steplock_dir.exists() {
        return Ok(());
    }
    let scope_key = if session_id.is_empty() {
        let fallback_path = steplock_dir.join("sessions").join("fallback-id");
        if !fallback_path.exists() {
            return Ok(());
        }
        fs::read_to_string(&fallback_path).map(|s| s.trim().to_owned())?
    } else {
        session_id.to_owned()
    };
    let scope_dir = steplock_dir.join("sessions").join(&scope_key);
    if !scope_dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&scope_dir)?;
    eprintln!("steplock: cleaned up session {scope_key}");
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
#[path = "run_tests.rs"]
mod tests;

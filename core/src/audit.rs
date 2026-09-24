//! Append-only JSONL audit log written on every gate event.
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use serde_json::json;

/// Append one JSONL line to `.steplock/audit.log`.
/// Failures are silently ignored — audit logging must never block the hook.
pub fn append(steplock_dir: &Path, event: &str, checklist: &str, state: &str, session: &str) {
    let path = steplock_dir.join("audit.log");
    let line = json!({
        "event":     event,
        "checklist": checklist,
        "state":     state,
        "session":   session,
        "ts":        Utc::now().to_rfc3339(),
    })
    .to_string();

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)]
#[path = "audit_tests.rs"]
mod tests;

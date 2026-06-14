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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn appends_jsonl_line() {
        let tmp = TempDir::new().unwrap();
        append(tmp.path(), "block", "my-checklist", "step_one", "sess-abc");
        let content = fs::read_to_string(tmp.path().join("audit.log")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["event"], "block");
        assert_eq!(parsed["checklist"], "my-checklist");
        assert_eq!(parsed["state"], "step_one");
        assert_eq!(parsed["session"], "sess-abc");
        assert!(parsed["ts"].is_string());
    }

    #[test]
    fn appends_multiple_lines() {
        let tmp = TempDir::new().unwrap();
        append(tmp.path(), "block", "cl", "s1", "sess");
        append(tmp.path(), "ack", "cl", "s1", "sess");
        let content = fs::read_to_string(tmp.path().join("audit.log")).unwrap();
        assert_eq!(content.lines().count(), 2);
    }
}

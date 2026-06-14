use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub checklist: String,
    pub current_state: String,
    pub next_state: Option<String>,
    pub transitions: Vec<String>,
    pub visited: Vec<String>,
}

impl SessionState {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.current_state == "[*]"
    }
}

pub fn load_state(path: &Path) -> Result<SessionState> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

/// Atomically write state via temp + mv (POSIX mv is atomic same-filesystem).
pub fn save_state(path: &Path, state: &SessionState) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!("state.json.tmp.{}", std::process::id()));
    let content = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Initialize fresh state for the first (initial) state of a flow.
#[must_use]
pub fn init_state(checklist: &str, initial_state: &str) -> SessionState {
    SessionState {
        checklist: checklist.to_owned(),
        current_state: initial_state.to_owned(),
        next_state: None,
        transitions: Vec::new(),
        visited: Vec::new(),
    }
}

/// Normalized hook event passed in from the polyhook layer.
#[derive(Debug, Clone)]
pub struct HookEvent {
    /// Polyhook event type: "tool:before", "tool:after", "session:start", etc.
    pub event: String,
    /// Normalized tool name, e.g. "bash".
    pub tool: String,
    /// Key-value inputs from the tool call (e.g. command, path).
    pub input: HashMap<String, serde_json::Value>,
    /// Key-value outputs (populated for "tool:after" events).
    pub output: HashMap<String, serde_json::Value>,
    /// Session ID from the AI tool. Empty if unknown.
    pub session_id: String,
    /// Caller identifier: "claude-code", "cursor", etc.
    pub caller: String,
}

/// Response returned to the polyhook layer.
#[derive(Debug)]
#[non_exhaustive]
pub enum HookResponse {
    Block { message: String },
    Approve,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_state_sets_fields() {
        let s = init_state("my-checklist", "first_step");
        assert_eq!(s.checklist, "my-checklist");
        assert_eq!(s.current_state, "first_step");
        assert!(s.next_state.is_none());
        assert!(s.transitions.is_empty());
        assert!(s.visited.is_empty());
    }

    #[test]
    fn is_complete_false_when_active() {
        let s = init_state("cl", "step_one");
        assert!(!s.is_complete());
    }

    #[test]
    fn is_complete_true_at_end() {
        let s = SessionState {
            checklist: "cl".to_owned(),
            current_state: "[*]".to_owned(),
            next_state: None,
            transitions: vec![],
            visited: vec!["step_one".to_owned()],
        };
        assert!(s.is_complete());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.json");
        let s = SessionState {
            checklist: "gate".to_owned(),
            current_state: "check_one".to_owned(),
            next_state: Some("check_two".to_owned()),
            transitions: vec!["check_two".to_owned()],
            visited: vec!["prev".to_owned()],
        };
        save_state(&path, &s).unwrap();
        let loaded = load_state(&path).unwrap();
        assert_eq!(loaded.checklist, s.checklist);
        assert_eq!(loaded.current_state, s.current_state);
        assert_eq!(loaded.next_state, s.next_state);
        assert_eq!(loaded.transitions, s.transitions);
        assert_eq!(loaded.visited, s.visited);
    }

    #[test]
    fn load_state_error_on_missing_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.json");
        assert!(load_state(&path).is_err());
    }
}

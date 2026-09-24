//! Checklist configuration parsed from `config.toml`.
use serde::{Deserialize, Serialize};

use crate::SteplockError;

/// Controls when the checklist state is reset between invocations.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Reset {
    /// Reset per session: state persists within a session and resets on the next
    /// invocation after the checklist completes. This is the default.
    #[default]
    Session,
    /// Reset on every invocation: block on the first checklist item every time,
    /// with no state persistence.
    Always,
}

/// Parsed representation of a checklist's `config.toml`.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ChecklistConfig {
    /// Hook event name to match, e.g. `"tool:before"`.
    pub on_event: String,
    /// Tool name to match (e.g. `"bash"`). Omit or set to `""` to match any tool.
    #[serde(default)]
    pub on_tool: String,
    /// Optional CEL expression evaluated against the hook event. `None` matches all.
    pub match_input: Option<String>,
    /// Whether session state persists across invocations or resets every time.
    #[serde(default)]
    pub reset: Reset,
    /// When `true`, steplock generates `preview.sh` in the session directory.
    #[serde(default)]
    pub allow_preview_request: bool,
}

/// Parse a checklist `config.toml` from `content`, using `path` in error messages.
///
/// # Errors
///
/// Returns `SteplockError::Toml` if `content` is not valid TOML or the fields don't match.
pub fn parse_config(path: &str, content: &str) -> crate::Result<ChecklistConfig> {
    toml::from_str(content).map_err(|e| SteplockError::Toml {
        path: path.to_owned(),
        source: e,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "config_tests.rs"]
mod tests;

use serde::{Deserialize, Serialize};

/// Controls when the checklist state is reset between invocations.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
    /// Polyhook event type that activates this checklist, e.g. `"tool:before"`.
    pub on_event: String,
    /// Tool name to match (e.g. `"bash"`). Omit or set to `""` to match any tool.
    #[serde(default)]
    pub on_tool: String,
    /// Optional CEL expression evaluated against the event. `None` matches all events.
    pub match_input: Option<String>,
    /// When to reset checklist progress.
    #[serde(default)]
    pub reset: Reset,
    /// If `true`, a `preview.sh` script is generated so the agent can inspect
    /// all checklist items before the first block.
    #[serde(default)]
    pub allow_preview_request: bool,
}

/// # Errors
///
/// Returns `Err` if `content` is not valid TOML or does not match the `ChecklistConfig` schema.
pub fn parse_config(path: &str, content: &str) -> crate::Result<ChecklistConfig> {
    toml::from_str(content).map_err(|e| SteplockError::Toml {
        path: path.to_owned(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let cfg = parse_config(
            "test.toml",
            r#"
on_event = "tool:before"
on_tool  = "bash"
"#,
        )
        .unwrap();
        assert_eq!(cfg.on_event, "tool:before");
        assert_eq!(cfg.on_tool, "bash");
        assert!(cfg.match_input.is_none());
        assert!(matches!(cfg.reset, Reset::Session));
        assert!(!cfg.allow_preview_request);
    }

    #[test]
    fn on_tool_defaults_to_empty_when_omitted() {
        let cfg = parse_config(
            "test.toml",
            r#"
on_event = "tool:before"
"#,
        )
        .unwrap();
        assert_eq!(cfg.on_tool, "");
    }

    #[test]
    fn parses_full_config() {
        let cfg = parse_config(
            "test.toml",
            r#"
on_event = "tool:after"
on_tool  = "write_file"
match_input = "input.path.startsWith('/etc')"
reset = "always"
allow_preview_request = true
"#,
        )
        .unwrap();
        assert_eq!(cfg.on_event, "tool:after");
        assert_eq!(cfg.on_tool, "write_file");
        assert_eq!(cfg.match_input.unwrap(), "input.path.startsWith('/etc')");
        assert!(matches!(cfg.reset, Reset::Always));
        assert!(cfg.allow_preview_request);
    }

    #[test]
    fn error_on_invalid_toml() {
        let err = parse_config("bad.toml", "not valid toml !!!@@@");
        err.unwrap_err();
    }

    #[test]
    fn reset_default_is_session() {
        assert!(matches!(Reset::default(), Reset::Session));
    }
}

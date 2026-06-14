use serde::{Deserialize, Serialize};

/// When to reset a checklist's session state.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reset {
    /// State persists per session ID and advances on each acknowledgement.
    #[default]
    Session,
    /// State is never persisted; the checklist blocks from the first step on every invocation.
    Always,
}

/// Parsed contents of a checklist's `config.toml`.
#[derive(Debug, Clone, Deserialize)]
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
    toml::from_str(content).map_err(|e| crate::error::SteplockError::Toml {
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
        assert!(err.is_err());
    }

    #[test]
    fn reset_default_is_session() {
        assert!(matches!(Reset::default(), Reset::Session));
    }
}

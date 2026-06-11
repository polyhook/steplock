use serde::{Deserialize, Serialize};

/// When to reset checklist progress for a scope.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reset {
    /// Reset when the agent session ends (default). Progress persists across retries within the same session.
    #[default]
    Session,
    /// Reset on every invocation — no state persisted.
    Always,
}

/// Parsed representation of a checklist's `config.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct ChecklistConfig {
    /// Polyhook event type that triggers this checklist (e.g. `"tool:before"`).
    pub on_event: String,
    /// Normalized polyhook tool name to match (e.g. `"bash"`, `"write_file"`).
    pub on_tool: String,
    /// Optional CEL expression evaluated against the hook event. Omit to match all invocations.
    pub match_input: Option<String>,
    /// When to reset checklist progress. Defaults to `Reset::Session`.
    #[serde(default)]
    pub reset: Reset,
    /// If true, the first block message includes a hint to run `preview.sh`.
    #[serde(default)]
    pub allow_preview_request: bool,
}

/// Parse a `config.toml` file from its string content.
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

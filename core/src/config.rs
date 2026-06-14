use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reset {
    #[default]
    Session,
    Always,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChecklistConfig {
    pub on_event: String,
    /// Tool name to match (e.g. `"bash"`). Omit or set to `""` to match any tool.
    #[serde(default)]
    pub on_tool: String,
    pub match_input: Option<String>,
    #[serde(default)]
    pub reset: Reset,
    #[serde(default)]
    pub allow_preview_request: bool,
}

/// Parse checklist configuration from a TOML string.
///
/// # Errors
///
/// Returns `Err` if `content` is not valid TOML or does not match the
/// `ChecklistConfig` schema.
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

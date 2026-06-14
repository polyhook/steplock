use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::error::Result;
use crate::flow::FlowGraph;

static ACK_SH: &str = include_str!("../../scripts/ack.sh");

/// Write ack.sh to `dir` only if it does not already exist.
///
/// # Errors
///
/// Returns an error if the file cannot be written or its permissions cannot be set.
pub fn ensure_ack_sh(dir: &Path) -> Result<()> {
    let path = dir.join("ack.sh");
    if path.exists() {
        return Ok(());
    }
    write_executable(&path, ACK_SH)
}

/// Write preview.sh to `dir` only if it does not already exist.
///
/// # Errors
///
/// Returns an error if the file cannot be written or its permissions cannot be set.
pub fn ensure_preview_sh(dir: &Path, checklist_name: &str, flow: &FlowGraph) -> Result<()> {
    let path = dir.join("preview.sh");
    if path.exists() {
        return Ok(());
    }
    let script = build_preview_sh(checklist_name, flow);
    write_executable(&path, &script)
}

fn write_executable(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

fn build_preview_sh(checklist_name: &str, flow: &FlowGraph) -> String {
    let n = flow.order.len();
    let mut lines = vec![
        "#!/bin/sh".to_owned(),
        r#"DIR="$(cd "$(dirname "$0")" && pwd)""#.to_owned(),
        r#"STATE="$DIR/state.json""#.to_owned(),
        r#"VISITED=$(jq -r '.visited[]?' "$STATE" 2>/dev/null)"#.to_owned(),
        String::new(),
        format!(
            r#"echo "Checklist: {} ({} item{})""#,
            checklist_name,
            n,
            if n == 1 { "" } else { "s" }
        ),
        String::new(),
        "check() {".to_owned(),
        r#"  label="$1"; state="$2""#.to_owned(),
        r#"  if printf '%s\n' $VISITED | grep -qxF "$state"; then"#.to_owned(),
        r#"    printf "  [x] %s\n" "$label""#.to_owned(),
        "  else".to_owned(),
        r#"    printf "  [ ] %s\n" "$label""#.to_owned(),
        "  fi".to_owned(),
        "}".to_owned(),
        String::new(),
    ];

    for state in &flow.order {
        let label = flow
            .labels
            .get(state)
            .map_or(state.as_str(), |s| s.as_str());
        // Escape single quotes in label
        let label_escaped = label.replace('\'', "'\\''");
        let state_escaped = state.replace('\'', "'\\''");
        lines.push(format!("check '{label_escaped}' '{state_escaped}'"));
    }

    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::parse_mmd;
    use std::fs;
    use tempfile::TempDir;

    const SIMPLE_MMD: &str = r#"stateDiagram-v2
    [*] --> step_one
    step_one --> [*]
    step_one : Do the first thing
"#;

    const TWO_STEP_MMD: &str = r#"stateDiagram-v2
    [*] --> step_one
    step_one --> step_two
    step_two --> [*]
    step_one : First step
    step_two : Second step
"#;

    #[test]
    fn ensure_ack_sh_creates_file() {
        let tmp = TempDir::new().unwrap();
        ensure_ack_sh(tmp.path()).unwrap();
        let path = tmp.path().join("ack.sh");
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("state.json"));
    }

    #[test]
    fn ack_sh_handles_complete_session() {
        assert!(ACK_SH.contains("session already complete"));
        assert!(ACK_SH.contains("[*]"));
    }

    #[test]
    fn ensure_ack_sh_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ack.sh");
        fs::write(&path, "custom content").unwrap();
        ensure_ack_sh(tmp.path()).unwrap();
        // Should not overwrite existing file
        assert_eq!(fs::read_to_string(&path).unwrap(), "custom content");
    }

    #[test]
    fn ensure_preview_sh_creates_file() {
        let tmp = TempDir::new().unwrap();
        let flow = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        ensure_preview_sh(tmp.path(), "my-checklist", &flow).unwrap();
        let path = tmp.path().join("preview.sh");
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("my-checklist"));
        assert!(content.contains("Do the first thing"));
    }

    #[test]
    fn ensure_preview_sh_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("preview.sh");
        fs::write(&path, "custom").unwrap();
        let flow = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        ensure_preview_sh(tmp.path(), "checklist", &flow).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "custom");
    }

    #[test]
    fn preview_sh_singular_item() {
        let flow = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        let script = build_preview_sh("my-gate", &flow);
        assert!(script.contains("1 item)"));
        assert!(!script.contains("1 items)"));
    }

    #[test]
    fn preview_sh_plural_items() {
        let flow = parse_mmd("test.mmd", TWO_STEP_MMD).unwrap();
        let script = build_preview_sh("my-gate", &flow);
        assert!(script.contains("2 items)"));
    }

    #[test]
    fn preview_sh_escapes_single_quotes() {
        let mmd = "stateDiagram-v2\n    [*] --> s\n    s --> [*]\n    s : It's fine\n";
        let flow = parse_mmd("test.mmd", mmd).unwrap();
        let script = build_preview_sh("checklist", &flow);
        assert!(script.contains("It'\\''s fine"));
    }

    #[test]
    fn preview_sh_uses_state_name_as_fallback_label() {
        let mmd = "stateDiagram-v2\n    [*] --> unlabeled\n    unlabeled --> [*]\n";
        let flow = parse_mmd("test.mmd", mmd).unwrap();
        let script = build_preview_sh("checklist", &flow);
        assert!(script.contains("'unlabeled' 'unlabeled'"));
    }
}

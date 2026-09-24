//! Generates `ack.sh` and `preview.sh` helper scripts in session directories.
use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::flow::FlowGraph;

static ACK_SH: &str = include_str!("../scripts/ack.sh");

/// Write ack.sh to `dir` only if it does not already exist.
///
/// # Errors
///
/// Returns `Err` if writing the file or setting its permissions fails.
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
/// Returns `Err` if writing the file or setting its permissions fails.
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
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
            .map_or(state.as_str(), String::as_str);
        // Escape single quotes in label
        let label_escaped = label.replace('\'', "'\\''");
        let state_escaped = state.replace('\'', "'\\''");
        lines.push(format!("check '{label_escaped}' '{state_escaped}'"));
    }

    lines.join("\n") + "\n"
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "scripts_tests.rs"]
mod tests;

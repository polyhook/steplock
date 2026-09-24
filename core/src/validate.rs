use std::fs;
use std::path::Path;

use crate::config::parse_config;
use crate::error::{Result, SteplockError};
use crate::flow::parse_mmd;

/// Validate all checklists under the given `checklists_dir`.
///
/// Returns one `(label, error)` pair per file that fails to parse.
/// An empty vec means all checklists are valid.
///
/// # Errors
///
/// Individual parse errors are collected and returned rather than short-circuiting.
/// The directory itself is silently skipped if it cannot be read.
#[must_use]
pub fn validate_checklists(checklists_dir: &Path) -> Vec<(String, SteplockError)> {
    let mut errors: Vec<(String, SteplockError)> = Vec::new();
    let Ok(entries) = fs::read_dir(checklists_dir) else {
        return errors;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let dir = entry.path();

        check_file(
            &dir.join("config.toml"),
            &format!("{name}/config.toml"),
            |label, content| parse_config(label, content).map(|_| ()),
            &mut errors,
        );

        check_file(
            &dir.join("flow.mmd"),
            &format!("{name}/flow.mmd"),
            |label, content| parse_mmd(label, content).map(|_| ()),
            &mut errors,
        );
    }
    errors
}

fn check_file(
    path: &Path,
    label: &str,
    parse: impl FnOnce(&str, &str) -> Result<()>,
    errors: &mut Vec<(String, SteplockError)>,
) {
    match fs::read_to_string(path) {
        Err(e) => errors.push((label.to_owned(), SteplockError::Io(e))),
        Ok(content) => {
            if let Err(e) = parse(label, &content) {
                errors.push((label.to_owned(), e));
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "validate_tests.rs"]
mod tests;

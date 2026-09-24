//! Location of the global steplock directory shared by every project.
//!
//! The global directory has the same layout as a project `.steplock/`:
//! `checklists/<name>/{config.toml,flow.mmd}`, `sessions/` and `audit.log`.
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

/// Environment variable that overrides the global steplock directory.
/// Set it to an empty string to turn global checklists off.
pub const GLOBAL_DIR_ENV: &str = "STEPLOCK_GLOBAL_DIR";

/// Resolve the global steplock directory from the process environment.
///
/// Lookup order:
/// 1. `$STEPLOCK_GLOBAL_DIR` — used as-is; an empty value disables global checklists.
/// 2. `$XDG_CONFIG_HOME/steplock` — when `XDG_CONFIG_HOME` is set to an absolute path.
/// 3. `$HOME/.config/steplock`.
///
/// Returns `None` when global checklists are disabled or no home directory is known.
/// The directory is not required to exist.
#[must_use]
pub fn global_steplock_dir() -> Option<PathBuf> {
    resolve_global_dir(|key| env::var_os(key))
}

/// Resolve the global steplock directory with `var` as the environment lookup.
fn resolve_global_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = var(GLOBAL_DIR_ENV) {
        return if dir.is_empty() {
            None
        } else {
            Some(PathBuf::from(dir))
        };
    }
    if let Some(xdg) = var("XDG_CONFIG_HOME").map(PathBuf::from) {
        if xdg.is_absolute() {
            return Some(xdg.join("steplock"));
        }
    }
    var("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".config").join("steplock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn env_override_wins() {
        let dir = resolve_global_dir(lookup(&[
            (GLOBAL_DIR_ENV, "/custom/steplock"),
            ("XDG_CONFIG_HOME", "/xdg"),
            ("HOME", "/home/me"),
        ]));
        assert_eq!(
            dir,
            Some(PathBuf::from("/custom/steplock")),
            "STEPLOCK_GLOBAL_DIR must take precedence"
        );
    }

    #[test]
    fn empty_env_override_disables_global() {
        let dir = resolve_global_dir(lookup(&[(GLOBAL_DIR_ENV, ""), ("HOME", "/home/me")]));
        assert_eq!(dir, None, "empty STEPLOCK_GLOBAL_DIR must disable global");
    }

    #[test]
    fn uses_xdg_config_home() {
        let dir = resolve_global_dir(lookup(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/me")]));
        assert_eq!(
            dir,
            Some(PathBuf::from("/xdg/steplock")),
            "XDG path expected"
        );
    }

    #[test]
    fn ignores_relative_xdg_config_home() {
        let dir = resolve_global_dir(lookup(&[
            ("XDG_CONFIG_HOME", "relative"),
            ("HOME", "/home/me"),
        ]));
        assert_eq!(
            dir,
            Some(PathBuf::from("/home/me/.config/steplock")),
            "relative XDG_CONFIG_HOME must fall back to HOME"
        );
    }

    #[test]
    fn falls_back_to_home() {
        let dir = resolve_global_dir(lookup(&[("HOME", "/home/me")]));
        assert_eq!(
            dir,
            Some(PathBuf::from("/home/me/.config/steplock")),
            "HOME fallback expected"
        );
    }

    #[test]
    fn none_without_home() {
        assert_eq!(
            resolve_global_dir(lookup(&[])),
            None,
            "no env means no global dir"
        );
    }
}

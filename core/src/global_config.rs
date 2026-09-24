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
/// 3. `$HOME/.config/steplock`, or `%USERPROFILE%\.config\steplock` when `HOME` is unset.
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
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(&var)
        .find(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".config").join("steplock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An absolute path on the current platform (`/name` or `C:\name`).
    fn abs(name: &str) -> PathBuf {
        let root = if cfg!(windows) { "C:\\" } else { "/" };
        PathBuf::from(root).join(name)
    }

    fn lookup(vars: &[(&str, OsString)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        move |key| map.get(key).cloned()
    }

    fn home_config(home: &str) -> PathBuf {
        abs(home).join(".config").join("steplock")
    }

    #[test]
    fn env_override_wins() {
        let dir = resolve_global_dir(lookup(&[
            (GLOBAL_DIR_ENV, abs("custom").into()),
            ("XDG_CONFIG_HOME", abs("xdg").into()),
            ("HOME", abs("home").into()),
        ]));
        assert_eq!(
            dir,
            Some(abs("custom")),
            "STEPLOCK_GLOBAL_DIR must take precedence"
        );
    }

    #[test]
    fn empty_env_override_disables_global() {
        let dir = resolve_global_dir(lookup(&[
            (GLOBAL_DIR_ENV, OsString::new()),
            ("HOME", abs("home").into()),
        ]));
        assert_eq!(dir, None, "empty STEPLOCK_GLOBAL_DIR must disable global");
    }

    #[test]
    fn uses_xdg_config_home() {
        let dir = resolve_global_dir(lookup(&[
            ("XDG_CONFIG_HOME", abs("xdg").into()),
            ("HOME", abs("home").into()),
        ]));
        assert_eq!(dir, Some(abs("xdg").join("steplock")), "XDG path expected");
    }

    #[test]
    fn ignores_relative_xdg_config_home() {
        let dir = resolve_global_dir(lookup(&[
            ("XDG_CONFIG_HOME", "relative".into()),
            ("HOME", abs("home").into()),
        ]));
        assert_eq!(
            dir,
            Some(home_config("home")),
            "relative XDG_CONFIG_HOME must fall back to HOME"
        );
    }

    #[test]
    fn falls_back_to_home() {
        let dir = resolve_global_dir(lookup(&[
            ("HOME", abs("home").into()),
            ("USERPROFILE", abs("profile").into()),
        ]));
        assert_eq!(dir, Some(home_config("home")), "HOME wins over USERPROFILE");
    }

    #[test]
    fn falls_back_to_userprofile_without_home() {
        let dir = resolve_global_dir(lookup(&[
            ("HOME", OsString::new()),
            ("USERPROFILE", abs("profile").into()),
        ]));
        assert_eq!(
            dir,
            Some(home_config("profile")),
            "USERPROFILE fallback expected"
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

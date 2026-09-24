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
/// 3. `<home>/.config/steplock`, where `<home>` comes from [`dirs::home_dir`].
///
/// Returns `None` when global checklists are disabled or no home directory is known.
/// The directory is not required to exist.
#[must_use]
pub fn global_steplock_dir() -> Option<PathBuf> {
    resolve_global_dir(|key| env::var_os(key), dirs::home_dir())
}

/// Resolve the global steplock directory with `var` as the environment lookup and `home`
/// as the user's home directory.
fn resolve_global_dir(
    var: impl Fn(&str) -> Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
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
    home.map(|home| home.join(".config").join("steplock"))
}

#[cfg(test)]
#[path = "global_config_tests.rs"]
mod tests;

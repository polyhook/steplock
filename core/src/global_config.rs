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

/// Variables agents set to the user's real home when they give hooks a sandboxed `HOME`.
/// Hermes Agent points `HOME` at a per-profile directory in containers or with
/// `TERMINAL_HOME_MODE=profile`, and exports the real home as `HERMES_REAL_HOME`. Reading it
/// keeps the global directory the same no matter which agent runs the hook.
const REAL_HOME_ENVS: [&str; 1] = ["HERMES_REAL_HOME"];

/// Resolve the global steplock directory from the process environment.
///
/// Lookup order:
/// 1. `$STEPLOCK_GLOBAL_DIR` — used as-is; an empty value disables global checklists.
/// 2. `$XDG_CONFIG_HOME/steplock` — when `XDG_CONFIG_HOME` is set to an absolute path.
/// 3. `<home>/.config/steplock`. `<home>` is the real home an agent reports (such as
///    `$HERMES_REAL_HOME`) when it is an absolute path, else [`dirs::home_dir`].
///
/// Returns `None` when global checklists are disabled or no home directory is known.
/// The directory is not required to exist.
#[must_use]
pub fn global_steplock_dir() -> Option<PathBuf> {
    resolve_global_dir(|key| env::var_os(key), dirs::home_dir())
}

/// Resolve the global steplock directory with `var` as the environment lookup and `home`
/// as the process home directory (used when no agent reports a real home).
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
    let real_home = REAL_HOME_ENVS
        .iter()
        .filter_map(|key| var(key).map(PathBuf::from))
        .find(|dir| dir.is_absolute());
    real_home
        .or(home)
        .map(|home| home.join(".config").join("steplock"))
}

#[cfg(test)]
#[path = "global_config_tests.rs"]
mod tests;

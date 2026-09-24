//! Unit tests for `global_config`.
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
    let dir = resolve_global_dir(
        lookup(&[
            (GLOBAL_DIR_ENV, abs("custom").into()),
            ("XDG_CONFIG_HOME", abs("xdg").into()),
        ]),
        Some(abs("home")),
    );
    assert_eq!(
        dir,
        Some(abs("custom")),
        "STEPLOCK_GLOBAL_DIR must take precedence"
    );
}

#[test]
fn empty_env_override_disables_global() {
    let dir = resolve_global_dir(
        lookup(&[(GLOBAL_DIR_ENV, OsString::new())]),
        Some(abs("home")),
    );
    assert_eq!(dir, None, "empty STEPLOCK_GLOBAL_DIR must disable global");
}

#[test]
fn uses_xdg_config_home() {
    let dir = resolve_global_dir(
        lookup(&[("XDG_CONFIG_HOME", abs("xdg").into())]),
        Some(abs("home")),
    );
    assert_eq!(dir, Some(abs("xdg").join("steplock")), "XDG path expected");
}

#[test]
fn ignores_relative_xdg_config_home() {
    let dir = resolve_global_dir(
        lookup(&[("XDG_CONFIG_HOME", "relative".into())]),
        Some(abs("home")),
    );
    assert_eq!(
        dir,
        Some(home_config("home")),
        "relative XDG_CONFIG_HOME must fall back to the home directory"
    );
}

#[test]
fn falls_back_to_home_dir() {
    let dir = resolve_global_dir(lookup(&[]), Some(abs("home")));
    assert_eq!(dir, Some(home_config("home")), "home fallback expected");
}

#[test]
fn none_without_home() {
    assert_eq!(
        resolve_global_dir(lookup(&[]), None),
        None,
        "no env and no home means no global dir"
    );
}

#[test]
fn prefers_agent_real_home_over_sandboxed_home() {
    let dir = resolve_global_dir(
        lookup(&[("HERMES_REAL_HOME", abs("real").into())]),
        Some(abs("profile-home")),
    );
    assert_eq!(
        dir,
        Some(home_config("real")),
        "HERMES_REAL_HOME must win over a sandboxed HOME"
    );
}

#[test]
fn ignores_relative_agent_real_home() {
    let dir = resolve_global_dir(
        lookup(&[("HERMES_REAL_HOME", "relative".into())]),
        Some(abs("home")),
    );
    assert_eq!(
        dir,
        Some(home_config("home")),
        "relative HERMES_REAL_HOME must fall back to the home directory"
    );
}

#[test]
fn xdg_config_home_wins_over_agent_real_home() {
    let dir = resolve_global_dir(
        lookup(&[
            ("XDG_CONFIG_HOME", abs("xdg").into()),
            ("HERMES_REAL_HOME", abs("real").into()),
        ]),
        Some(abs("home")),
    );
    assert_eq!(dir, Some(abs("xdg").join("steplock")), "XDG path expected");
}

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Global checklists: `$STEPLOCK_GLOBAL_DIR`, `$XDG_CONFIG_HOME/steplock` or `~/.config/steplock` holds checklists that apply to every project; project checklists with the same name override them
- `steplock init --global` and `steplock clean --global`; `steplock validate` also checks global checklists
- `run_with_global` and `global_steplock_dir` library API
- Hermes Agent setup guide (`pre_tool_call` + `on_session_end` shell hooks)
- `steplock init` command creates `.steplock/checklists/` and `.gitignore` skeleton
- `session:stop` event cleans up the session directory so the checklist resets
- `allow_preview_request` config option generates a `preview.sh` showing checklist progress
- `command_words` CEL variable for matching subcommands without false positives from paths
- `reset = "always"` mode blocks unconditionally on every hook invocation
- `#[non_exhaustive]` on `HookResponse` and `Reset` for semver-safe extensibility
- MSRV set to Rust 1.75 in `Cargo.toml`
- Windows `x86_64-pc-windows-msvc` binary in the release matrix
- Module-level rustdoc (`//!`) and public-API doc comments across all modules
- Append-only JSONL audit log at `.steplock/audit.log`
- Incrementally ratcheted clippy deny list (14 lints and counting)

### Fixed
- Hook responses use the calling agent's own format again (fixes #196). The CLI parsed stdin directly instead of reading it through polyhook, so the detected caller was lost and every response used the legacy Claude Code shape. In Claude Code that top-level `decision: "block"` ended the whole session instead of denying one tool call, and agents that don't accept that shape let the call through.
- Global checklists resolve to the same directory under Hermes Agent when it sandboxes `HOME` (containers, `TERMINAL_HOME_MODE=profile`): `$HERMES_REAL_HOME` is preferred over `HOME`
- `ack.sh` exits 0 with a message when the session is already complete
- Unknown CLI arguments now exit 1 with a usage hint instead of silently doing nothing
- `on_tool` is now optional in `config.toml` (omit to match any tool)
- Mermaid state labels with backticks no longer cause a parse error
- Session cleanup uses correct scope key when `session_id` is empty

### Changed
- CLI argument parsing uses `clap`: adds `steplock help`, per-command `--help`, and clearer usage errors (still exit 1)
- Idempotent ack: re-acknowledging the current step is a no-op, not an error

## [0.1.0] - Initial release

### Added
- Core gate engine: intercepts polyhook events and blocks until checklist is complete
- Mermaid `stateDiagram-v2` parser for defining sequential checklists
- `config.toml` schema: `on_event`, `on_tool`, `match_input` (CEL), `reset`
- CEL expression evaluation for `match_input` (filtering by tool input fields)
- `state.json` persistence tracking current state and visited steps per session
- `ack.sh` helper script that advances the checklist when the operator runs it
- Append-only JSONL audit log
- Pre-push checklist example in `.steplock/checklists/pre-push/`
- GitHub Actions CI: fmt, clippy, tests, doc check, release binary build
- MIT license

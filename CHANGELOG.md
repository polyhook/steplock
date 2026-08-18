# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
- Hook responses are serialized in the calling agent's own wire format again — the CLI parsed stdin directly instead of reading it through polyhook, discarding the detected caller so every response used the legacy Claude Code shape (which also terminated the whole Claude Code session on a `PreToolUse` block instead of denying the single tool call)
- `ack.sh` exits 0 with a message when the session is already complete
- Unknown CLI arguments now exit 1 with a usage hint instead of silently doing nothing
- `on_tool` is now optional in `config.toml` (omit to match any tool)
- Mermaid state labels with backticks no longer cause a parse error
- Session cleanup uses correct scope key when `session_id` is empty

### Changed
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

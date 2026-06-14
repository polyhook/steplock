//! Stateful quality gate library for AI agent tool calls.
//!
//! `steplock-core` intercepts tool calls from AI coding assistants (Claude Code,
//! Cursor, etc.) and blocks execution until sequential checklist items are
//! acknowledged by the agent via `ack.sh`.

/// Audit log utilities.
pub mod audit;
/// CEL expression evaluator for `match_input` conditions.
pub mod cel_eval;
/// Checklist configuration types and TOML parser.
pub mod config;
/// Error types.
pub mod error;
/// Mermaid `stateDiagram-v2` parser.
pub mod flow;
/// Gate runner — entry point for polyhook integration.
pub mod run;
/// Ack and preview script generation.
pub mod scripts;
/// Session state persistence.
pub mod state;

pub use error::{Result, SteplockError};
pub use run::run;
pub use state::{HookEvent, HookResponse, SessionState};

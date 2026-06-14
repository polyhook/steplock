//! Stateful quality gate logic for steplock.
//!
//! Evaluates hook events against Mermaid-defined checklists and returns
//! a block or approve response that the polyhook layer converts to JSON.
#![forbid(unsafe_code)]

pub mod audit;
pub mod cel_eval;
pub mod config;
pub mod error;
pub mod flow;
pub mod run;
pub mod scripts;
pub mod state;

pub use error::{Result, SteplockError};
pub use run::run;
pub use state::{HookEvent, HookResponse, SessionState};

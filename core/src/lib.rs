//! `steplock-core` — stateful quality gate logic for the steplock system.
//!
//! Intercepts AI agent tool calls via [polyhook](https://docs.rs/polyhook) hooks and blocks
//! execution until the operator has acknowledged each step in a sequential checklist.
//!
//! # Quick start
//!
//! ```no_run
//! use std::collections::HashMap;
//! use std::path::Path;
//! use steplock_core::{run, HookEvent};
//!
//! let event = HookEvent {
//!     event: "tool:before".to_owned(),
//!     tool: "bash".to_owned(),
//!     input: HashMap::new(),
//!     output: HashMap::new(),
//!     session_id: "my-session".to_owned(),
//!     caller: "claude-code".to_owned(),
//! };
//! let response = run(&event, Path::new(".")).unwrap();
//! ```

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

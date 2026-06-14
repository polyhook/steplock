use std::io;
use std::result;

use thiserror::Error;
use toml::de;

/// All errors that can be produced by `steplock`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SteplockError {
    /// A filesystem I/O error.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// A TOML parse error in the config file at `path`.
    #[error("TOML parse error in {path}: {source}")]
    Toml {
        /// Path to the config file that failed to parse.
        path: String,
        /// Underlying TOML parse error.
        source: de::Error,
    },

    /// A JSON serialization or deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A Mermaid `stateDiagram-v2` parse error in the flow file at `path`.
    #[error("Mermaid parse error in {path}: {message}")]
    Mermaid {
        /// Path to the `.mmd` file that failed to parse.
        path: String,
        /// Human-readable description of the parse failure.
        message: String,
    },

    /// A CEL expression evaluation error.
    #[error("CEL error in expression `{expr}`: {message}")]
    Cel {
        /// The CEL expression that failed.
        expr: String,
        /// Human-readable description of the evaluation failure.
        message: String,
    },

    /// An invalid or inconsistent state was detected.
    #[error("State error: {0}")]
    State(String),
}

/// Convenience alias for `Result<T, SteplockError>`.
pub type Result<T> = result::Result<T, SteplockError>;

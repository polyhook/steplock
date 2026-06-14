//! Error types for steplock-core.
use thiserror::Error;

/// All error variants that steplock-core can produce.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SteplockError {
    /// Filesystem I/O failure.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// `config.toml` could not be parsed as valid TOML.
    #[error("TOML parse error in {path}: {source}")]
    Toml {
        /// Path to the config file.
        path: String,
        /// Underlying TOML parse error.
        source: toml::de::Error,
    },

    /// State JSON serialization or deserialization failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// `flow.mmd` could not be parsed as a valid Mermaid state diagram.
    #[error("Mermaid parse error in {path}: {message}")]
    Mermaid {
        /// Path to the `.mmd` file.
        path: String,
        /// Human-readable description of what went wrong.
        message: String,
    },

    /// A CEL expression was syntactically invalid or failed to evaluate.
    #[error("CEL error in expression `{expr}`: {message}")]
    Cel {
        /// The CEL expression text.
        expr: String,
        /// Human-readable description of the failure.
        message: String,
    },

    /// Generic state machine error.
    #[error("State error: {0}")]
    State(String),
}

/// Convenience alias: `Result<T>` with [`SteplockError`] as the error type.
pub type Result<T> = std::result::Result<T, SteplockError>;

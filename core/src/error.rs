use thiserror::Error;

/// All errors returned by `steplock-core`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SteplockError {
    /// Filesystem I/O failure (read, write, rename, etc.).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML deserialization error in a checklist `config.toml`.
    #[error("TOML parse error in {path}: {source}")]
    Toml {
        /// Path to the config file, for error messages.
        path: String,
        /// Underlying parse error.
        source: toml::de::Error,
    },

    /// JSON serialization or deserialization failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Mermaid flow diagram could not be parsed.
    #[error("Mermaid parse error in {path}: {message}")]
    Mermaid {
        /// Path to the `.mmd` file.
        path: String,
        /// Description of the parse failure.
        message: String,
    },

    /// CEL expression failed to compile or execute.
    #[error("CEL error in expression `{expr}`: {message}")]
    Cel {
        /// The expression that caused the error.
        expr: String,
        /// Description of the CEL failure.
        message: String,
    },

    /// Session state file is corrupt or contains unexpected data.
    #[error("State error: {0}")]
    State(String),
}

/// Convenience alias used throughout `steplock-core`.
pub type Result<T> = std::result::Result<T, SteplockError>;

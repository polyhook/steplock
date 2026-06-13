use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SteplockError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    Toml {
        path: String,
        source: toml::de::Error,
    },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Mermaid parse error in {path}: {message}")]
    Mermaid { path: String, message: String },

    #[error("CEL error in expression `{expr}`: {message}")]
    Cel { expr: String, message: String },

    #[error("State error: {0}")]
    State(String),
}

pub type Result<T> = std::result::Result<T, SteplockError>;

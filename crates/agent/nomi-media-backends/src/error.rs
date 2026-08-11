//! Error types for Flowy media backends and local media I/O.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaBackendError {
    #[error("{0}")]
    Message(String),

    #[error("not logged in — sign in via Settings → Cloud Account first")]
    NotAuthenticated,

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("LLM failed: {0}")]
    Llm(String),

    #[error("image generation failed: {0}")]
    Image(String),

    #[error("video generation failed: {0}")]
    Video(String),

    #[error("media processing failed: {0}")]
    Media(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("cancelled")]
    Cancelled,
}

impl MediaBackendError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

pub type MediaBackendResult<T> = Result<T, MediaBackendError>;

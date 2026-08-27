//! Error types for ViMax pipelines.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VimaxError {
    #[error("{0}")]
    Message(String),

    #[error("not logged in — sign in via Settings → Cloud Account first")]
    NotAuthenticated,

    #[error("session not found: {0}")]
    SessionNotFound(String),

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

impl VimaxError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }

    /// True when this error is a user-initiated cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, VimaxError::Cancelled)
    }

    /// Turn a `catch_unwind` payload into a readable error (panics often hide the cause).
    pub fn from_panic_payload(ctx: &str, payload: Box<dyn std::any::Any + Send>) -> Self {
        let detail = panic_payload_message(payload);
        tracing::error!(%ctx, %detail, "vimax task panicked");
        Self::Message(format!("{ctx} panicked: {detail}"))
    }
}

fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    "unknown panic payload".into()
}

pub type VimaxResult<T> = Result<T, VimaxError>;

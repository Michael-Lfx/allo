//! Error types for the Montage runtime.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MontageError {
    #[error("{0}")]
    Message(String),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("pipeline not found: {0}")]
    PipelineNotFound(String),

    #[error("stage not found: {0} (pipeline {1})")]
    StageNotFound(String, String),

    #[error("artifact schema not found: {0}")]
    SchemaNotFound(String),

    #[error("artifact validation failed for '{0}': {1}")]
    ArtifactInvalid(String, String),

    #[error("checkpoint validation failed: {0}")]
    CheckpointInvalid(String),

    #[error("pipeline manifest invalid ({0}): {1}")]
    ManifestInvalid(String, String),

    #[error("governance blocked: {0}")]
    GovernanceBlocked(String),

    #[error("tool not available: {0} — {1}")]
    ToolUnavailable(String, String),

    #[error("tool '{0}' failed: {1}")]
    ToolFailed(String, String),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("not logged in — sign in via Settings → Cloud Account first")]
    NotAuthenticated,

    #[error("project is busy: {0}")]
    Busy(String),

    #[error("LLM failed: {0}")]
    Llm(String),

    #[error("media backend error: {0}")]
    Media(#[from] nomi_media_backends::MediaBackendError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("zip error: {0}")]
    Zip(String),

    #[error("cancelled")]
    Cancelled,
}

impl MontageError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }

    /// Turn a `catch_unwind` payload into a readable error.
    pub fn from_panic_payload(ctx: &str, payload: Box<dyn std::any::Any + Send>) -> Self {
        let detail = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "unknown panic payload".to_string());
        tracing::error!(%ctx, %detail, "montage task panicked");
        Self::Message(format!("{ctx} panicked: {detail}"))
    }
}

pub type MontageResult<T> = Result<T, MontageError>;

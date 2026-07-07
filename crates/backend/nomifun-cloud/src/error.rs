//! Structured errors for remote LLM server client operations.

use thiserror::Error;

/// Token store, OAuth PKCE, and other cloud-side local persistence errors.
#[derive(Debug, Error)]
pub enum CloudError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("authentication failed: {0}")]
    AuthFailed(String),
}

#[derive(Debug, Error)]
pub enum ServerClientError {
    #[error("server client not configured: {0}")]
    NotConfigured(String),

    #[error("server client disabled in config")]
    Disabled,

    #[error("server base_url not configured")]
    MissingBaseUrl,

    #[error("authentication required: {0}")]
    AuthRequired(String),

    #[error("API error {code}: {msg}")]
    Api { code: i32, msg: String },

    #[error("HTTP request failed: {0}")]
    Http(String),

    #[error("server returned {status}: {body}")]
    Server {
        status: u16,
        body: String,
        request_id: Option<String>,
    },

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error(transparent)]
    Cloud(#[from] CloudError),
}

impl ServerClientError {
    pub fn not_configured(feature: &str) -> Self {
        Self::NotConfigured(format!(
            "{feature} API is not wired yet — waiting for server interface documentation"
        ))
    }

    pub fn from_http_status(status: u16, body: String, request_id: Option<String>) -> Self {
        Self::Server {
            status,
            body,
            request_id,
        }
    }
}

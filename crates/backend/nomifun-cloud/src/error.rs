//! Structured errors for remote LLM server client operations.

use nomifun_common::AppError;
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

    /// Map a Flowy client failure onto the local HTTP error envelope.
    ///
    /// Auth failures (missing/expired cloud JWT) become 401 even when the
    /// upstream envelope used 400, so the SPA can prompt cloud re-login.
    pub fn into_app_error(self) -> AppError {
        match self {
            Self::AuthRequired(msg) => AppError::Unauthorized(msg),
            Self::Cloud(CloudError::AuthFailed(msg)) => AppError::Unauthorized(msg),
            Self::MissingBaseUrl => {
                AppError::BadRequest("server base_url not configured".into())
            }
            Self::Disabled => AppError::BadRequest("server client disabled".into()),
            Self::NotConfigured(msg) => AppError::BadRequest(msg),
            Self::Api { code, msg }
                if is_auth_status(code) || is_auth_failure_message(&msg) =>
            {
                AppError::Unauthorized(msg)
            }
            Self::Api { code: 400, msg } => AppError::BadRequest(msg),
            Self::Server { status, body, .. }
                if is_auth_status(status as i32)
                    || (status == 400 && is_auth_failure_message(&body)) =>
            {
                AppError::Unauthorized(truncate_diag(&body))
            }
            Self::Server {
                status: 400,
                body,
                ..
            } => AppError::BadRequest(truncate_diag(&body)),
            other => AppError::BadGateway(truncate_diag(&other.to_string())),
        }
    }
}

fn is_auth_status(code: i32) -> bool {
    code == 401 || code == 403
}

fn is_auth_failure_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("invalid or expired token")
        || lower.contains("authentication required")
        || lower.contains("not authenticated")
        || lower.contains("not logged in")
        || lower.contains("cloud login required")
        || (lower.contains("token")
            && (lower.contains("expired")
                || lower.contains("invalid")
                || lower.contains("missing")
                || lower.contains("revoked")))
}

fn truncate_diag(message: &str) -> String {
    const MAX: usize = 500;
    let trimmed = message.trim();
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }
    trimmed.chars().take(MAX).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;

    fn status_of(err: ServerClientError) -> StatusCode {
        err.into_app_error().status_code()
    }

    #[test]
    fn auth_required_invalid_token_is_unauthorized() {
        let err = ServerClientError::AuthRequired("Invalid or expired token".into());
        assert_eq!(status_of(err), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn api_400_invalid_token_is_unauthorized() {
        let err = ServerClientError::Api {
            code: 400,
            msg: "Invalid or expired token".into(),
        };
        assert_eq!(status_of(err), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn api_401_and_403_are_unauthorized() {
        for code in [401, 403] {
            let err = ServerClientError::Api {
                code,
                msg: "token expired".into(),
            };
            assert_eq!(status_of(err), StatusCode::UNAUTHORIZED);
        }
    }

    #[test]
    fn http_400_401_403_invalid_token_are_unauthorized() {
        for status in [400_u16, 401, 403] {
            let err = ServerClientError::from_http_status(
                status,
                "Invalid or expired token".into(),
                None,
            );
            assert_eq!(status_of(err), StatusCode::UNAUTHORIZED);
        }
    }

    #[test]
    fn api_400_validation_stays_bad_request() {
        let err = ServerClientError::Api {
            code: 400,
            msg: "page must be positive".into(),
        };
        assert_eq!(status_of(err), StatusCode::BAD_REQUEST);
    }
}

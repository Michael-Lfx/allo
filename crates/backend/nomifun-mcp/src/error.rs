use nomifun_common::AppError;

/// MCP crate-level errors.
///
/// Uses `thiserror` (library crate convention).
/// Converts to `AppError` for HTTP response mapping.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("MCP server not found: {0}")]
    NotFound(String),

    #[error("MCP server name conflict: {0}")]
    Conflict(String),

    #[error("Invalid MCP server edit: {0}")]
    InvalidEdit(String),

    #[error("Invalid transport configuration: {0}")]
    InvalidTransport(String),

    #[error("Agent CLI not installed: {0}")]
    AgentNotInstalled(String),

    #[error("Agent operation failed: {0}")]
    AgentOperationFailed(String),

    #[error("Connection test failed: {0}")]
    ConnectionFailed(String),

    #[error("OAuth error: {0}")]
    OAuth(String),

    /// The authorization server publishes no registration endpoint and no
    /// pre-registered client identity is configured for this connector.
    #[error("OAuth pre-registered client required: {0}")]
    PreRegisteredClientRequired(String),

    /// The authorization server advertises no RFC 7591 registration endpoint
    /// (raised when dynamic registration was explicitly attempted).
    #[error("OAuth registration not supported: {0}")]
    RegistrationNotSupported(String),

    /// RFC 7591 dynamic registration failed at the authorization server.
    #[error("OAuth dynamic registration failed: {0}")]
    DynamicRegistrationFailed(String),

    /// RFC 7591 registration response was malformed (e.g. missing client_id).
    #[error("Invalid OAuth registration response: {0}")]
    InvalidRegistrationResponse(String),

    /// The authorization server rejected the redirect URI.
    #[error("OAuth redirect URI not allowed: {0}")]
    RedirectUriNotAllowed(String),

    /// The configured callback/redirect setup is not supported (non-loopback
    /// host, missing port, path mismatch on the callback listener).
    #[error("Unsupported OAuth auth setup: {0}")]
    UnsupportedAuth(String),

    #[error("OAuth reauthorization required")]
    ReauthorizationRequired,

    #[error("{0}")]
    Database(#[from] nomifun_db::DbError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl McpError {
    /// Stable client-facing code for OAuth flows (RFC 7591/8414/9728), or
    /// `None` for errors that carry no structured OAuth code. Mirrors the
    /// codes defined in `docs/agent-store/nomifun-mcp-oauth-dynamic-client-registration-design.md` §6.2/§8.
    pub fn oauth_error_code(&self) -> Option<&'static str> {
        match self {
            McpError::PreRegisteredClientRequired(_) => Some("pre_registered_client_required"),
            McpError::RegistrationNotSupported(_) => Some("registration_not_supported"),
            McpError::DynamicRegistrationFailed(_) => Some("dynamic_registration_failed"),
            McpError::InvalidRegistrationResponse(_) => Some("invalid_registration_response"),
            McpError::RedirectUriNotAllowed(_) => Some("redirect_uri_not_allowed"),
            McpError::UnsupportedAuth(_) => Some("unsupported_auth"),
            McpError::ReauthorizationRequired => Some("reauthorization_required"),
            _ => None,
        }
    }
}

impl From<McpError> for AppError {
    fn from(err: McpError) -> Self {
        match err {
            McpError::NotFound(msg) => AppError::NotFound(msg),
            McpError::Conflict(msg) => AppError::Conflict(msg),
            McpError::InvalidEdit(msg) => AppError::BadRequest(msg),
            McpError::InvalidTransport(msg) => AppError::BadRequest(msg),
            McpError::AgentNotInstalled(msg) => AppError::BadRequest(msg),
            McpError::AgentOperationFailed(msg) => AppError::Internal(msg),
            McpError::ConnectionFailed(msg) => AppError::BadGateway(msg),
            McpError::OAuth(msg) => AppError::Internal(format!("OAuth error: {msg}")),
            McpError::PreRegisteredClientRequired(msg) => {
                AppError::BadRequest(format!("pre_registered_client_required: {msg}"))
            }
            McpError::RegistrationNotSupported(msg) => {
                AppError::BadRequest(format!("registration_not_supported: {msg}"))
            }
            McpError::DynamicRegistrationFailed(msg) => {
                AppError::BadGateway(format!("dynamic_registration_failed: {msg}"))
            }
            McpError::InvalidRegistrationResponse(msg) => {
                AppError::BadGateway(format!("invalid_registration_response: {msg}"))
            }
            McpError::RedirectUriNotAllowed(msg) => {
                AppError::BadRequest(format!("redirect_uri_not_allowed: {msg}"))
            }
            McpError::UnsupportedAuth(msg) => {
                AppError::BadRequest(format!("unsupported_auth: {msg}"))
            }
            McpError::ReauthorizationRequired => {
                AppError::Unauthorized("OAuth reauthorization required".into())
            }
            McpError::Database(db_err) => AppError::from(db_err),
            McpError::Json(e) => AppError::Internal(format!("JSON error: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_maps_to_app_not_found() {
        let err: AppError = McpError::NotFound("mcp_123".into()).into();
        assert!(matches!(err, AppError::NotFound(msg) if msg == "mcp_123"));
    }

    #[test]
    fn conflict_maps_to_app_conflict() {
        let err: AppError = McpError::Conflict("test-server".into()).into();
        assert!(matches!(err, AppError::Conflict(_)));
    }

    #[test]
    fn invalid_transport_maps_to_bad_request() {
        let err: AppError = McpError::InvalidTransport("missing command".into()).into();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn invalid_edit_maps_to_bad_request() {
        let err: AppError = McpError::InvalidEdit("rename forbidden".into()).into();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn agent_not_installed_maps_to_bad_request() {
        let err: AppError = McpError::AgentNotInstalled("claude".into()).into();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn agent_operation_failed_maps_to_internal() {
        let err: AppError = McpError::AgentOperationFailed("exit code 1".into()).into();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn connection_failed_maps_to_bad_gateway() {
        let err: AppError = McpError::ConnectionFailed("timeout".into()).into();
        assert!(matches!(err, AppError::BadGateway(_)));
    }

    #[test]
    fn oauth_maps_to_internal() {
        let err: AppError = McpError::OAuth("discovery failed".into()).into();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn reauthorization_required_maps_to_unauthorized() {
        let err: AppError = McpError::ReauthorizationRequired.into();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[test]
    fn json_error_maps_to_internal() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let err: AppError = McpError::Json(json_err).into();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn display_messages() {
        assert_eq!(
            McpError::NotFound("mcp_1".into()).to_string(),
            "MCP server not found: mcp_1"
        );
        assert_eq!(
            McpError::InvalidTransport("bad".into()).to_string(),
            "Invalid transport configuration: bad"
        );
        assert_eq!(
            McpError::InvalidEdit("rename forbidden".into()).to_string(),
            "Invalid MCP server edit: rename forbidden"
        );
    }
}

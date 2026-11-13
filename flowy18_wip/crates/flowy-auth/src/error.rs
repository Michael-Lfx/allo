use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid email")]
    InvalidEmail,
    #[error("otp required")]
    OtpRequired,
    #[error("invalid code")]
    InvalidCode,
    #[error("session expired")]
    SessionExpired,
    #[error("transport")]
    Transport,
    #[error("server: {0}")]
    Server(String),
    #[error("store")]
    Store,
    #[error("auth required")]
    AuthRequired,
}

impl AuthError {
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::InvalidEmail => "auth.invalid_email",
            Self::OtpRequired => "auth.otp_required",
            Self::InvalidCode => "auth.invalid_code",
            Self::SessionExpired | Self::AuthRequired => "auth.session_expired",
            Self::Transport => "auth.network",
            Self::Server(_) => "auth.unknown",
            Self::Store => "auth.store",
        }
    }
}

//! Authentication types shared by login providers.

use chrono::{DateTime, Utc};
use nomi_config::ServerLoginMethod;
use serde::{Deserialize, Serialize};

use crate::session::ServerTokens;

/// Supported remote server login methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    WechatQr,
    EmailOtp,
}

impl LoginMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WechatQr => "wechat_qr",
            Self::EmailOtp => "email_otp",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wechat" | "wechat_qr" | "wx" => Some(Self::WechatQr),
            "email" | "email_otp" | "otp" => Some(Self::EmailOtp),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WechatQr => "WeChat QR scan",
            Self::EmailOtp => "Email verification code",
        }
    }
}

impl From<ServerLoginMethod> for LoginMethod {
    fn from(value: ServerLoginMethod) -> Self {
        match value {
            ServerLoginMethod::WechatQr => Self::WechatQr,
            ServerLoginMethod::EmailOtp => Self::EmailOtp,
        }
    }
}

impl From<LoginMethod> for ServerLoginMethod {
    fn from(value: LoginMethod) -> Self {
        match value {
            LoginMethod::WechatQr => Self::WechatQr,
            LoginMethod::EmailOtp => Self::EmailOtp,
        }
    }
}

/// In-progress login state returned to CLI/GUI surfaces.
#[derive(Debug, Clone)]
pub struct PendingLogin {
    pub method: LoginMethod,
    pub message: String,
    pub qr_content: Option<String>,
    pub qr_image_url: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Opaque provider state for polling/submit (serialized when needed).
    pub provider_state: Option<String>,
}

/// User input during a multi-step login flow.
#[derive(Debug, Clone)]
pub enum AuthUserInput {
    /// Poll provider without new user input (e.g. WeChat scan status).
    Poll,
    /// Submit email address to request OTP.
    Email { address: String },
    /// Submit OTP code after email was sent.
    OtpCode { code: String },
}

/// Result of a login poll/submit step.
#[derive(Debug, Clone)]
pub enum AuthPollResult {
    Pending(PendingLogin),
    Success(ServerTokens),
    Failed(String),
}

//! Cloud API endpoints and brand channel for email OTP.

pub const DEFAULT_BASE_URL: &str = "https://server.flowyaipc.com/claw";
pub const DEFAULT_CHANNEL: &str = "flowy";
pub const DEFAULT_APP: &str = "flowymes";
pub const EMAIL_OTP_LENGTH: usize = 6;
pub const EMAIL_OTP_COOLDOWN_SECS: u64 = 60;
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub base_url: String,
    pub channel: String,
    pub app: String,
    pub invite_code: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: env_or("FLOWY_API_BASE_URL", DEFAULT_BASE_URL),
            channel: env_or("FLOWY_AUTH_CHANNEL", DEFAULT_CHANNEL),
            app: env_or("FLOWY_AUTH_APP", DEFAULT_APP),
            invite_code: std::env::var("FLOWY_INVITE_CODE")
                .unwrap_or_default()
                .trim()
                .to_string(),
        }
    }
}

impl Config {
    #[must_use]
    pub fn effective_base_url(&self) -> String {
        self.base_url.trim().trim_end_matches('/').to_string()
    }
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

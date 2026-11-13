//! Blocking Flowy cloud client for email OTP login.

use crate::config::{Config, REQUEST_TIMEOUT_SECS};
use crate::error::AuthError;
use crate::session::Session;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct AuthClient {
    config: Config,
}

#[derive(Debug, Serialize)]
struct SendEmailCodeRequest {
    email: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    channel: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    app: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginByEmailRequest {
    email: String,
    valid_code: String,
    valid_code_req_no: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    invite_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    channel: String,
    device: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    app: String,
}

#[derive(Debug, Deserialize)]
struct FlowyEnvelope {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: i64,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

impl AuthClient {
    #[must_use]
    pub const fn new(config: Config) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn from_env() -> Self {
        Self::new(Config::default())
    }

    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    pub fn send_email_code(&self, email: &str) -> Result<String, AuthError> {
        let email = normalize_email(email);
        if !valid_email(&email) {
            return Err(AuthError::InvalidEmail);
        }
        let body = SendEmailCodeRequest {
            email,
            channel: self.config.channel.clone(),
            app: self.config.app.clone(),
        };
        let env = self.post("/user/getEmailRegisterValidCode", None, &body)?;
        env.into_data_string()
    }

    pub fn login_by_email(
        &self,
        email: &str,
        code: &str,
        req_no: &str,
    ) -> Result<String, AuthError> {
        let email = normalize_email(email);
        let code = normalize_otp(code);
        if !valid_email(&email) {
            return Err(AuthError::InvalidEmail);
        }
        if code.len() != crate::config::EMAIL_OTP_LENGTH {
            return Err(AuthError::OtpRequired);
        }
        if req_no.trim().is_empty() {
            return Err(AuthError::SessionExpired);
        }
        let body = LoginByEmailRequest {
            email,
            valid_code: code,
            valid_code_req_no: req_no.to_string(),
            invite_code: self.config.invite_code.clone(),
            channel: self.config.channel.clone(),
            device: String::new(),
            app: self.config.app.clone(),
        };
        let env = self.post("/user/doLoginByEmail", None, &body)?;
        let code = env.code;
        match env.into_jwt() {
            Ok(token) => Ok(token),
            Err(AuthError::Server(msg)) if is_invalid_otp(&msg, Some(code)) => {
                Err(AuthError::InvalidCode)
            }
            Err(error) => Err(map_verify_error(error)),
        }
    }

    pub fn complete_login(
        &self,
        email: &str,
        code: &str,
        req_no: &str,
    ) -> Result<Session, AuthError> {
        let email = normalize_email(email);
        let token = self.login_by_email(&email, code, req_no)?;
        match self.fetch_me(&token) {
            Ok(profile) => {
                let display_name = profile
                    .nickname
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| display_name_from_email(&email));
                let email = profile
                    .email
                    .filter(|value| !value.is_empty())
                    .unwrap_or(email);
                Ok(Session {
                    access_token: token,
                    email,
                    display_name,
                    user_id: Some(profile.id),
                })
            }
            Err(_) => Ok(Session {
                access_token: token,
                email: email.clone(),
                display_name: display_name_from_email(&email),
                user_id: None,
            }),
        }
    }

    pub fn fetch_me(&self, access_token: &str) -> Result<UserProfile, AuthError> {
        let env = self.get("/user/me", Some(access_token))?;
        env.into_data()
    }

    /// Lightweight reachability check against the configured API root.
    pub fn diagnose_network(&self) -> bool {
        let Ok(client) = http_client() else {
            return false;
        };
        let url = format!("{}/", self.config.effective_base_url());
        match client
            .get(&url)
            .timeout(Duration::from_secs(8))
            .send()
        {
            Ok(response) => response.status().as_u16() < 500,
            Err(_) => false,
        }
    }

    fn post(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: &impl Serialize,
    ) -> Result<FlowyEnvelope, AuthError> {
        let client = http_client()?;
        let url = self.url(path);
        let mut request = client
            .post(&url)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .header("Content-Type", "application/json")
            .header("User-Agent", user_agent())
            .json(body);
        if let Some(token) = bearer {
            request = request
                .header("Authorization", format!("Bearer {token}"))
                .header("token", token);
        }
        let response = request.send().map_err(|_| AuthError::Transport)?;
        parse_response(response)
    }

    fn get(
        &self,
        path: &str,
        bearer: Option<&str>,
    ) -> Result<FlowyEnvelope, AuthError> {
        let client = http_client()?;
        let url = self.url(path);
        let mut request = client
            .get(&url)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .header("User-Agent", user_agent());
        if let Some(token) = bearer {
            request = request
                .header("Authorization", format!("Bearer {token}"))
                .header("token", token);
        }
        let response = request.send().map_err(|_| AuthError::Transport)?;
        parse_response(response)
    }

    fn url(&self, path: &str) -> String {
        let base = self.config.effective_base_url();
        let path = path.trim().trim_start_matches('/');
        format!("{base}/{path}")
    }
}

impl FlowyEnvelope {
    fn into_jwt(self) -> Result<String, AuthError> {
        if self.code == 401 {
            return Err(AuthError::AuthRequired);
        }
        if self.code != 200 {
            return Err(AuthError::Server(self.msg));
        }
        match self.data {
            Value::String(token) if !token.is_empty() => Ok(token),
            _ => Err(AuthError::Server(
                "login response data is not a JWT string".into(),
            )),
        }
    }

    fn into_data_string(self) -> Result<String, AuthError> {
        if self.code == 401 {
            return Err(AuthError::AuthRequired);
        }
        if self.code != 200 {
            return Err(AuthError::Server(self.msg));
        }
        match self.data {
            Value::String(value) if !value.is_empty() => Ok(value),
            _ => Err(AuthError::Server(
                "success envelope missing string data".into(),
            )),
        }
    }

    fn into_data<T: for<'de> Deserialize<'de>>(self) -> Result<T, AuthError> {
        if self.code == 401 {
            return Err(AuthError::AuthRequired);
        }
        if self.code != 200 {
            return Err(AuthError::Server(self.msg));
        }
        serde_json::from_value(self.data).map_err(|_| {
            AuthError::Server("success envelope data decode failed".into())
        })
    }
}

fn parse_response(response: reqwest::blocking::Response) -> Result<FlowyEnvelope, AuthError> {
    let status = response.status().as_u16();
    let body = response.text().map_err(|_| AuthError::Transport)?;
    if status == 401 {
        return Err(AuthError::AuthRequired);
    }
    let env: FlowyEnvelope = serde_json::from_str(&body).map_err(|_| {
        if status >= 400 {
            AuthError::Transport
        } else {
            AuthError::Server("not valid Flowy JSON envelope".into())
        }
    })?;
    if env.code == 401 {
        return Err(AuthError::AuthRequired);
    }
    if is_invalid_otp(&env.msg, Some(env.code)) {
        return Err(AuthError::InvalidCode);
    }
    if is_session_expired_message(&env.msg) {
        return Err(AuthError::SessionExpired);
    }
    if env.code != 200 && status >= 400 {
        return Err(AuthError::Server(env.msg));
    }
    Ok(env)
}

fn http_client() -> Result<reqwest::blocking::Client, AuthError> {
    flowy_net::blocking_client().map_err(|_| AuthError::Transport)
}

fn user_agent() -> String {
    format!("FlowyGPUI/{}", env!("CARGO_PKG_VERSION"))
}

#[must_use]
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

#[must_use]
pub fn normalize_otp(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_digit)
        .take(crate::config::EMAIL_OTP_LENGTH)
        .collect()
}

#[must_use]
pub fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
}

#[must_use]
pub fn display_name_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let mut chars = local.chars();
    let Some(first) = chars.next() else {
        return "用户".to_string();
    };
    let mut name = first.to_uppercase().collect::<String>();
    name.extend(chars);
    name
}

fn map_verify_error(error: AuthError) -> AuthError {
    match error {
        AuthError::Server(msg) if is_session_expired_message(&msg) => {
            AuthError::SessionExpired
        }
        AuthError::Server(msg) if is_invalid_otp(&msg, None) => AuthError::InvalidCode,
        other => other,
    }
}

fn is_session_expired_message(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("expired")
        || normalized.contains("session expired")
        || message.contains("过期")
        || message.contains("会话失效")
}

fn is_invalid_otp(message: &str, status_like_code: Option<i32>) -> bool {
    if is_session_expired_message(message) {
        return false;
    }
    let normalized = message.to_ascii_lowercase();
    let explicit = message.contains("验证码错误")
        || message.contains("验证码无效")
        || message.contains("验证码不正确")
        || ((normalized.contains("invalid")
            || normalized.contains("incorrect")
            || normalized.contains("wrong"))
            && (normalized.contains("code")
                || normalized.contains("otp")
                || normalized.contains("verification")));
    matches!(status_like_code, Some(400 | 422)) || explicit
}

#[cfg(test)]
mod tests {
    use super::{is_invalid_otp, is_session_expired_message, normalize_otp, valid_email};

    #[test]
    fn email_and_otp_helpers() {
        assert!(valid_email("ada@example.com"));
        assert!(!valid_email("not-an-email"));
        assert_eq!(normalize_otp("12a34b56"), "123456");
    }

    #[test]
    fn classifies_invalid_otp_without_expiry() {
        assert!(is_invalid_otp("验证码无效", Some(422)));
        assert!(is_invalid_otp("wrong verification code", Some(500)));
        assert!(!is_invalid_otp("verification code expired", Some(422)));
    }

    #[test]
    fn attempt_limits_do_not_expire_the_otp_session() {
        assert!(!is_session_expired_message("too many attempts"));
        assert!(!is_session_expired_message("验证码错误次数过多"));
        assert!(is_invalid_otp("验证码错误次数过多", Some(400)));
    }

    #[test]
    fn only_genuine_expiry_ends_the_otp_session() {
        assert!(is_session_expired_message("verification code expired"));
        assert!(is_session_expired_message("验证码已过期"));
        assert!(is_session_expired_message("会话失效，请重新登录"));
    }
}

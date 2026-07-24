//! Flowy cloud account HTTP DTOs (email OTP login).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLoginStartRequest {
    #[serde(default = "default_email_method")]
    pub method: String,
}

fn default_email_method() -> String {
    "email_otp".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLoginStartResponse {
    pub pending_id: String,
    pub method: String,
    pub message: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CloudLoginInput {
    Email { address: String },
    OtpCode { code: String },
    Poll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLoginContinueRequest {
    pub pending_id: String,
    pub input: CloudLoginInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLoginPendingResponse {
    pub status: String,
    pub pending_id: String,
    pub method: String,
    pub message: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLoginSuccessResponse {
    pub status: String,
    pub authenticated: bool,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLoginFailedResponse {
    pub status: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWhoamiResponse {
    pub authenticated: bool,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
    pub server_base_url: Option<String>,
    /// Subscription plan display label, e.g. "Free" / "Pro" / "Ultra".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Raw plan code from server, e.g. "FreePlan" / "ProPlan".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudServerSettingsResponse {
    pub enabled: bool,
    pub base_url: String,
    pub channel: String,
    pub app: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCloudServerSettingsRequest {
    pub enabled: Option<bool>,
    pub base_url: Option<String>,
    pub channel: Option<String>,
    pub app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDeviceActivationStatusResponse {
    pub authenticated: bool,
    pub serial_number: Option<String>,
    pub app_version: Option<String>,
    pub activated_for_version: bool,
    pub last_reported_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDeviceActivationRetryResponse {
    pub reported: bool,
}

/// Result of syncing the Flowy chat model catalog into the local builtin provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncModelsResponse {
    /// True when the upstream catalog was fetched and written to the local provider row.
    pub synced: bool,
}

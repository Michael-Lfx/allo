//! Flowy media settings / credits / workflow history DTOs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSettingsResponse {
    pub provider: String,
    pub image_model: String,
    pub video_model: String,
    pub image_save_locally: bool,
    pub video_save_locally: bool,
    pub video_default_duration: u32,
    pub video_default_aspect_ratio: String,
    pub video_default_resolution: String,
    pub workflows_enabled: bool,
    pub workflows_max_retries: u32,
    pub workflows_async_execution: bool,
    pub workflows_llm_prompt_refine: bool,
    pub workflows_check_credits: bool,
    pub flowy_media_exposed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateMediaSettingsRequest {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub image_model: Option<String>,
    #[serde(default)]
    pub video_model: Option<String>,
    #[serde(default)]
    pub image_save_locally: Option<bool>,
    #[serde(default)]
    pub video_save_locally: Option<bool>,
    #[serde(default)]
    pub video_default_duration: Option<u32>,
    #[serde(default)]
    pub video_default_aspect_ratio: Option<String>,
    #[serde(default)]
    pub workflows_enabled: Option<bool>,
    #[serde(default)]
    pub workflows_max_retries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaCreditsResponse {
    pub balance: i64,
    pub authenticated: bool,
}

/// Per-turn Flowy credit usage (`GET /api/media/credits/usage-by-turn`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaTurnCreditUsage {
    pub turn_id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub call_count: i32,
    #[serde(default)]
    pub credits_consumed: i64,
    #[serde(default)]
    pub calls: Vec<MediaTurnCreditUsageCall>,
    /// False when the local session has no JWT (caller should hide the chip).
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaTurnCreditUsageCall {
    #[serde(default)]
    pub chat_id: i64,
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub channel_model_id: Option<i64>,
    #[serde(default)]
    pub prompt_tokens: Option<i64>,
    #[serde(default)]
    pub completion_tokens: Option<i64>,
    #[serde(default)]
    pub cache_tokens: Option<i64>,
    #[serde(default)]
    pub credit_consumed: i64,
    #[serde(default)]
    pub call_status: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Body for `POST /api/media/credits/checkin`. Wire field is `timeZone`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCreditsCheckinRequest {
    pub time_zone: String,
}

/// Result of a daily check-in. Mirrors the upstream Flowy response plus an
/// `authenticated` flag so the client gets a uniform signal. `balance` is the
/// post-check-in balance, so the client can update its display without a
/// follow-up balance fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCreditsCheckinResponse {
    pub already_checked_in: bool,
    #[serde(default)]
    pub granted_points: i64,
    #[serde(default)]
    pub balance: i64,
    #[serde(default)]
    pub check_in_at: Option<String>,
    #[serde(default)]
    pub day_key: Option<i64>,
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaWorkflowHistoryItem {
    pub run_id: String,
    pub workflow_id: String,
    pub status: String,
    pub current_step: Option<String>,
    pub error: Option<String>,
    pub artifacts: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaWorkflowHistoryResponse {
    pub runs: Vec<MediaWorkflowHistoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaModelOption {
    /// Flowy catalog id — pass as image/video request `model`.
    pub id: String,
    /// Human-readable catalog name for UI labels.
    pub name: String,
    /// Optional catalog icon URL from Flowy (`icon` field).
    #[serde(default)]
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaModelListResponse {
    pub image_models: Vec<MediaModelOption>,
    pub video_models: Vec<MediaModelOption>,
}

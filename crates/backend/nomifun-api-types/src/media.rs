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
    pub workflows_enabled: Option<bool>,
    #[serde(default)]
    pub workflows_max_retries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaCreditsResponse {
    pub balance: i64,
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
pub struct MediaModelListResponse {
    pub image_models: Vec<String>,
    pub video_models: Vec<String>,
}

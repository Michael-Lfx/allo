use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefingSessionSummary {
    pub id: String,
    pub title: String,
    pub stage: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_video: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefingSessionListResponse {
    pub sessions: Vec<BriefingSessionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefingCreateRequest {
    pub intent: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub format_secs: u32,
    #[serde(default)]
    pub research_depth: String,
    #[serde(default)]
    pub time_window_hours: u32,
    #[serde(default)]
    pub source_urls: Vec<String>,
    #[serde(default)]
    pub tts_provider_id: Option<String>,
    #[serde(default)]
    pub tts_model: Option<String>,
    #[serde(default)]
    pub tts_voice: Option<String>,
    #[serde(default)]
    pub image_provider_id: Option<String>,
    #[serde(default)]
    pub image_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BriefingModelsRequest {
    #[serde(default)]
    pub tts_provider_id: Option<String>,
    #[serde(default)]
    pub tts_model: Option<String>,
    #[serde(default)]
    pub tts_voice: Option<String>,
    #[serde(default)]
    pub image_provider_id: Option<String>,
    #[serde(default)]
    pub image_model: Option<String>,
}

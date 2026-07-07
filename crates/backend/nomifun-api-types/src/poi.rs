use serde::{Deserialize, Serialize};

/// Wire representation of a POI topic row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoiTopicResponse {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub weight: f64,
    pub status: String,
    pub source: String,
    pub confidence: f64,
    pub evidence_count: u32,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub last_seen_at: String,
}

/// Response for `GET /api/poi/topics`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoiTopicListResponse {
    pub topics: Vec<PoiTopicResponse>,
    pub database_path: String,
}

/// Pipeline + config summary for `GET /api/poi/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoiStatusResponse {
    pub enabled: bool,
    pub extract_mode: String,
    pub per_turn_buffer: bool,
    pub per_turn_persist: bool,
    pub session_end_llm: bool,
    pub topic_count: u32,
    pub database_path: String,
}

/// Request body for `POST /api/poi/topics/{id}/pin`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PoiPinRequest {
    #[serde(default = "default_pin_enabled")]
    pub pinned: bool,
}

fn default_pin_enabled() -> bool {
    true
}

/// Request body for `PUT /api/poi/topics/{id}/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoiTopicStatusRequest {
    pub status: String,
}

/// Interest (POI) settings exposed to the web UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PoiSettingsResponse {
    pub enabled: bool,
    pub max_topics: u32,
    pub snapshot_top_k: u32,
    pub prefetch_top_k: u32,
    pub char_budget_snapshot: usize,
    pub char_budget_prefetch: usize,
    pub extract_mode: String,
    pub decay_half_life_days: f64,
    pub llm_on_session_end: bool,
    pub per_turn_buffer: bool,
    pub per_turn_persist: bool,
    pub promote_min_evidence: u32,
    pub promote_min_confidence: f64,
    pub min_turn_chars: u32,
}

/// Partial update for `PATCH /api/poi/settings`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePoiSettingsRequest {
    pub enabled: Option<bool>,
    pub max_topics: Option<u32>,
    pub snapshot_top_k: Option<u32>,
    pub prefetch_top_k: Option<u32>,
    pub char_budget_snapshot: Option<usize>,
    pub char_budget_prefetch: Option<usize>,
    pub extract_mode: Option<String>,
    pub decay_half_life_days: Option<f64>,
    pub llm_on_session_end: Option<bool>,
    pub per_turn_buffer: Option<bool>,
    pub per_turn_persist: Option<bool>,
    pub promote_min_evidence: Option<u32>,
    pub promote_min_confidence: Option<f64>,
    pub min_turn_chars: Option<u32>,
}

impl UpdatePoiSettingsRequest {
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.max_topics.is_none()
            && self.snapshot_top_k.is_none()
            && self.prefetch_top_k.is_none()
            && self.char_budget_snapshot.is_none()
            && self.char_budget_prefetch.is_none()
            && self.extract_mode.is_none()
            && self.decay_half_life_days.is_none()
            && self.llm_on_session_end.is_none()
            && self.per_turn_buffer.is_none()
            && self.per_turn_persist.is_none()
            && self.promote_min_evidence.is_none()
            && self.promote_min_confidence.is_none()
            && self.min_turn_chars.is_none()
    }
}

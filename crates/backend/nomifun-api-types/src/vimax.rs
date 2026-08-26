//! ViMax session list DTOs.

use serde::{Deserialize, Serialize};

/// Compact session data used by the Video Generation home and sidebar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VimaxSessionSummary {
    pub id: String,
    pub title: String,
    pub workflow: String,
    pub stage: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_video: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub credits_consumed: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VimaxSessionListResponse {
    pub sessions: Vec<VimaxSessionSummary>,
}

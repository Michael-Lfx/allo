//! ViMax campaign marketing DTOs — Flowy cloud `/vimax/campaigns/*` (camelCase).

use serde::{Deserialize, Serialize};

/// Server-computed campaign window. Clients must not infer this locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CampaignPhase {
    Upcoming,
    Ongoing,
    Ended,
    #[serde(other)]
    Unknown,
}

impl Default for CampaignPhase {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Shared fields across carousel, list, and detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignSummary {
    pub id: i64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub show_in_carousel: bool,
    #[serde(default)]
    pub show_in_list: bool,
    #[serde(default)]
    pub allow_submission: bool,
    #[serde(default)]
    pub can_submit: bool,
    pub start_at: String,
    pub end_at: String,
    #[serde(default)]
    pub phase: CampaignPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_sort: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Homepage carousel slide. `media_type` is `image` or `video`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignCarouselItem {
    pub id: i64,
    pub title: String,
    pub media_type: String,
    pub media_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_url: Option<String>,
    #[serde(default)]
    pub show_in_list: bool,
    #[serde(default)]
    pub allow_submission: bool,
    #[serde(default)]
    pub can_submit: bool,
    pub start_at: String,
    pub end_at: String,
    #[serde(default)]
    pub phase: CampaignPhase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignCarouselResponse {
    #[serde(default)]
    pub list: Vec<CampaignCarouselItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignListResponse {
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub page: i32,
    #[serde(default)]
    pub page_size: i32,
    #[serde(default)]
    pub list: Vec<CampaignSummary>,
}

/// List fields plus HTML body for the landing page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignDetail {
    #[serde(flatten)]
    pub summary: CampaignSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

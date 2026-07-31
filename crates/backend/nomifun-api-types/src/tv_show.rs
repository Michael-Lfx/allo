//! ViMax TV Show DTOs — Flowy cloud `/vimax/tv-show/*` (camelCase).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TvShowAuthor {
    pub id: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TvShowVideo {
    pub id: i64,
    pub title: String,
    pub cover_url: String,
    pub workflow: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_duration_secs: Option<i32>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub author: TvShowAuthor,
    #[serde(default)]
    pub like_count: i64,
    #[serde(default)]
    pub view_count: i64,
    #[serde(default)]
    pub liked: bool,
    #[serde(default)]
    pub is_mine: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<String>,
    /// Detail-only fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_version: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TvShowListResponse {
    pub total: i64,
    pub page: i32,
    pub page_size: i32,
    pub list: Vec<TvShowVideo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TvShowPublishRequest {
    pub client_session_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub workflow: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_duration_secs: Option<i32>,
    pub cover_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_object_key: Option<String>,
    pub package_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_version: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TvShowPublishResponse {
    pub id: i64,
    pub client_session_id: String,
    pub title: String,
    pub status: String,
    pub cover_url: String,
    pub package_url: String,
    pub workflow: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    pub author: TvShowAuthor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TvShowLikeResponse {
    pub id: i64,
    pub liked: bool,
    pub like_count: i64,
}

/// Local HTTP body for publishing a session to TV Show (orchestration endpoint).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TvShowPublishSessionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

//! ViMax Skill Hub DTOs — Flowy cloud `/vimax/skills/*` (camelCase).
//! Spec: `allo/docs/vimax-skill-hub-api-client.md`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VimaxSkillAuthor {
    pub id: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VimaxCloudSkill {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub compatible_modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_scenario: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how_to_use: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_url: Option<String>,
    #[serde(default)]
    pub install_count: i64,
    #[serde(default)]
    pub like_count: i64,
    #[serde(default)]
    pub liked: bool,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
    #[serde(default)]
    pub is_mine: bool,
    pub author: VimaxSkillAuthor,
    /// Detail-only / author-only fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_skill_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VimaxCloudSkillListResponse {
    pub total: i64,
    pub page: i32,
    pub page_size: i32,
    pub list: Vec<VimaxCloudSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VimaxCloudSkillPublishRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_scenario: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how_to_use: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compatible_modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirement_overlay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_overlay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playbook: Option<String>,
    pub package_url: String,
    pub package_object_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_skill_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VimaxCloudSkillPublishResponse {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    pub status: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VimaxCloudSkillInstallResponse {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub package_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualified_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VimaxCloudSkillLikeResponse {
    pub id: i64,
    pub liked: bool,
    pub like_count: i64,
}

/// Local HTTP body when publishing a local skill to cloud Skill Hub.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VimaxCloudSkillPublishLocalRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_url: Option<String>,
}

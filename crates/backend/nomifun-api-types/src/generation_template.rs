//! Canvas Generation Template DTOs — Flowy `/generation-templates/*` (camelCase).
//! Independent of Skill Hub. Spec: FlowyClaw `docs/generation-templates-api.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateAuthor {
    pub id: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateTarget {
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default)]
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplatePrompt {
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateSlot {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateModelIntent {
    #[serde(default)]
    pub preferred: String,
    #[serde(default)]
    pub fallbacks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateAsset {
    #[serde(default)]
    pub id: i64,
    pub role: String,
    #[serde(default)]
    pub sort: i32,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateListItem {
    pub id: i64,
    pub slug: String,
    pub version: String,
    pub title: String,
    #[serde(default)]
    pub title_en: String,
    pub job: String,
    #[serde(default)]
    pub job_en: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub featured_rank: i32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub apply_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    #[serde(default)]
    pub estimated_credits: i32,
    #[serde(default)]
    pub impression_count: i32,
    #[serde(default)]
    pub apply_count: i32,
    #[serde(default)]
    pub generate_count: i32,
    #[serde(default)]
    pub succeed_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<String>,
    #[serde(default)]
    pub submitted_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    pub author: GenerationTemplateAuthor,
    #[serde(default)]
    pub is_mine: bool,
    #[serde(default)]
    pub target: GenerationTemplateTarget,
    #[serde(default)]
    pub model_intent: GenerationTemplateModelIntent,
    #[serde(default)]
    pub asset_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateDetail {
    #[serde(flatten)]
    pub item: GenerationTemplateListItem,
    #[serde(default)]
    pub prompt: GenerationTemplatePrompt,
    #[serde(default)]
    pub slots: Vec<GenerationTemplateSlot>,
    #[serde(default)]
    pub assets: Vec<GenerationTemplateAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remix_of_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_external_channel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateListResponse {
    pub total: i64,
    pub page: i32,
    pub page_size: i32,
    pub list: Vec<GenerationTemplateListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplateEventRequest {
    #[serde(rename = "type")]
    pub event_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTemplatePublishRequest {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_en: Option<String>,
    pub job: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_en: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub target: GenerationTemplateTarget,
    pub prompt: GenerationTemplatePrompt,
    #[serde(default)]
    pub slots: Vec<GenerationTemplateSlot>,
    #[serde(default)]
    pub model_intent: GenerationTemplateModelIntent,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_credits: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remix_of_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_node_id: Option<String>,
    #[serde(default)]
    pub assets: Vec<GenerationTemplateAsset>,
}

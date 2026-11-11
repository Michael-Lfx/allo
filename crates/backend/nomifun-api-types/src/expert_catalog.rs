//! Expert catalog DTOs for Flowy `GET /experts`.
//!
//! List rows and detail payloads use the cloud's camelCase names. Empty and
//! null optional fields decode to empty values so a missing README or an
//! unrecorded checksum does not fail the whole response.

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn string_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn i64_or_zero<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<i64>::deserialize(deserializer)?.unwrap_or_default())
}

fn bool_or_false<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(deserializer)?.unwrap_or_default())
}

fn text_or_empty<'de, D>(deserializer: D) -> Result<ExpertText, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<ExpertText>::deserialize(deserializer)?.unwrap_or_default())
}

fn text_vec_or_empty<'de, D>(deserializer: D) -> Result<Vec<ExpertText>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<ExpertText>>::deserialize(deserializer)?.unwrap_or_default())
}

fn summary_vec_or_empty<'de, D>(deserializer: D) -> Result<Vec<ExpertSummary>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<ExpertSummary>>::deserialize(deserializer)?.unwrap_or_default())
}

fn json_vec_or_empty<'de, D>(deserializer: D) -> Result<Vec<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertText {
    #[serde(default, deserialize_with = "string_or_empty")]
    pub zh: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub en: String,
}

fn id_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text),
        Some(Value::Number(number)) => Ok(number.to_string()),
        Some(_) => Err(serde::de::Error::custom("expert id must be a string or number")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertSummary {
    /// Catalog row id used by `POST /api/v1/experts/:id/download`.
    #[serde(default, deserialize_with = "id_or_empty")]
    pub id: String,
    pub slug: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub expert_type: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub name: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub name_en: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub profession: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub profession_en: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub summary: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub summary_en: String,
    #[serde(default, deserialize_with = "text_vec_or_empty")]
    pub tags: Vec<ExpertText>,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub version: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub download_url: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub sha256: String,
    #[serde(default, deserialize_with = "i64_or_zero")]
    pub file_count: i64,
    #[serde(default, deserialize_with = "i64_or_zero")]
    pub download_count: i64,
    #[serde(default, deserialize_with = "i64_or_zero")]
    pub zip_size_bytes: i64,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub avatar_path: String,
    /// CDN image URL from `avatarUrl`. Empty when the catalog row has no image.
    #[serde(default, deserialize_with = "string_or_empty")]
    pub avatar_url: String,
    #[serde(default, deserialize_with = "bool_or_false")]
    pub has_detail: bool,
    #[serde(default, deserialize_with = "i64_or_zero")]
    pub sort: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertListResponse {
    #[serde(default, deserialize_with = "summary_vec_or_empty")]
    pub list: Vec<ExpertSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertDetail {
    #[serde(flatten)]
    pub summary: ExpertSummary,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub description: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub detail_markdown: String,
    #[serde(default, deserialize_with = "string_or_empty")]
    pub agent_name: String,
    #[serde(default, deserialize_with = "text_or_empty")]
    pub default_init_prompt: ExpertText,
    #[serde(default, deserialize_with = "json_vec_or_empty")]
    pub quick_prompts: Vec<Value>,
    #[serde(default)]
    pub team_info: Option<Value>,
    #[serde(default, deserialize_with = "json_vec_or_empty")]
    pub members: Vec<Value>,
}

impl ExpertDetail {
    pub fn slug(&self) -> &str {
        &self.summary.slug
    }
}

/// Result of installing one catalog expert as a local user preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertInstallResponse {
    pub preset_id: String,
    pub name: String,
    pub already_installed: bool,
    #[serde(default)]
    pub installed_skills: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Slugs whose catalog package is already a local preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertInstallIndex {
    pub slugs: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_row_decodes_null_optional_fields() {
        let raw = r#"{
            "slug": "demo-expert",
            "expertType": "agent",
            "name": "演示",
            "nameEn": "Demo",
            "profession": null,
            "summary": "摘要",
            "summaryEn": "",
            "tags": [{"zh": "后端", "en": "Backend"}],
            "version": "1.2.0",
            "downloadUrl": "https://example.com/demo-expert.zip",
            "sha256": null,
            "fileCount": null,
            "downloadCount": 3860,
            "zipSizeBytes": 2048,
            "avatarPath": "avatar.png",
            "avatarUrl": "https://cdn.example.com/experts/a.png",
            "hasDetail": true,
            "sort": 2
        }"#;
        let row: ExpertSummary = serde_json::from_str(raw).expect("row");
        assert_eq!(row.slug, "demo-expert");
        assert_eq!(row.expert_type, "agent");
        assert_eq!(row.name, "演示");
        assert_eq!(row.profession, "");
        assert_eq!(row.sha256, "");
        assert_eq!(row.file_count, 0);
        assert_eq!(row.download_count, 3860);
        assert_eq!(row.avatar_path, "avatar.png");
        assert_eq!(row.avatar_url, "https://cdn.example.com/experts/a.png");
        assert_eq!(row.zip_size_bytes, 2048);
        assert!(row.has_detail);
        assert_eq!(row.tags.len(), 1);
        assert_eq!(row.tags[0].zh, "后端");
        assert_eq!(row.id, "");
    }

    #[test]
    fn list_row_id_accepts_number_or_string() {
        let numeric: ExpertSummary = serde_json::from_str(r#"{"slug":"a","id":8801}"#).expect("number");
        assert_eq!(numeric.id, "8801");
        let text: ExpertSummary = serde_json::from_str(r#"{"slug":"a","id":"exp_1"}"#).expect("string");
        assert_eq!(text.id, "exp_1");
    }

    #[test]
    fn tag_list_skips_a_null_entry_by_failing_closed_on_bad_items() {
        let raw = r#"{"slug":"ok","tags":[null]}"#;
        assert!(serde_json::from_str::<ExpertSummary>(raw).is_err());
    }

    #[test]
    fn detail_decodes_empty_prompt_and_null_team() {
        let raw = r#"{
            "slug": "team-demo",
            "expertType": "team",
            "name": "专家团",
            "defaultInitPrompt": null,
            "quickPrompts": null,
            "teamInfo": null,
            "members": null,
            "detailMarkdown": "",
            "description": "plugin description"
        }"#;
        let detail: ExpertDetail = serde_json::from_str(raw).expect("detail");
        assert_eq!(detail.slug(), "team-demo");
        assert_eq!(detail.summary.expert_type, "team");
        assert_eq!(detail.description, "plugin description");
        assert_eq!(detail.default_init_prompt, ExpertText::default());
        assert!(detail.quick_prompts.is_empty());
        assert!(detail.team_info.is_none());
        assert!(detail.members.is_empty());
    }

    #[test]
    fn list_payload_defaults_when_list_is_null() {
        let raw = r#"{"list":null,"total":0,"page":1,"pageSize":20}"#;
        let page: ExpertListResponse = serde_json::from_str(raw).expect("page");
        assert!(page.list.is_empty());
        assert_eq!(page.total, Some(0));
        assert_eq!(page.page_size, Some(20));
    }
}

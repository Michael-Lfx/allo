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

/// Read-only cloud credit history. Dates and day keys are resolved upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCreditsCheckinRecords {
    pub month: String,
    pub total_check_in_days: i64,
    pub list: Vec<MediaCreditsCheckinRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCreditsCheckinRecord {
    pub date: String,
    pub day: u32,
    pub day_key: i64,
    pub check_in_at: String,
    pub granted_points: i64,
    pub time_zone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCreditsUsageByType {
    #[serde(default)]
    pub server_time: String,
    #[serde(default)]
    pub balance: i64,
    #[serde(default)]
    pub include_team_seat: bool,
    #[serde(default)]
    pub list: Vec<MediaCreditsUsageTypeItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCreditsUsageTypeItem {
    #[serde(rename = "type")]
    pub usage_type: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_en: Option<String>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub used: i64,
    #[serde(default)]
    pub remaining: i64,
    #[serde(default)]
    pub buckets: Vec<MediaCreditsUsageBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCreditsUsageBucket {
    #[serde(default)]
    pub expire_at: Option<String>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub used: i64,
    #[serde(default)]
    pub remaining: i64,
    #[serde(default)]
    pub source_code: Option<String>,
}

pub fn valid_credits_month(month: &str) -> bool {
    let bytes = month.as_bytes();
    bytes.len() == 7
        && bytes[4] == b'-'
        && bytes.iter().enumerate().all(|(i, b)| i == 4 || b.is_ascii_digit())
        && month[..4].parse::<u32>().is_ok_and(|year| year > 0)
        && month[5..].parse::<u32>().is_ok_and(|month| (1..=12).contains(&month))
}

#[cfg(test)]
mod credit_history_tests {
    use super::*;

    #[test]
    fn month_validation_is_strict_and_unicode_safe() {
        for value in ["2026-01", "2026-12", "0001-01"] { assert!(valid_credits_month(value)); }
        for value in ["2026-00", "2026-13", "2026-1", "0000-01", "2026-10&x=1", "你你-1", ""] { assert!(!valid_credits_month(value)); }
    }

    #[test]
    fn cloud_usage_tolerates_missing_source_fields() {
        let usage: MediaCreditsUsageByType = serde_json::from_value(serde_json::json!({
            "list":[{"type":"PACK"}]
        })).unwrap();
        assert_eq!(usage.server_time, "");
        assert_eq!(usage.balance, 0);
        assert_eq!(usage.list.len(), 1);
        assert_eq!(usage.list[0].total, 0);
        assert_eq!(usage.list[0].remaining, 0);
        assert!(usage.list[0].buckets.is_empty());
        let bucket: MediaCreditsUsageBucket = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(bucket.expire_at.is_none());
        assert_eq!(bucket.remaining, 0);
    }

    #[test]
    fn cloud_history_preserves_day_keys_and_credit_buckets() {
        let records: MediaCreditsCheckinRecords = serde_json::from_value(serde_json::json!({
            "month":"2026-10", "totalCheckInDays":114,
            "list":[{"date":"2026-10-12","day":12,"dayKey":20261012,"checkInAt":"2026-10-11T23:30:00Z","grantedPoints":3000,"timeZone":"Asia/Shanghai"}]
        })).unwrap();
        assert_eq!(records.list[0].day_key, 20261012);
        assert_eq!(records.total_check_in_days, 114);
        let usage: MediaCreditsUsageByType = serde_json::from_value(serde_json::json!({
            "serverTime":"2026-10-12T00:00:00Z","balance":3000,"includeTeamSeat":false,
            "list":[{"type":"DAILY_CHECKIN","title":"签到","titleEn":"Check-in","total":3000,"used":0,"remaining":3000,
                "buckets":[{"expireAt":"2027-10-12T00:00:00Z","total":3000,"used":0,"remaining":3000}]}]
        })).unwrap();
        assert_eq!(usage.server_time, "2026-10-12T00:00:00Z");
        assert_eq!(usage.list[0].buckets[0].expire_at.as_deref(), Some("2027-10-12T00:00:00Z"));
        assert_eq!(serde_json::to_value(usage).unwrap()["list"][0]["type"], "DAILY_CHECKIN");
    }
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
    /// Flowy catalog id — pass as image / video / TTS request `model`.
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
    /// TTS models from `availableListClaw?category=8`. Omitted by older servers.
    #[serde(default)]
    pub audio_models: Vec<MediaModelOption>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_model_list_defaults_audio_models_when_omitted() {
        let parsed: MediaModelListResponse =
            serde_json::from_str(r#"{"image_models":[],"video_models":[]}"#).unwrap();
        assert!(parsed.audio_models.is_empty());
    }

    #[test]
    fn media_model_list_roundtrips_tts_catalog_entries() {
        let body = MediaModelListResponse {
            image_models: vec![],
            video_models: vec![],
            audio_models: vec![MediaModelOption {
                id: "AIPC-qwen3-tts".into(),
                name: "qwen3-tts".into(),
                icon: String::new(),
            }],
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["audio_models"][0]["id"], "AIPC-qwen3-tts");
        let parsed: MediaModelListResponse = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.audio_models[0].id, "AIPC-qwen3-tts");
    }
}

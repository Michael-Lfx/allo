//! Flowy image/video generation API types.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// `tb_model.category` for image models (`GET .../model/availableListClaw?category=6`).
pub const MODEL_CATEGORY_IMAGE: i32 = 6;

/// `tb_model.category` for video models (`GET .../model/availableListClaw?category=4`).
pub const MODEL_CATEGORY_VIDEO: i32 = 4;

/// Local `tb_video_task.status` — succeeded.
pub const VIDEO_TASK_STATUS_SUCCEEDED: i32 = 4;

/// Local `tb_video_task.status` — failed.
pub const VIDEO_TASK_STATUS_FAILED: i32 = 5;

/// Local `tb_video_task.status` — expired.
pub const VIDEO_TASK_STATUS_EXPIRED: i32 = 6;

/// Local `tb_video_task.status` — cancelled.
pub const VIDEO_TASK_STATUS_CANCELLED: i32 = 3;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateVideoTaskResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoTaskRecord {
    pub id: i64,
    #[serde(default)]
    pub task_id: Option<String>,
    pub status: i32,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl VideoTaskRecord {
    pub fn video_url(&self) -> Option<String> {
        self.result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.get("video_url"))
            .and_then(|u| u.as_str())
            .map(str::to_string)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            VIDEO_TASK_STATUS_CANCELLED
                | VIDEO_TASK_STATUS_SUCCEEDED
                | VIDEO_TASK_STATUS_FAILED
                | VIDEO_TASK_STATUS_EXPIRED
        )
    }

    pub fn is_success(&self) -> bool {
        self.status == VIDEO_TASK_STATUS_SUCCEEDED
    }

    /// Best-effort upstream failure reason from `result` JSON.
    pub fn failure_detail(&self) -> Option<String> {
        let result = self.result.as_ref()?;
        for key in ["error", "message", "fail_reason", "reason"] {
            if let Some(s) = result.get(key).and_then(|v| v.as_str()) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
        result
            .get("status")
            .and_then(|v| v.as_str())
            .filter(|s| {
                let lower = s.to_ascii_lowercase();
                lower.contains("fail") || lower.contains("error")
            })
            .map(str::to_string)
    }
}

/// Reference image / frame in a Seedance `content` array.
#[derive(Debug, Clone, Serialize)]
pub struct VideoContentImage {
    pub url: String,
    /// `first_frame`, `last_frame`, or `reference_image`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub role: String,
}

/// High-level parameters for building a Seedance video create body.
#[derive(Debug, Clone, Default)]
pub struct VideoCreateParams {
    pub model: String,
    pub prompt: String,
    pub duration: Option<u32>,
    pub aspect_ratio: String,
    pub resolution: Option<String>,
    pub negative_prompt: Option<String>,
    pub seed: Option<i64>,
    pub watermark: bool,
    pub generate_audio: Option<bool>,
    pub images: Vec<VideoContentImage>,
    pub reference_video_url: Option<String>,
    pub reference_audio_url: Option<String>,
}

impl VideoCreateParams {
    /// Build Ark-compatible `POST /video/generations/tasks` JSON.
    pub fn to_json(&self) -> Value {
        let mut content = vec![json!({"type": "text", "text": self.prompt})];

        for img in &self.images {
            let role = if img.role.trim().is_empty() {
                "reference_image".to_string()
            } else {
                img.role.clone()
            };
            content.push(json!({
                "type": "image_url",
                "image_url": {"url": img.url},
                "role": role,
            }));
        }
        if let Some(url) = self
            .reference_video_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
        {
            content.push(json!({
                "type": "video_url",
                "video_url": {"url": url},
                "role": "reference_video",
            }));
        }
        if let Some(url) = self
            .reference_audio_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
        {
            content.push(json!({
                "type": "audio_url",
                "audio_url": {"url": url},
                "role": "reference_audio",
            }));
        }

        let mut body = serde_json::Map::new();
        body.insert("model".into(), json!(self.model));
        body.insert("content".into(), Value::Array(content));
        body.insert("ratio".into(), json!(self.aspect_ratio));
        body.insert("watermark".into(), json!(self.watermark));
        if let Some(d) = self.duration {
            body.insert("duration".into(), json!(d));
        }
        if let Some(r) = self.resolution.as_deref().filter(|s| !s.is_empty()) {
            body.insert("resolution".into(), json!(r));
        }
        if let Some(neg) = self.negative_prompt.as_deref().filter(|s| !s.is_empty()) {
            body.insert("negative_prompt".into(), json!(neg));
        }
        if let Some(s) = self.seed {
            body.insert("seed".into(), json!(s));
        }
        if let Some(ga) = self.generate_audio {
            body.insert("generate_audio".into(), json!(ga));
        }
        Value::Object(body)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageGenerationRequest {
    pub model: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

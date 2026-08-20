//! Flowy image/video generation API types.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// `tb_model.category` for image models (`GET .../model/availableListClaw?category=6`).
pub const MODEL_CATEGORY_IMAGE: i32 = 6;

/// `tb_model.category` for video models (`GET .../model/availableListClaw?category=4`).
pub const MODEL_CATEGORY_VIDEO: i32 = 4;

/// `tb_model.category` for ASR models (`GET .../model/availableListClaw?category=7`).
pub const MODEL_CATEGORY_ASR: i32 = 7;

/// `tb_model.category` for TTS models (`GET .../model/availableListClaw?category=8`).
pub const MODEL_CATEGORY_TTS: i32 = 8;

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
        let content = self.result.as_ref().and_then(|r| r.get("content"))?;
        // Prefer normalized `video_url`; MiniMax-H3 may only expose upstream `url`
        // until the gateway mirrors it (server docs guarantee both when possible).
        for key in ["video_url", "url"] {
            if let Some(url) = content
                .get(key)
                .and_then(|u| u.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(url.to_string());
            }
        }
        None
    }

    /// Last-frame still URL when the create request set `return_last_frame=true`.
    /// Searches common Flowy / Ark response shapes.
    pub fn last_frame_url(&self) -> Option<String> {
        let result = self.result.as_ref()?;
        for path in [
            &["content", "last_frame_url"][..],
            &["content", "last_frame", "url"][..],
            &["last_frame_url"][..],
            &["data", "last_frame_url"][..],
            &["output", "last_frame_url"][..],
        ] {
            if let Some(url) = dig_str(result, path) {
                return Some(url);
            }
        }
        // Some gateways nest: result.content.last_frame.url / result.data[0].last_frame_url
        if let Some(arr) = result
            .get("data")
            .and_then(|d| d.as_array())
            .or_else(|| result.get("content").and_then(|c| c.get("data")).and_then(|d| d.as_array()))
        {
            for item in arr {
                if let Some(url) = item
                    .get("last_frame_url")
                    .and_then(|u| u.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    return Some(url.to_string());
                }
            }
        }
        None
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

/// High-level parameters for building a Flowy video create body.
///
/// Serialization branches on [`is_minimax_h3_model`]: Seedance/Ark fields vs MiniMax V2.
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
    /// When true, Seedance returns a still of the clip ending (`last_frame_url`).
    /// Ignored for MiniMax-H3 (not part of MiniMax V2 create schema).
    pub return_last_frame: Option<bool>,
    pub images: Vec<VideoContentImage>,
    pub reference_video_url: Option<String>,
    pub reference_audio_url: Option<String>,
}

/// True when `model` is MiniMax-H3 (Flowy id forms: `flowy/MiniMax-H3`, `AIPC-…`, bare name).
pub fn is_minimax_h3_model(model: &str) -> bool {
    let blob = model
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '.', ' ', '/'], "-");
    blob.contains("minimax-h3")
        || blob.contains("minimaxh3")
        // Catalog / gateway variants occasionally drop the hyphen.
        || (blob.contains("minimax") && blob.contains("h3"))
}

/// MiniMax-H3 create API resolutions (`768P` | `2K`).
pub const MINIMAX_H3_RESOLUTIONS: &[&str] = &["768P", "2K"];
pub const DEFAULT_MINIMAX_H3_RESOLUTION: &str = "768P";
pub const MINIMAX_H3_DURATION_MIN: u32 = 4;
pub const MINIMAX_H3_DURATION_MAX: u32 = 15;

/// Map UI / Seedance-style tokens onto MiniMax-H3 `resolution`.
pub fn normalize_minimax_h3_resolution(resolution: &str) -> String {
    let lower = resolution
        .trim()
        .to_ascii_lowercase()
        .replace('_', "")
        .replace(' ', "");
    match lower.as_str() {
        "2k" | "1080p" | "1080" | "2160p" | "4k" | "high" => "2K".into(),
        "768p" | "768" | "480p" | "720p" | "low" | "medium" | "auto" => {
            DEFAULT_MINIMAX_H3_RESOLUTION.into()
        }
        _ if MINIMAX_H3_RESOLUTIONS
            .iter()
            .any(|r| r.eq_ignore_ascii_case(resolution.trim())) =>
        {
            // Preserve canonical casing from the allow-list.
            MINIMAX_H3_RESOLUTIONS
                .iter()
                .find(|r| r.eq_ignore_ascii_case(resolution.trim()))
                .copied()
                .unwrap_or(DEFAULT_MINIMAX_H3_RESOLUTION)
                .to_string()
        }
        _ => DEFAULT_MINIMAX_H3_RESOLUTION.into(),
    }
}

pub fn clamp_minimax_h3_duration(duration: u32) -> u32 {
    duration.clamp(MINIMAX_H3_DURATION_MIN, MINIMAX_H3_DURATION_MAX)
}

impl VideoCreateParams {
    /// Build `POST /video/generations/tasks` JSON (Seedance Ark or MiniMax-H3 V2).
    pub fn to_json(&self) -> Value {
        if is_minimax_h3_model(&self.model) {
            self.to_minimax_h3_json()
        } else {
            self.to_seedance_json()
        }
    }

    fn build_content_array(&self) -> Vec<Value> {
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
        content
    }

    fn has_media_content(&self) -> bool {
        !self.images.is_empty()
            || self
                .reference_video_url
                .as_deref()
                .is_some_and(|u| !u.trim().is_empty())
            || self
                .reference_audio_url
                .as_deref()
                .is_some_and(|u| !u.trim().is_empty())
    }

    /// MiniMax V2 `/v2/video_generation` shape (gateway rewrites `model` upstream).
    fn to_minimax_h3_json(&self) -> Value {
        let content = self.build_content_array();
        let resolution = self
            .resolution
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(normalize_minimax_h3_resolution)
            .unwrap_or_else(|| DEFAULT_MINIMAX_H3_RESOLUTION.to_string());
        let duration = clamp_minimax_h3_duration(self.duration.unwrap_or(5));

        // Text-only: ratio is required and must not be `adaptive`.
        // Image / multimodal reference: always `adaptive` (upstream ignores other values).
        let ratio = if self.has_media_content() {
            "adaptive".to_string()
        } else {
            let r = self.aspect_ratio.trim();
            if r.is_empty() || r.eq_ignore_ascii_case("adaptive") || r.eq_ignore_ascii_case("auto")
            {
                "16:9".to_string()
            } else {
                r.to_string()
            }
        };

        let mut body = serde_json::Map::new();
        body.insert("model".into(), json!(self.model));
        body.insert("content".into(), Value::Array(content));
        body.insert("resolution".into(), json!(resolution));
        body.insert("duration".into(), json!(duration));
        body.insert("ratio".into(), json!(ratio));
        // Prefer MiniMax `aigc_watermark`; never send Ark `watermark`.
        if self.watermark {
            body.insert("aigc_watermark".into(), json!(true));
        }
        Value::Object(body)
    }

    /// Ark / Seedance create-task shape.
    fn to_seedance_json(&self) -> Value {
        let content = self.build_content_array();

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
        if let Some(rlf) = self.return_last_frame {
            body.insert("return_last_frame".into(), json!(rlf));
        }
        Value::Object(body)
    }
}

fn dig_str(value: &Value, path: &[&str]) -> Option<String> {
    let mut cur = value;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageGenerationRequest {
    pub model: String,
    pub prompt: String,
    /// Single reference (legacy). Prefer [`Self::image_urls`] for multi-ref img2img.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// Extra reference images for multi-ref img2img (Seedream `image: string[]`, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_urls: Vec<String>,
    #[serde(flatten)]
    pub extra: Value,
}

impl ImageGenerationRequest {
    /// Combined reference URLs (`image_url` first, then `image_urls`, de-duplicated).
    pub fn reference_urls(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        if let Some(u) = self
            .image_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            out.push(u);
        }
        for u in &self.image_urls {
            let t = u.trim();
            if !t.is_empty() && !out.iter().any(|x| *x == t) {
                out.push(t);
            }
        }
        out
    }
}

/// `POST /uploads/oss/presignPut` request body.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OssPresignPutRequest {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_seconds: Option<u64>,
}

/// `POST /uploads/oss/presignPut` success `data`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OssPresignPutData {
    /// Usually `"PUT"`.
    #[serde(default)]
    pub method: String,
    /// Presigned upload URL (query-signed; short-lived). Do **not** pass to video API.
    pub url: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Headers that must be sent verbatim on the upload PUT (at least `Content-Type`).
    #[serde(default)]
    pub required_headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub object_key: Option<String>,
    /// HTTPS download URL for video/image generation `content[].image_url.url`.
    #[serde(default)]
    pub public_url: Option<String>,
}

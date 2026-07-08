//! Video generation tool types and handler.

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::sync::Arc;

use nomi_types::{JsonSchema, ToolError, ToolHandler, ToolSchema, tool_schema};

/// Parameters for text-to-video or image-to-video generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoGenerateRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub model_explicit: bool,
    pub image_url: Option<String>,
    pub reference_image_urls: Vec<String>,
    pub duration: Option<u32>,
    pub aspect_ratio: String,
    pub resolution: String,
    pub negative_prompt: Option<String>,
    pub audio: Option<bool>,
    pub seed: Option<i64>,
    pub last_frame_url: Option<String>,
    pub reference_video_url: Option<String>,
    pub reference_audio_url: Option<String>,
    pub generate_audio: Option<bool>,
}

/// Backend for video generation operations.
#[async_trait]
pub trait VideoGenerateBackend: Send + Sync {
    async fn generate_video(&self, request: VideoGenerateRequest) -> Result<String, ToolError>;
}

/// Tool for generating videos from text prompts, optionally guided by a starting image.
pub struct VideoGenerateHandler {
    backend: Arc<dyn VideoGenerateBackend>,
}

impl VideoGenerateHandler {
    pub fn new(backend: Arc<dyn VideoGenerateBackend>) -> Self {
        Self { backend }
    }
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_string_list(params: &Value, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn optional_u32(params: &Value, key: &str) -> Option<u32> {
    params.get(key).and_then(|v| {
        v.as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u32>().ok()))
    })
}

fn optional_i64(params: &Value, key: &str) -> Option<i64> {
    params.get(key).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
    })
}

#[async_trait]
impl ToolHandler for VideoGenerateHandler {
    async fn execute(&self, params: Value) -> Result<String, ToolError> {
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidParams("Missing 'prompt' parameter".into()))?;

        let mut reference_image_urls = optional_string_list(&params, "reference_image_urls");
        if reference_image_urls.is_empty() {
            reference_image_urls = optional_string_list(&params, "reference_images");
        }

        let model = optional_string(&params, "model");
        let request = VideoGenerateRequest {
            prompt: prompt.to_string(),
            model_explicit: model.is_some(),
            model,
            image_url: optional_string(&params, "image_url"),
            reference_image_urls,
            duration: optional_u32(&params, "duration"),
            aspect_ratio: optional_string(&params, "aspect_ratio")
                .unwrap_or_else(|| "16:9".to_string()),
            resolution: optional_string(&params, "resolution")
                .unwrap_or_else(|| "720p".to_string()),
            negative_prompt: optional_string(&params, "negative_prompt"),
            audio: params.get("audio").and_then(|v| v.as_bool()),
            seed: optional_i64(&params, "seed"),
            last_frame_url: optional_string(&params, "last_frame_url"),
            reference_video_url: optional_string(&params, "reference_video_url"),
            reference_audio_url: optional_string(&params, "reference_audio_url"),
            generate_audio: params.get("generate_audio").and_then(|v| v.as_bool()),
        };

        self.backend.generate_video(request).await
    }

    fn schema(&self) -> ToolSchema {
        let mut props = IndexMap::new();
        props.insert(
            "prompt".into(),
            json!({
                "type": "string",
                "description": "Text prompt for text-to-video or image-to-video generation."
            }),
        );
        props.insert(
            "model".into(),
            json!({
                "type": "string",
                "description": "Provider model/family to use."
            }),
        );
        props.insert(
            "image_url".into(),
            json!({
                "type": "string",
                "description": "Optional starting image URL for image-to-video generation."
            }),
        );
        props.insert(
            "reference_image_urls".into(),
            json!({
                "type": "array",
                "items": {"type": "string"},
                "description": "Optional reference image URLs or local paths."
            }),
        );
        props.insert(
            "duration".into(),
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": 15,
                "description": "Requested duration in seconds."
            }),
        );
        props.insert(
            "aspect_ratio".into(),
            json!({
                "type": "string",
                "description": "Requested output aspect ratio.",
                "default": "16:9"
            }),
        );
        props.insert(
            "resolution".into(),
            json!({
                "type": "string",
                "description": "Requested output resolution.",
                "default": "720p"
            }),
        );
        props.insert(
            "negative_prompt".into(),
            json!({
                "type": "string",
                "description": "Optional negative prompt."
            }),
        );
        props.insert(
            "seed".into(),
            json!({
                "type": "integer",
                "description": "Optional random seed."
            }),
        );

        tool_schema(
            "video_generate",
            "Generate videos from text prompts or starting images.",
            JsonSchema::object(props, vec!["prompt".into()]),
        )
    }
}

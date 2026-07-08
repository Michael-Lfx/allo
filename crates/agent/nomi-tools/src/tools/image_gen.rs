//! Image generation tool types and handler.

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::sync::Arc;

use nomi_types::{JsonSchema, ToolError, ToolHandler, ToolSchema, tool_schema};

/// Parameters for image generation (text-to-image or image-to-image).
#[derive(Debug, Clone, Default)]
pub struct ImageGenRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub image_url: Option<String>,
    pub size: Option<String>,
    pub style: Option<String>,
    pub n: Option<u32>,
    pub extra: Option<Value>,
}

/// Backend for image generation operations.
#[async_trait]
pub trait ImageGenBackend: Send + Sync {
    async fn generate(&self, request: ImageGenRequest) -> Result<String, ToolError>;
}

/// Tool for generating images from text prompts.
pub struct ImageGenerateHandler {
    backend: Arc<dyn ImageGenBackend>,
}

impl ImageGenerateHandler {
    pub fn new(backend: Arc<dyn ImageGenBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl ToolHandler for ImageGenerateHandler {
    async fn execute(&self, params: Value) -> Result<String, ToolError> {
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("Missing 'prompt' parameter".into()))?;

        let request = ImageGenRequest {
            prompt: prompt.to_string(),
            model: params
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            image_url: params
                .get("image_url")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            size: params
                .get("size")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            style: params
                .get("style")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            n: params.get("n").and_then(|v| v.as_u64()).map(|n| n as u32),
            extra: params.get("extra").cloned().or_else(|| {
                params
                    .get("parameters")
                    .cloned()
                    .or_else(|| params.get("input").cloned())
            }),
        };

        self.backend.generate(request).await
    }

    fn schema(&self) -> ToolSchema {
        let mut props = IndexMap::new();
        props.insert(
            "prompt".into(),
            json!({
                "type": "string",
                "description": "Text description of the image to generate"
            }),
        );
        props.insert(
            "model".into(),
            json!({
                "type": "string",
                "description": "Model id (Flowy: AIPC-... or flowy/...; FAL: fal-ai/...)"
            }),
        );
        props.insert(
            "image_url".into(),
            json!({
                "type": "string",
                "description": "Optional reference image URL for image-to-image / edit"
            }),
        );
        props.insert("size".into(), json!({
            "type": "string",
            "description": "Image size: '256x256', '512x512', '1024x1024' (default: '1024x1024')",
            "enum": ["256x256", "512x512", "1024x1024"]
        }));
        props.insert(
            "style".into(),
            json!({
                "type": "string",
                "description": "Image style: 'natural' or 'vivid'"
            }),
        );
        props.insert(
            "n".into(),
            json!({
                "type": "integer",
                "description": "Number of images to generate (default: 1)",
                "default": 1
            }),
        );

        tool_schema(
            "image_generate",
            "Generate images from text descriptions using AI image generation models.",
            JsonSchema::object(props, vec!["prompt".into()]),
        )
    }
}

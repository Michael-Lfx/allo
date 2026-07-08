//! Unified media generation backend abstraction (Flowy today; ComfyUI-ready).

use async_trait::async_trait;

use nomi_types::{ToolError, ToolHandler};
use nomi_tools::tools::video::VideoGenerateRequest;

/// High-level media backend id for workflow routing and future plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaBackendId {
    Flowy,
    ComfyUi,
}

impl MediaBackendId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flowy => "flowy",
            Self::ComfyUi => "comfyui",
        }
    }
}

/// Image generation request for backend adapters.
#[derive(Debug, Clone)]
pub struct MediaImageRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub image_url: Option<String>,
}

/// Unified image + video generation surface for workflow steps.
#[async_trait]
pub trait MediaGenerationBackend: Send + Sync {
    fn id(&self) -> MediaBackendId;

    async fn generate_image(&self, request: MediaImageRequest) -> Result<String, ToolError>;

    async fn generate_video(&self, request: VideoGenerateRequest) -> Result<String, ToolError>;
}

/// Flowy adapter wrapping existing image/video backends.
pub struct FlowyMediaBackend {
    image: std::sync::Arc<dyn nomi_tools::ImageGenBackend>,
    video: std::sync::Arc<dyn nomi_tools::VideoGenerateBackend>,
}

impl FlowyMediaBackend {
    pub fn new(
        image: std::sync::Arc<dyn nomi_tools::ImageGenBackend>,
        video: std::sync::Arc<dyn nomi_tools::VideoGenerateBackend>,
    ) -> Self {
        Self { image, video }
    }
}

#[async_trait]
impl MediaGenerationBackend for FlowyMediaBackend {
    fn id(&self) -> MediaBackendId {
        MediaBackendId::Flowy
    }

    async fn generate_image(&self, request: MediaImageRequest) -> Result<String, ToolError> {
        use nomi_tools::ImageGenerateHandler;
        let handler = ImageGenerateHandler::new(self.image.clone());
        handler
            .execute(serde_json::json!({
                "prompt": request.prompt,
                "model": request.model,
                "image_url": request.image_url,
            }))
            .await
    }

    async fn generate_video(&self, request: VideoGenerateRequest) -> Result<String, ToolError> {
        use nomi_tools::VideoGenerateHandler;
        let handler = VideoGenerateHandler::new(self.video.clone());
        handler
            .execute(serde_json::json!({
                "prompt": request.prompt,
                "model": request.model,
                "image_url": request.image_url,
                "reference_image_urls": request.reference_image_urls,
                "duration": request.duration,
                "aspect_ratio": request.aspect_ratio,
                "resolution": request.resolution,
                "negative_prompt": request.negative_prompt,
                "seed": request.seed,
                "last_frame_url": request.last_frame_url,
                "reference_video_url": request.reference_video_url,
                "reference_audio_url": request.reference_audio_url,
                "generate_audio": request.generate_audio,
            }))
            .await
    }
}

/// Placeholder ComfyUI backend — use `skills/creative/comfyui` for local pipelines today.
pub struct ComfyUiMediaBackend;

#[async_trait]
impl MediaGenerationBackend for ComfyUiMediaBackend {
    fn id(&self) -> MediaBackendId {
        MediaBackendId::ComfyUi
    }

    async fn generate_image(&self, _request: MediaImageRequest) -> Result<String, ToolError> {
        Err(ToolError::ExecutionFailed(
            "ComfyUI media backend is not wired in hermes-media-workflows — use the comfyui skill or set media.provider=flowy".into(),
        ))
    }

    async fn generate_video(&self, _request: VideoGenerateRequest) -> Result<String, ToolError> {
        Err(ToolError::ExecutionFailed(
            "ComfyUI media backend is not wired in hermes-media-workflows — use the comfyui skill or set media.provider=flowy".into(),
        ))
    }
}

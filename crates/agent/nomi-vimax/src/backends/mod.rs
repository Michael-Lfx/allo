//! Flowy-only backends for chat / image / video.

mod chat;
mod image;
mod traits;
mod video;

pub use chat::FlowyChat;
pub use image::FlowyImage;
pub use traits::{VimaxChat, VimaxImage, VimaxVideo};
pub use video::FlowyVideo;

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::{GatewayConfig, MediaGenConfig, ServerConfig};
use nomifun_cloud::{FlowyApiClient, ServerSession};

/// Shared handle for Flowy-authenticated ViMax backends.
#[derive(Clone)]
pub struct FlowyVimaxServices {
    pub api: Arc<FlowyApiClient>,
    pub session: ServerSession,
    pub media: MediaGenConfig,
    pub server: ServerConfig,
    pub data_dir: PathBuf,
}

impl FlowyVimaxServices {
    pub fn try_new(config: &GatewayConfig, data_dir: &std::path::Path) -> Option<Self> {
        if !config.server.api_ready() {
            return None;
        }
        let api = FlowyApiClient::new(&config.server).ok()?;
        let session = ServerSession::from_config(&config.server, data_dir);
        Some(Self {
            api: Arc::new(api),
            session,
            media: config.media.clone(),
            server: config.server.clone(),
            data_dir: data_dir.to_path_buf(),
        })
    }

    pub async fn require_token(&self) -> Result<(), crate::error::VimaxError> {
        let tok = self
            .session
            .access_token()
            .await
            .map_err(|e| crate::error::VimaxError::msg(e.to_string()))?
            .filter(|t| !t.trim().is_empty());
        if tok.is_none() {
            return Err(crate::error::VimaxError::NotAuthenticated);
        }
        Ok(())
    }

    pub fn chat(&self) -> FlowyChat {
        FlowyChat::new(self.clone(), None)
    }

    pub fn chat_with_model(&self, model: Option<String>) -> FlowyChat {
        FlowyChat::new(self.clone(), model)
    }

    pub fn image(&self) -> FlowyImage {
        FlowyImage::new(self.clone(), None)
    }

    pub fn image_with_model(&self, model: Option<String>) -> FlowyImage {
        FlowyImage::new(self.clone(), model)
    }

    pub fn video(&self) -> FlowyVideo {
        FlowyVideo::new(self.clone(), None, None)
    }

    pub fn video_with_model(&self, model: Option<String>) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, None)
    }

    pub fn video_with_model_and_cancel(
        &self,
        model: Option<String>,
        cancel: Option<tokio_util::sync::CancellationToken>,
    ) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, cancel)
    }
}

pub(crate) fn map_server_err(err: nomifun_cloud::ServerClientError) -> crate::error::VimaxError {
    crate::error::VimaxError::msg(err.to_string())
}

/// Classify a Flowy upstream error for chat / image / video calls.
pub(crate) fn map_model_err(
    kind: &str,
    model: Option<&str>,
    stage_hint: &str,
    err: nomifun_cloud::ServerClientError,
) -> crate::error::VimaxError {
    let raw = err.to_string();
    let model_label = model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(default)");
    let lower = raw.to_ascii_lowercase();
    let hint = if lower.contains("datainspectionfailed")
        || lower.contains("inappropriate content")
        || lower.contains("不当内容")
        || lower.contains("内容安全")
        || lower.contains("敏感内容")
    {
        "Upstream content safety rejected the prompt/result. The client auto-retries with safer prompts; if it still fails, soften violent/sensitive shot wording and resume."
    } else if lower.contains("not valid flowy json envelope")
        || lower.contains("expected value at line 1 column 1")
        || lower.contains("<empty body>")
    {
        "The Flowy video API returned an empty or non-JSON body (often oversized data-URL images, gateway timeout, or channel fault). Retry with first/last frame only, or switch video model and resume."
    } else if lower.contains("all channel models failed") || lower.contains("所有渠道模型均失败")
    {
        if lower.contains("datainspection") || lower.contains("inappropriate") {
            "Channel failure was caused by content safety. Soften shot wording and resume."
        } else {
            "Upstream reports all channels for this model are unavailable (safety, quota, breaker, or outage). Check upstream detail or switch model."
        }
    } else if lower.contains("empty content") {
        if lower.contains("system_len=0") || lower.contains("user_len=0") {
            "Request system/user prompt was empty — not a model outage. Check whether the scene script was generated."
        } else if lower.contains("finish_reason=length") {
            "Model output was truncated (reasoning used the token budget). Switch to a non-reasoning model, or resume later."
        } else {
            "Upstream returned empty content (common with reasoning models). Switch model or resume."
        }
    } else if lower.contains("refusing llm call with empty prompt")
        || lower.contains("refusing multimodal llm call")
    {
        "Request prompt was empty — check prior artifacts (e.g. script.txt)."
    } else if lower.contains("privacyinformation")
        || lower.contains("inputimagesensitivecontent")
        || lower.contains("may contain real person")
    {
        "Input frame/reference was flagged as a real-person likeness. A stylized redraw retry is available; if it still fails, use a more illustrated style and resume."
    } else if lower.contains("401") || lower.contains("unauthorized") {
        "Auth failed — confirm you are signed in to Flowy cloud."
    } else if lower.contains("429") || lower.contains("rate limit") {
        "Rate limited — retry shortly."
    } else {
        "Check that the selected model is available, or resume from checkpoint later."
    };
    let kind_label = match kind {
        "image" => "Image generation",
        "video" => "Video generation",
        _ => "Chat model (LLM)",
    };
    let msg = format!(
        "{kind_label} failed\nModel: {model_label}\nStage: {stage_hint}\nCause: {raw}\nHint: {hint}"
    );
    match kind {
        "image" => crate::error::VimaxError::Image(msg),
        "video" => crate::error::VimaxError::Video(msg),
        _ => crate::error::VimaxError::Llm(msg),
    }
}

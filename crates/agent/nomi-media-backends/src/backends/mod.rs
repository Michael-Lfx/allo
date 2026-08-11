//! Flowy-only backends for chat / image / video.

mod chat;
mod image;
mod traits;
mod video;

pub use chat::FlowyChat;
pub use image::FlowyImage;
pub use traits::{MediaChat, MediaImage, MediaVideo};
pub use video::FlowyVideo;

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::{GatewayConfig, MediaGenConfig, ServerConfig};
use nomifun_cloud::{FlowyApiClient, ServerSession};

/// Shared handle for Flowy-authenticated media backends.
#[derive(Clone)]
pub struct FlowyMediaServices {
    pub api: Arc<FlowyApiClient>,
    pub session: ServerSession,
    pub media: MediaGenConfig,
    pub server: ServerConfig,
    pub data_dir: PathBuf,
}

impl FlowyMediaServices {
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

    pub async fn require_token(&self) -> Result<(), crate::error::MediaBackendError> {
        let tok = self
            .session
            .access_token()
            .await
            .map_err(|e| crate::error::MediaBackendError::msg(e.to_string()))?
            .filter(|t| !t.trim().is_empty());
        if tok.is_none() {
            return Err(crate::error::MediaBackendError::NotAuthenticated);
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
        FlowyImage::new(self.clone(), None, None)
    }

    pub fn image_with_model(&self, model: Option<String>) -> FlowyImage {
        FlowyImage::new(self.clone(), model, None)
    }

    pub fn image_with_model_and_aspect(
        &self,
        model: Option<String>,
        aspect_ratio: Option<String>,
    ) -> FlowyImage {
        FlowyImage::new(self.clone(), model, aspect_ratio)
    }

    pub fn video(&self) -> FlowyVideo {
        FlowyVideo::new(self.clone(), None, None, None, None, None)
    }

    pub fn video_with_model(&self, model: Option<String>) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, None, None, None, None)
    }

    pub fn video_with_model_and_cancel(
        &self,
        model: Option<String>,
        cancel: Option<tokio_util::sync::CancellationToken>,
    ) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, cancel, None, None, None)
    }

    pub fn video_with_model_cancel_and_aspect(
        &self,
        model: Option<String>,
        cancel: Option<tokio_util::sync::CancellationToken>,
        aspect_ratio: Option<String>,
    ) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, cancel, aspect_ratio, None, None)
    }

    /// Video backend with a pipeline progress hook (create / poll / download stages).
    pub fn video_with_model_cancel_and_aspect_and_progress(
        &self,
        model: Option<String>,
        cancel: Option<tokio_util::sync::CancellationToken>,
        aspect_ratio: Option<String>,
        progress: Option<crate::progress::ProgressCallback>,
    ) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, cancel, aspect_ratio, None, progress)
    }

    pub fn video_with_session_quality(
        &self,
        model: Option<String>,
        cancel: Option<tokio_util::sync::CancellationToken>,
        aspect_ratio: Option<String>,
        resolution: Option<String>,
        progress: Option<crate::progress::ProgressCallback>,
    ) -> FlowyVideo {
        FlowyVideo::new(
            self.clone(),
            model,
            cancel,
            aspect_ratio,
            resolution,
            progress,
        )
    }

    /// Upload a local image via OSS presign PUT and return the HTTPS `publicUrl`.
    ///
    /// `role` is a fallback stem; the local file stem is preferred so multi-ref
    /// models can bind prompts to meaningful names (e.g. `Alice_three_view.jpg`).
    ///
    /// Identical files (same size + mtime) reuse the process-wide cached `publicUrl`,
    /// so cross-shot re-uploads of unchanged cast/env/prop frames cost zero network.
    pub async fn upload_image_public_url(
        &self,
        path: &std::path::Path,
        role: &str,
    ) -> Result<String, crate::error::MediaBackendError> {
        use tracing::debug;

        let meta = tokio::fs::metadata(path).await.ok();
        let size = meta.as_ref().map(|m| m.len());
        let modified = meta.and_then(|m| m.modified().ok());
        if let Some(size) = size
            && let Some(url) = oss_url_cache_get(path, size, modified)
        {
            debug!(path = %path.display(), "OSS publicUrl cache hit");
            return Ok(url);
        }

        let bytes = tokio::fs::read(path).await?;
        let (bytes, mime) =
            tokio::task::spawn_blocking(move || prepare_media_image_upload(bytes))
                .await
                .map_err(|e| {
                    crate::error::MediaBackendError::msg(format!("image encode join: {e}"))
                })??;

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(role);
        let safe_stem: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe_stem = {
            let t = safe_stem.trim_matches('_');
            if t.is_empty() {
                role.to_string()
            } else {
                t.chars().take(64).collect()
            }
        };

        let file_name = match mime {
            "image/jpeg" => format!("{safe_stem}.jpg"),
            "image/webp" => format!("{safe_stem}.webp"),
            _ => format!("{safe_stem}.png"),
        };

        debug!(
            role = %safe_stem,
            path = %path.display(),
            bytes = bytes.len(),
            mime,
            "uploading media image to OSS"
        );

        let url = self
            .api
            .upload_bytes_via_oss(&self.session, &bytes, &file_name, mime)
            .await
            .map_err(|e| {
                crate::error::MediaBackendError::msg(format!(
                    "OSS upload failed ({safe_stem}): {e}"
                ))
            })?;
        if let Some(size) = size {
            oss_url_cache_put(path, size, modified, &url);
        }
        Ok(url)
    }
}

/// Cache key: canonical path + file size + mtime (cheap content fingerprint).
/// `publicUrl` is the CDN download URL (long-lived) — safe to reuse within a process.
type OssUrlCache = std::collections::HashMap<PathBuf, (u64, Option<std::time::SystemTime>, String)>;

const OSS_URL_CACHE_MAX: usize = 2048;

fn oss_url_cache() -> &'static std::sync::Mutex<OssUrlCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<OssUrlCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(OssUrlCache::new()))
}

fn oss_url_cache_get(
    path: &std::path::Path,
    size: u64,
    modified: Option<std::time::SystemTime>,
) -> Option<String> {
    let guard = oss_url_cache().lock().unwrap_or_else(|e| e.into_inner());
    match guard.get(path) {
        Some((s, m, url)) if *s == size && *m == modified => Some(url.clone()),
        _ => None,
    }
}

fn oss_url_cache_put(
    path: &std::path::Path,
    size: u64,
    modified: Option<std::time::SystemTime>,
    url: &str,
) {
    let mut guard = oss_url_cache().lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() >= OSS_URL_CACHE_MAX {
        guard.clear();
    }
    guard.insert(
        path.to_path_buf(),
        (size, modified, url.to_string()),
    );
}

/// Cap oversized images before OSS upload (shared by image + video backends).
pub(crate) fn prepare_media_image_upload(
    bytes: Vec<u8>,
) -> Result<(Vec<u8>, &'static str), crate::error::MediaBackendError> {
    const MAX_BYTES: usize = 1_200_000;
    let kind = crate::media_local::image_magic_kind(&bytes);
    if bytes.len() <= MAX_BYTES {
        let mime = match kind {
            Some("jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("png") => "image/png",
            _ => {
                return Err(crate::error::MediaBackendError::msg(
                    "image is not a decodable PNG/JPEG/WEBP".to_string(),
                ));
            }
        };
        return Ok((bytes, mime));
    }

    let img = ::image::load_from_memory(&bytes)
        .map_err(|e| crate::error::MediaBackendError::msg(format!("decode image: {e}")))?;
    let img = img.resize(1280, 720, ::image::imageops::FilterType::Triangle);
    let mut out = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut out);
    img.write_to(&mut cursor, ::image::ImageFormat::Jpeg)
        .map_err(|e| crate::error::MediaBackendError::msg(format!("re-encode image jpeg: {e}")))?;
    if out.is_empty() {
        return Err(crate::error::MediaBackendError::msg(
            "re-encoded image is empty".to_string(),
        ));
    }
    Ok((out, "image/jpeg"))
}

pub(crate) fn map_server_err(err: nomifun_cloud::ServerClientError) -> crate::error::MediaBackendError {
    crate::error::MediaBackendError::msg(err.to_string())
}

/// Classify a Flowy upstream error for chat / image / video calls.
pub(crate) fn map_model_err(
    kind: &str,
    model: Option<&str>,
    stage_hint: &str,
    err: nomifun_cloud::ServerClientError,
) -> crate::error::MediaBackendError {
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
    } else if lower.contains("cannot be mixed")
        || lower.contains("last frame image content cannot be mixed")
        || (lower.contains("reference_image") && lower.contains("first_frame"))
        || (lower.contains("last_frame") && lower.contains("reference"))
    {
        "Seedance rejects mixing first/last_frame with reference_image. Use frame I2V only (client now omits refs when frames are present)."
    } else if lower.contains("captions are not enough")
        || (lower.contains("caption") && lower.contains("empty"))
    {
        "Seedance 2.0 rejected empty/weak audio captions. The client reinforces ambient/BGM captions then retries without audio only as a last resort; ensure shot audio_desc has dialogue or SFX+BGM."
    } else if lower.contains("publicurl")
        || lower.contains("presign_not_configured")
        || lower.contains("oss put failed")
        || (lower.contains("oss") && lower.contains("presign"))
    {
        "Frame OSS upload failed. Confirm you are signed in with JWT (not API Key only), and that the server has OSS + CDN (publicUrl) configured."
    } else if lower.contains("not valid flowy json envelope")
        || lower.contains("expected value at line 1 column 1")
        || lower.contains("<empty body>")
    {
        "The Flowy video API returned an empty or non-JSON body (gateway timeout or channel fault). Frames are uploaded via OSS URLs; retry or switch video model and resume."
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
    } else if lower.contains("402")
        || lower.contains("insufficient_credit")
        || lower.contains("insufficient credit")
        || lower.contains("insufficient credits")
        || lower.contains("credit balance is too low")
        || raw.contains("积分不足")
        || raw.contains("余额不足")
    {
        // Stable marker for UI i18n (ProgressTimeline / toast). Keep the
        // English hint readable in error details for non-localized logs.
        "INSUFFICIENT_CREDITS — Flowy credits are too low. Top up or shorten duration, then resume from checkpoint (finished clips are not re-billed)."
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
        "image" => crate::error::MediaBackendError::Image(msg),
        "video" => crate::error::MediaBackendError::Video(msg),
        _ => crate::error::MediaBackendError::Llm(msg),
    }
}

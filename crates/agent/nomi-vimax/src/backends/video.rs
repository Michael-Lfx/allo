//! Flowy video generation → local file (strictly serial + cancelable).

use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::info;

use nomifun_cloud::{
    MODEL_CATEGORY_VIDEO, VideoContentImage, VideoCreateParams, resolve_model_in_catalog,
};

use super::{FlowyVimaxServices, VimaxVideo, map_model_err, map_server_err};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{is_usable_video_file, write_video_bytes_atomic};

/// Cap concurrent Flowy video create+poll calls process-wide to **one**.
const GLOBAL_VIDEO_CONCURRENCY: usize = 1;

fn global_video_gate() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(GLOBAL_VIDEO_CONCURRENCY))
}

pub struct FlowyVideo {
    services: FlowyVimaxServices,
    model_override: Option<String>,
    cancel: Option<CancellationToken>,
}

impl FlowyVideo {
    pub fn new(
        services: FlowyVimaxServices,
        model_override: Option<String>,
        cancel: Option<CancellationToken>,
    ) -> Self {
        Self {
            services,
            model_override: model_override.and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            }),
            cancel,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|t| t.is_cancelled())
    }

    async fn resolve_model(&self) -> VimaxResult<String> {
        self.services.require_token().await?;
        let configured = self
            .model_override
            .as_deref()
            .unwrap_or_else(|| self.services.media.video.model.trim());
        let catalog = self
            .services
            .api
            .get_available_models_claw(&self.services.session, Some(MODEL_CATEGORY_VIDEO))
            .await
            .map_err(map_server_err)?;
        if !configured.is_empty() {
            if let Some(id) = resolve_model_in_catalog(configured, &catalog.cloud) {
                return Ok(id);
            }
            if self.model_override.is_some() {
                return Ok(configured.to_string());
            }
        }
        catalog
            .cloud
            .first()
            .map(|m| m.id.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| VimaxError::Video("no Flowy video model in catalog".into()))
    }

    /// Read a local frame, optionally shrink it, upload via OSS presign PUT, return `publicUrl`.
    async fn frame_public_url(&self, path: &Path, role: &str) -> VimaxResult<String> {
        self.services.upload_image_public_url(path, role).await
    }
}

#[async_trait]
impl VimaxVideo for FlowyVideo {
    async fn generate(
        &self,
        prompt: &str,
        first_frame: Option<&Path>,
        last_frame: Option<&Path>,
        ref_images: &[&Path],
        duration_secs: u32,
        out_path: &Path,
    ) -> VimaxResult<()> {
        if self.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        // Resume: never re-bill for a clip already on disk.
        if is_usable_video_file(out_path) {
            return Ok(());
        }

        self.services.require_token().await?;
        let model = self.resolve_model().await?;
        let model_for_err = model.clone();

        // Upload frames to OSS first so the create-task JSON only carries short HTTPS URLs
        // (avoids base64-bloated bodies that break Flowy / Seedance).
        let mut images = Vec::new();
        let mut local_frame_notes = Vec::new();
        if let Some(path) = first_frame {
            local_frame_notes.push(format!(
                "first_frame←{}",
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
            ));
            images.push(VideoContentImage {
                url: self.frame_public_url(path, "first_frame").await?,
                role: "first_frame".into(),
            });
        }
        if let Some(path) = last_frame {
            local_frame_notes.push(format!(
                "last_frame←{}",
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
            ));
            images.push(VideoContentImage {
                url: self.frame_public_url(path, "last_frame").await?,
                role: "last_frame".into(),
            });
        }
        // Seedance forbids mixing first/last_frame with reference_image (or draft_task).
        let uses_frame_roles = images.iter().any(|img| {
            matches!(img.role.as_str(), "first_frame" | "last_frame")
        });
        if uses_frame_roles {
            if !ref_images.is_empty() {
                tracing::info!(
                    omitted = ref_images.len(),
                    "omitting reference_image(s): Seedance forbids mixing with first/last_frame"
                );
                local_frame_notes.push(format!(
                    "reference_image_omitted×{}",
                    ref_images.len()
                ));
            }
        } else {
            for (i, path) in ref_images.iter().enumerate() {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("reference_image_{i}"));
                local_frame_notes.push(format!("reference_image←{stem}"));
                images.push(VideoContentImage {
                    url: self.frame_public_url(path, &stem).await?,
                    role: "reference_image".into(),
                });
            }
        }

        let aspect = self.services.media.video.default_aspect_ratio.clone();
        let resolution = Some(self.services.media.video.default_resolution.clone());
        // Seedance 2.0 / 2.0-fast I2V rejects duration outside [4, 15]; we use ≥5s clips.
        let max_d = self.services.media.video.default_duration.clamp(5, 15);
        let duration = duration_secs.clamp(5, max_d);

        let params = VideoCreateParams {
            model: model.clone(),
            prompt: prompt.to_string(),
            duration: Some(duration),
            aspect_ratio: aspect,
            resolution,
            negative_prompt: None,
            seed: None,
            watermark: false,
            // Seedance 2.0 requires non-empty audio captions in the prompt when true.
            generate_audio: Some(true),
            images,
            reference_video_url: None,
            reference_audio_url: None,
        };

        log_video_create_params(&params, &local_frame_notes, out_path);

        let timeout = self.services.media.video.poll_timeout_seconds.max(600);

        // Strictly one in-flight video API call process-wide.
        let _permit = global_video_gate()
            .acquire()
            .await
            .map_err(|_| VimaxError::Video("video rate-limit gate closed".into()))?;

        if self.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        if is_usable_video_file(out_path) {
            return Ok(());
        }

        let cancel = self.cancel.clone();
        let should_cancel: Option<Arc<dyn Fn() -> bool + Send + Sync>> =
            cancel.map(|t| {
                Arc::new(move || t.is_cancelled()) as Arc<dyn Fn() -> bool + Send + Sync>
            });

        let first = self
            .services
            .api
            .generate_video_with_timeout_and_progress_cancellable(
                &self.services.session,
                params.to_json(),
                timeout,
                None,
                should_cancel.clone(),
                None,
            )
            .await;

        let record = match first {
            Ok(r) => r,
            Err(e) if is_seedance_caption_empty_err(&e) => {
                // Seedance 2.0 audio pipeline rejected empty/weak captions — retry silent.
                tracing::warn!(
                    model = %model_for_err,
                    error = %e,
                    "Seedance rejected audio captions; retrying with generate_audio=false"
                );
                let mut silent = params;
                silent.generate_audio = Some(false);
                log_video_create_params(&silent, &local_frame_notes, out_path);
                if self.is_cancelled() {
                    return Err(VimaxError::Cancelled);
                }
                if is_usable_video_file(out_path) {
                    return Ok(());
                }
                self.services
                    .api
                    .generate_video_with_timeout_and_progress_cancellable(
                        &self.services.session,
                        silent.to_json(),
                        timeout,
                        None,
                        should_cancel,
                        None,
                    )
                    .await
                    .map_err(|e2| {
                        if self.is_cancelled() {
                            return VimaxError::Cancelled;
                        }
                        map_model_err(
                            "video",
                            Some(model_for_err.as_str()),
                            "video_generate_silent_retry",
                            e2,
                        )
                    })?
            }
            Err(e) => {
                if self.is_cancelled() {
                    return Err(VimaxError::Cancelled);
                }
                return Err(map_model_err(
                    "video",
                    Some(model_for_err.as_str()),
                    "video_generate",
                    e,
                ));
            }
        };

        if self.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }

        let url = record
            .video_url()
            .ok_or_else(|| VimaxError::Video("video task succeeded but no video_url".into()))?;

        download_video(&url, out_path).await
    }
}

/// Seedance 2.0 audio path requires usable dialogue/SFX captions in the prompt.
fn is_seedance_caption_empty_err(err: &nomifun_cloud::ServerClientError) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("captions are not enough")
        || s.contains("captions are not enough or empty")
        || (s.contains("caption") && s.contains("empty"))
        || (s.contains("caption") && s.contains("not enough"))
}

fn log_video_create_params(
    params: &VideoCreateParams,
    local_frame_notes: &[String],
    out_path: &Path,
) {
    let prompt_preview: String = params.prompt.chars().take(200).collect();
    let image_summaries: Vec<String> = params
        .images
        .iter()
        .map(|img| format!("{}={}", img.role, redact_media_url_for_log(&img.url)))
        .collect();
    info!(
        model = %params.model,
        duration = ?params.duration,
        aspect_ratio = %params.aspect_ratio,
        resolution = ?params.resolution,
        watermark = params.watermark,
        generate_audio = ?params.generate_audio,
        prompt_chars = params.prompt.chars().count(),
        prompt_preview = %prompt_preview,
        image_count = params.images.len(),
        images = ?image_summaries,
        local_frames = ?local_frame_notes,
        out = %out_path.display(),
        "video_generate API params"
    );
}

/// Log-safe URL: keep host + path, drop query (presign secrets) and truncate long paths.
fn redact_media_url_for_log(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("data:") {
        let mime = trimmed
            .split(';')
            .next()
            .unwrap_or("data:")
            .trim_start_matches("data:");
        return format!("data:{mime};base64,<omitted len={}>", trimmed.len());
    }
    let no_query = trimmed.split('?').next().unwrap_or(trimmed);
    if no_query.chars().count() <= 160 {
        no_query.to_string()
    } else {
        format!("{}…", no_query.chars().take(160).collect::<String>())
    }
}

async fn download_video(url: &str, out_path: &Path) -> VimaxResult<()> {
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| VimaxError::Video(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(VimaxError::Video(format!(
            "download failed: HTTP {}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| VimaxError::Video(e.to_string()))?;
    write_video_bytes_atomic(out_path, &bytes).await?;
    Ok(())
}

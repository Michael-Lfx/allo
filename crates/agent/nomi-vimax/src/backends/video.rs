//! Flowy video generation → local file (strictly serial + cancelable).

use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

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

        let mut images = Vec::new();
        if let Some(path) = first_frame {
            images.push(VideoContentImage {
                url: path_as_upload_url(path).await?,
                role: "first_frame".into(),
            });
        }
        if let Some(path) = last_frame {
            images.push(VideoContentImage {
                url: path_as_upload_url(path).await?,
                role: "last_frame".into(),
            });
        }
        for path in ref_images {
            images.push(VideoContentImage {
                url: path_as_upload_url(path).await?,
                role: "reference_image".into(),
            });
        }

        let aspect = self.services.media.video.default_aspect_ratio.clone();
        let resolution = Some(self.services.media.video.default_resolution.clone());
        // Seedance 2.0 / 2.0-fast I2V rejects duration outside [4, 15]; we use ≥5s clips.
        let max_d = self.services.media.video.default_duration.clamp(5, 15);
        let duration = duration_secs.clamp(5, max_d);

        let params = VideoCreateParams {
            model,
            prompt: prompt.to_string(),
            duration: Some(duration),
            aspect_ratio: aspect,
            resolution,
            negative_prompt: None,
            seed: None,
            watermark: false,
            generate_audio: Some(true),
            images,
            reference_video_url: None,
            reference_audio_url: None,
        };

        let timeout = self.services.media.video.poll_timeout_seconds.max(600);
        let body = params.to_json();

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

        let record = self
            .services
            .api
            .generate_video_with_timeout_and_progress_cancellable(
                &self.services.session,
                body,
                timeout,
                None,
                should_cancel,
                None,
            )
            .await
            .map_err(|e| {
                if self.is_cancelled() {
                    return VimaxError::Cancelled;
                }
                map_model_err("video", Some(model_for_err.as_str()), "video_generate", e)
            })?;

        if self.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }

        let url = record
            .video_url()
            .ok_or_else(|| VimaxError::Video("video task succeeded but no video_url".into()))?;

        download_video(&url, out_path).await
    }
}

async fn path_as_upload_url(path: &Path) -> VimaxResult<String> {
    let bytes = tokio::fs::read(path).await?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    Ok(format!("data:image/png;base64,{b64}"))
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

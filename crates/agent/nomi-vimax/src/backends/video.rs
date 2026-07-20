//! Flowy video generation → local file (atomic save + global rate-limit gate).

use async_trait::async_trait;
use std::path::Path;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

use nomifun_cloud::{
    MODEL_CATEGORY_VIDEO, VideoContentImage, VideoCreateParams, resolve_model_in_catalog,
};

use super::{FlowyVimaxServices, VimaxVideo, map_model_err, map_server_err};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{is_usable_video_file, write_video_bytes_atomic};

/// Cap concurrent Flowy video create+poll calls process-wide.
/// Vendor gateways (502) trip easily when many shots/scenes fire together.
const GLOBAL_VIDEO_CONCURRENCY: usize = 1;

fn global_video_gate() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(GLOBAL_VIDEO_CONCURRENCY))
}

pub struct FlowyVideo {
    services: FlowyVimaxServices,
    model_override: Option<String>,
}

impl FlowyVideo {
    pub fn new(services: FlowyVimaxServices, model_override: Option<String>) -> Self {
        Self {
            services,
            model_override: model_override.and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            }),
        }
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
        let duration = duration_secs
            .max(1)
            .min(self.services.media.video.default_duration.max(10));

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

        // Serialize vendor calls so parallel shot/scene jobs do not stampede into 502.
        let _permit = global_video_gate()
            .acquire()
            .await
            .map_err(|_| VimaxError::Video("video rate-limit gate closed".into()))?;

        // Re-check after waiting in queue — another task may have finished this path.
        if is_usable_video_file(out_path) {
            return Ok(());
        }

        let record = self
            .services
            .api
            .generate_video_with_timeout(&self.services.session, body, timeout)
            .await
            .map_err(|e| {
                map_model_err("video", Some(model_for_err.as_str()), "video_generate", e)
            })?;

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

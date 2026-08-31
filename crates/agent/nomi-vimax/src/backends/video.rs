//! Flowy video generation → local file (strictly serial + cancelable).

use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::info;

use nomifun_cloud::{
    video_task_failure_message, is_minimax_h3_model, MODEL_CATEGORY_VIDEO, VideoContentImage,
    VideoCreateParams, resolve_model_in_catalog, VideoTaskRecord,
};

use super::{FlowyVimaxServices, VimaxVideo, map_model_err, map_server_err};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{
    is_usable_video_file, scrub_unusable_video, write_image_bytes_atomic, write_video_bytes_atomic,
};

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
    aspect_ratio: Option<String>,
    resolution: Option<String>,
    /// Optional pipeline progress hook — emits fine-grained create / poll / download stages.
    progress: Option<crate::progress::ProgressCallback>,
}

impl FlowyVideo {
    pub fn new(
        services: FlowyVimaxServices,
        model_override: Option<String>,
        cancel: Option<CancellationToken>,
        aspect_ratio: Option<String>,
        resolution: Option<String>,
        progress: Option<crate::progress::ProgressCallback>,
    ) -> Self {
        Self {
            services,
            model_override: model_override.and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            }),
            cancel,
            aspect_ratio: aspect_ratio.and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(crate::aspect::normalize_aspect_ratio(&t))
                }
            }),
            resolution: resolution.and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            }),
            progress,
        }
    }

    fn resolved_aspect(&self) -> String {
        self.aspect_ratio
            .clone()
            .unwrap_or_else(|| {
                crate::aspect::normalize_aspect_ratio(
                    &self.services.media.video.default_aspect_ratio,
                )
            })
    }

    fn resolved_resolution(&self, model: &str) -> String {
        let raw = self
            .resolution
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                self.services
                    .media
                    .video
                    .default_resolution
                    .trim()
                    .to_string()
            });
        crate::video_quality::normalize_resolution_for_model(model, &raw)
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

    /// Emit a user-visible progress event (no-op without a wired callback).
    fn emit_progress(&self, stage: &str, message: &str, metadata: Option<serde_json::Value>) {
        if let Some(cb) = &self.progress {
            cb(stage, message, metadata);
        }
    }

    /// Seedance poll progress hook — reports status + elapsed seconds so the UI
    /// can render a live "cloud rendering · waited Ns" line (create → poll → download).
    fn poll_progress_hook(&self) -> Option<Box<dyn FnMut(&VideoTaskRecord, u64) + Send>> {
        let cb = self.progress.clone()?;
        Some(Box::new(move |record: &VideoTaskRecord, elapsed: u64| {
            let mut meta = serde_json::json!({
                "elapsed_secs": elapsed,
                "status": record.status,
                "task_id": record.id,
                "credits_consumed": record.credits_consumed,
            });
            if let Some(obj) = meta.as_object_mut() {
                if let Some(tid) = record.task_id.as_ref() {
                    obj.insert("upstream_task_id".into(), serde_json::json!(tid));
                }
            }
            cb("video_poll", &format!("elapsed {elapsed}s"), Some(meta));
        }))
    }

    fn emit_video_credits(&self, record: &VideoTaskRecord) {
        if record.credits_consumed <= 0 {
            return;
        }
        self.emit_progress(
            "video_credits",
            &format!("credits {}", record.credits_consumed),
            Some(serde_json::json!({
                "task_id": record.id,
                "credits_consumed": record.credits_consumed,
            })),
        );
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
        last_frame_out: Option<&Path>,
        ref_video: Option<&Path>,
        ref_audio: Option<&Path>,
    ) -> VimaxResult<()> {
        if self.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        // Resume: never re-bill for a clip already on disk (scrub drops corrupt first).
        scrub_unusable_video(out_path).await?;
        if is_usable_video_file(out_path) {
            return Ok(());
        }

        self.services.require_token().await?;
        let model = self.resolve_model().await?;
        let model_for_err = model.clone();
        let is_h3 = is_minimax_h3_model(&model);
        if ref_video.is_some() && !is_h3 {
            return Err(VimaxError::InvalidParams(
                "action imitation (reference_video) requires MiniMax-H3".into(),
            ));
        }

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
            // Upload reference images concurrently (independent I/O) while preserving
            // their order — the prompt binds each `Image N` by array position.
            let n = ref_images.len();
            let mut set = tokio::task::JoinSet::new();
            for (i, path) in ref_images.iter().enumerate() {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("reference_image_{i}"));
                let services = self.services.clone();
                let p = (*path).to_path_buf();
                let stem_clone = stem.clone();
                set.spawn(async move {
                    let url = services.upload_image_public_url(&p, &stem_clone).await;
                    (i, stem, url)
                });
            }
            let mut slots: Vec<Option<(String, VimaxResult<String>)>> = (0..n)
                .map(|_| None)
                .collect();
            while let Some(joined) = set.join_next().await {
                let (i, stem, url) =
                    joined.map_err(|e| VimaxError::Video(format!("ref upload join: {e}")))?;
                slots[i] = Some((stem, url));
            }
            for slot in slots {
                let (stem, url) =
                    slot.expect("every spawned ref upload joins exactly once");
                local_frame_notes.push(format!("reference_image←{stem}"));
                images.push(VideoContentImage {
                    url: url?,
                    role: "reference_image".into(),
                });
            }
        }

        let mut reference_video_url = None;
        if uses_frame_roles {
            if ref_video.is_some() {
                tracing::info!(
                    "omitting reference_video: cannot mix with first/last_frame"
                );
                local_frame_notes.push("reference_video_omitted".into());
            }
        } else if let Some(path) = ref_video {
            local_frame_notes.push(format!(
                "reference_video←{}",
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
            ));
            reference_video_url = Some(
                self.services
                    .upload_video_public_url(path, "reference_video")
                    .await?,
            );
        }

        let mut reference_audio_url = None;
        // Seedance: reference_audio cannot be the only reference input — need ≥1
        // image or reference_video. Pure T2V must omit voice refs.
        let has_visual_ref = !images.is_empty() || reference_video_url.is_some();
        if let Some(path) = ref_audio.filter(|p| crate::media_local::is_usable_audio_file(p)) {
            if has_visual_ref {
                local_frame_notes.push(format!(
                    "reference_audio←{}",
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                ));
                reference_audio_url = Some(
                    self.services
                        .upload_audio_public_url(path, "reference_audio")
                        .await?,
                );
            } else {
                tracing::info!(
                    path = %path.display(),
                    "omitting reference_audio: Seedance forbids audio as the only reference input"
                );
                local_frame_notes.push("reference_audio_omitted_no_visual_ref".into());
            }
        }

        let aspect = self.resolved_aspect();
        let resolution = Some(self.resolved_resolution(&model));
        // Last line of defence: planning already sizes clips to the model window,
        // but a resumed session or a hand-edited storyboard can still ask for a
        // duration the upstream API would reject.
        let bounds = crate::video_quality::clip_bounds_for_model(&model);
        let duration = bounds.clamp_secs(duration_secs);
        if duration != duration_secs {
            tracing::warn!(
                requested = duration_secs,
                clamped = duration,
                model = %model,
                "video duration clamped to [{}, {}]",
                bounds.min_secs(),
                bounds.max_secs(),
            );
        }
        let want_last_frame = last_frame_out.is_some();

        let params = VideoCreateParams {
            model: model.clone(),
            prompt: prompt.to_string(),
            duration: Some(duration),
            aspect_ratio: aspect,
            resolution,
            negative_prompt: None,
            seed: None,
            watermark: false,
            // Seedance 2.0 requires non-empty audio captions when true.
            // MiniMax-H3 does not use generate_audio / return_last_frame.
            generate_audio: if is_h3 { None } else { Some(true) },
            return_last_frame: if is_h3 { None } else { Some(want_last_frame) },
            images,
            reference_video_url,
            reference_audio_url,
        };

        log_video_create_params(&params, &local_frame_notes, out_path);

        let timeout = self.services.media.video.poll_timeout_seconds.max(1200);

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

        let first = {
            self.emit_progress("video_create", "submitting video task", None);
            self.services
                .api
                .generate_video_with_timeout_and_progress_cancellable(
                    &self.services.session,
                    params.to_json(),
                    timeout,
                    self.poll_progress_hook(),
                    should_cancel.clone(),
                    None,
                )
                .await
        };

        let record = match first {
            Ok(r) => r,
            Err(e)
                if !is_h3
                    && is_seedance_audio_only_ref_err(&e)
                    && params.reference_audio_url.is_some() =>
            {
                // Upstream sometimes strips rejected images then complains that audio
                // is the only remaining reference — drop voice ref and retry once.
                tracing::warn!(
                    model = %model_for_err,
                    error = %e,
                    "Seedance rejected audio-only reference; retrying without reference_audio"
                );
                let mut no_audio = params.clone();
                no_audio.reference_audio_url = None;
                let mut notes = local_frame_notes.clone();
                notes.push("reference_audio_dropped_after_audio_only_reject".into());
                log_video_create_params(&no_audio, &notes, out_path);
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
                        no_audio.to_json(),
                        timeout,
                        self.poll_progress_hook(),
                        should_cancel.clone(),
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
                            "video_generate_drop_ref_audio",
                            e2,
                        )
                    })?
            }
            Err(e) if !is_h3 && is_seedance_caption_empty_err(&e) => {
                // First reinforce captions (keep audio on); only then fall back to silent.
                tracing::warn!(
                    model = %model_for_err,
                    error = %e,
                    "Seedance rejected audio captions; retrying with reinforced ambient captions"
                );
                let mut reinforced = params.clone();
                reinforced.prompt = reinforce_seedance_audio_captions(&params.prompt);
                log_video_create_params(&reinforced, &local_frame_notes, out_path);
                if self.is_cancelled() {
                    return Err(VimaxError::Cancelled);
                }
                if is_usable_video_file(out_path) {
                    return Ok(());
                }
                let second = self
                    .services
                    .api
                    .generate_video_with_timeout_and_progress_cancellable(
                        &self.services.session,
                        reinforced.to_json(),
                        timeout,
                        self.poll_progress_hook(),
                        should_cancel.clone(),
                        None,
                    )
                    .await;
                match second {
                    Ok(r) => r,
                    Err(e2) if is_seedance_caption_empty_err(&e2) => {
                        tracing::warn!(
                            model = %model_for_err,
                            error = %e2,
                            "Seedance still rejected captions; retrying with generate_audio=false"
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
                                self.poll_progress_hook(),
                                should_cancel,
                                None,
                            )
                            .await
                            .map_err(|e3| {
                                if self.is_cancelled() {
                                    return VimaxError::Cancelled;
                                }
                                map_model_err(
                                    "video",
                                    Some(model_for_err.as_str()),
                                    "video_generate_silent_retry",
                                    e3,
                                )
                            })?
                    }
                    Err(e2) => {
                        if self.is_cancelled() {
                            return Err(VimaxError::Cancelled);
                        }
                        return Err(map_model_err(
                            "video",
                            Some(model_for_err.as_str()),
                            "video_generate_reinforced_audio",
                            e2,
                        ));
                    }
                }
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

        let url = record.video_url().ok_or_else(|| {
            if record.is_success() {
                VimaxError::Video("video task succeeded but no video_url".into())
            } else {
                // Surface the upstream failure reason (e.g. InputTextSensitiveContentDetected)
                // instead of a misleading success message.
                VimaxError::Video(video_task_failure_message(&record))
            }
        })?;

        self.emit_progress("video_download", "downloading video", None);
        download_video(&url, out_path).await?;
        self.emit_video_credits(&record);

        if let Some(lf_out) = last_frame_out {
            if let Some(lf_url) = record.last_frame_url() {
                match download_image_still(&lf_url, lf_out).await {
                    Ok(()) => tracing::info!(
                        out = %lf_out.display(),
                        "saved Seedance return_last_frame still"
                    ),
                    Err(e) => tracing::warn!(
                        out = %lf_out.display(),
                        error = %e,
                        "failed to download return_last_frame; caller may ffmpeg-extract"
                    ),
                }
            } else if want_last_frame {
                tracing::info!(
                    "return_last_frame requested but no last_frame_url in response; caller may ffmpeg-extract"
                );
            }
        }

        Ok(())
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

/// `reference_audio` without any image/video reference is rejected by Seedance.
fn is_seedance_audio_only_ref_err(err: &nomifun_cloud::ServerClientError) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("reference_audio cannot be the only")
        || (s.contains("reference_audio")
            && s.contains("only reference")
            && (s.contains("invalidparameter") || s.contains("not valid")))
}

/// Append a strong ambient/BGM caption block so a second attempt can keep `generate_audio=true`.
fn reinforce_seedance_audio_captions(prompt: &str) -> String {
    const REINFORCE: &str = "Throughout: <clear environmental ambience and scene-matched foley> \
(soft continuous cinematic atmospheric underscore, same motif tempo and instrumentation \
across adjacent shots, stable moderate volume)";
    let trimmed = prompt.trim_end();
    if trimmed.to_ascii_lowercase().contains("throughout:") {
        format!(
            "{trimmed}\nAlso ensure audible sound for the whole clip: {REINFORCE}"
        )
    } else {
        format!("{trimmed}\n{REINFORCE}")
    }
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
        return_last_frame = ?params.return_last_frame,
        prompt_chars = params.prompt.chars().count(),
        prompt_preview = %prompt_preview,
        image_count = params.images.len(),
        images = ?image_summaries,
        reference_video = params
            .reference_video_url
            .as_deref()
            .map(redact_media_url_for_log),
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

async fn download_image_still(url: &str, out_path: &Path) -> VimaxResult<()> {
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| VimaxError::Video(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(VimaxError::Video(format!(
            "last_frame download failed: HTTP {}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| VimaxError::Video(e.to_string()))?;
    write_image_bytes_atomic(&bytes, out_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_cloud::ServerClientError;

    #[test]
    fn detects_audio_only_reference_reject() {
        let err = ServerClientError::Api {
            code: 400,
            msg: "InvalidParameter (The parameter `content` specified in the request is not valid: \
reference_audio cannot be the only reference input. Request id: abc)"
                .into(),
        };
        assert!(is_seedance_audio_only_ref_err(&err));
        assert!(!is_seedance_caption_empty_err(&err));
    }
}

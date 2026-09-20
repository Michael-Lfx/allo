//! Flowy image/video generation for canvas tasks (Seedream / Seedance via flowy-cloud).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomi_vimax::{
    clip_bounds_for_model, max_reference_audio, FlowyImage, FlowyVideo, FlowyVimaxServices,
    VimaxImage, VimaxVideo,
};
use nomifun_common::AppError;
use tracing::{info, warn};

use crate::dto::GenerationTaskStatus;
use crate::service::{CanvasService, InternalTask};

pub fn load_flowy(data_dir: &Path) -> Option<FlowyVimaxServices> {
    let cfg: GatewayConfig = load_user_config_file(&config_yaml_path(Some(data_dir))).ok()?;
    FlowyVimaxServices::try_new(&cfg, data_dir)
}

pub fn map_vimax_err(e: nomi_vimax::VimaxError) -> AppError {
    match e {
        nomi_vimax::VimaxError::NotAuthenticated => AppError::Unauthorized(e.to_string()),
        nomi_vimax::VimaxError::InvalidParams(m) => AppError::BadRequest(m),
        nomi_vimax::VimaxError::Cancelled => AppError::BadRequest("cancelled".into()),
        other => AppError::Internal(other.to_string()),
    }
}

fn is_cancel_err(e: &AppError) -> bool {
    matches!(e, AppError::BadRequest(m) if m == "cancelled")
}

pub async fn run_generation_task(service: Arc<CanvasService>, task_id: String) {
    let Some(snapshot) = service.task_snapshot(&task_id).await else {
        return;
    };
    let cancel = service.task_cancel_token(&task_id).await;

    if cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
        service
            .set_task_status(&task_id, GenerationTaskStatus::Canceled, 1.0, None, None)
            .await;
        return;
    }

    service
        .set_task_status(&task_id, GenerationTaskStatus::Running, 0.05, None, None)
        .await;

    let flowy = match load_flowy(service.data_dir()) {
        Some(f) => f,
        None => {
            service
                .set_task_status(
                    &task_id,
                    GenerationTaskStatus::Failed,
                    1.0,
                    Some("Flowy cloud is not configured or not authenticated".into()),
                    None,
                )
                .await;
            return;
        }
    };

    let result = match snapshot.mode.as_str() {
        "image" | "t2i" | "i2i" => run_image(&service, &flowy, &snapshot, cancel.as_ref()).await,
        "video" | "t2v" | "i2v" => run_video(&service, &flowy, &snapshot, cancel.clone()).await,
        other => Err(AppError::BadRequest(format!("unsupported mode: {other}"))),
    };

    // Prefer canceled status if the token fired during the run.
    if cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
        service
            .set_task_status(&task_id, GenerationTaskStatus::Canceled, 1.0, None, None)
            .await;
        return;
    }

    match result {
        Ok(media_id) => {
            info!(%task_id, %media_id, "video-canvas generation succeeded");
            service
                .set_task_status(
                    &task_id,
                    GenerationTaskStatus::Succeeded,
                    1.0,
                    None,
                    Some(media_id),
                )
                .await;
        }
        Err(e) if is_cancel_err(&e) => {
            service
                .set_task_status(&task_id, GenerationTaskStatus::Canceled, 1.0, None, None)
                .await;
        }
        Err(e) => {
            warn!(%task_id, error = %e, "video-canvas generation failed");
            service
                .set_task_status(
                    &task_id,
                    GenerationTaskStatus::Failed,
                    1.0,
                    Some(e.to_string()),
                    None,
                )
                .await;
        }
    }
}

async fn run_image(
    service: &CanvasService,
    flowy: &FlowyVimaxServices,
    task: &InternalTask,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<String, AppError> {
    if cancel.is_some_and(|t| t.is_cancelled()) {
        return Err(AppError::BadRequest("cancelled".into()));
    }
    let out_path = scratch_path(service, &task.task_id, "png");
    ensure_parent(&out_path).await?;
    let classified = classify_task_media(service, task).await?;
    let ref_refs: Vec<&Path> = classified.images.iter().map(PathBuf::as_path).collect();

    // Seedream 等图模型：走 FlowyImage（与 Agent/vimax 同一套云端 API）。
    let backend: FlowyImage =
        flowy.image_with_model_and_aspect(task.model.clone(), task.aspect_ratio.clone());
    backend
        .generate(&task.prompt, &ref_refs, &out_path)
        .await
        .map_err(map_vimax_err)?;

    if cancel.is_some_and(|t| t.is_cancelled()) {
        let _ = tokio::fs::remove_file(&out_path).await;
        return Err(AppError::BadRequest("cancelled".into()));
    }

    let bytes = tokio::fs::read(&out_path)
        .await
        .map_err(|e| AppError::Internal(format!("read generated image: {e}")))?;
    let _ = tokio::fs::remove_file(&out_path).await;
    service
        .ingest_generated_bytes(
            bytes,
            "image",
            "image/png",
            "png",
            short_title(&task.task_id),
        )
        .await
}

async fn run_video(
    service: &CanvasService,
    flowy: &FlowyVimaxServices,
    task: &InternalTask,
    cancel: Option<tokio_util::sync::CancellationToken>,
) -> Result<String, AppError> {
    if cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
        return Err(AppError::BadRequest("cancelled".into()));
    }
    let out_path = scratch_path(service, &task.task_id, "mp4");
    ensure_parent(&out_path).await?;

    let classified = classify_task_media(service, task).await?;
    let first_path = resolve_frame_path(service, task.first_frame_media_id.as_deref()).await?;
    let last_path = resolve_frame_path(service, task.last_frame_media_id.as_deref()).await?;
    let has_frames = first_path.is_some() || last_path.is_some();
    let ref_images = extra_image_refs_when_frames_present(has_frames, classified.images);
    let ref_video = extra_video_ref_when_frames_present(has_frames, classified.videos);
    let ref_audios = classified.audios;
    let model = task.model.as_deref().unwrap_or("");
    if !ref_audios.is_empty() && max_reference_audio(model) == 0 {
        return Err(AppError::BadRequest(ERR_AUDIO_UNSUPPORTED.into()));
    }
    let has_visual = has_frames || !ref_images.is_empty() || ref_video.is_some();
    if !ref_audios.is_empty() && !has_visual {
        return Err(AppError::BadRequest(ERR_AUDIO_NEEDS_VISUAL.into()));
    }
    let ref_refs: Vec<&Path> = ref_images.iter().map(PathBuf::as_path).collect();
    let audio_refs: Vec<&Path> = ref_audios.iter().map(PathBuf::as_path).collect();
    let duration = clip_bounds_for_model(model).clamp_secs(task.duration_secs.unwrap_or(5));

    let backend: FlowyVideo = flowy.video_with_session_quality(
        task.model.clone(),
        cancel.clone(),
        task.aspect_ratio.clone(),
        task.resolution.clone(),
        None,
    );
    backend
        .generate(
            &task.prompt,
            first_path.as_deref(),
            last_path.as_deref(),
            &ref_refs,
            duration,
            &out_path,
            None,
            ref_video.as_deref(),
            &audio_refs,
        )
        .await
        .map_err(map_vimax_err)?;

    if cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
        let _ = tokio::fs::remove_file(&out_path).await;
        return Err(AppError::BadRequest("cancelled".into()));
    }

    let bytes = tokio::fs::read(&out_path)
        .await
        .map_err(|e| AppError::Internal(format!("read generated video: {e}")))?;
    let _ = tokio::fs::remove_file(&out_path).await;
    service
        .ingest_generated_bytes(
            bytes,
            "video",
            "video/mp4",
            "mp4",
            short_title(&task.task_id),
        )
        .await
}

fn scratch_path(service: &CanvasService, task_id: &str, ext: &str) -> PathBuf {
    service
        .data_dir()
        .join(crate::CANVAS_REL_DIR)
        .join("scratch")
        .join(format!("{task_id}.{ext}"))
}

async fn ensure_parent(path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        crate::fsio::ensure_dir(parent).await?;
    }
    Ok(())
}

const ERR_FRAME_NOT_IMAGE: &str = "首帧/尾帧必须是图片（PNG/JPEG/WebP），不能使用音频或视频。";
const ERR_AUDIO_UNSUPPORTED: &str =
    "当前视频模型不支持参考音频。请改用 Seedance 或 Wan 3.0，或断开音频节点。";
const ERR_AUDIO_NEEDS_VISUAL: &str =
    "参考音频需要同时连接至少一张参考图或参考视频。Seedance / Wan 不能只凭音频生成。";

#[derive(Debug, Default)]
struct ClassifiedMedia {
    images: Vec<PathBuf>,
    audios: Vec<PathBuf>,
    videos: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaClass {
    Image,
    Audio,
    Video,
}

#[cfg_attr(not(test), allow(dead_code))]
fn classify_media_kind(kind: &str, mime: &str, ext: &str) -> MediaClass {
    classify_media_kind_with_head(kind, mime, ext, &[])
}

fn looks_like_wav_head(head: &[u8]) -> bool {
    head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WAVE"
}

fn looks_like_video_head(head: &[u8]) -> bool {
    head.len() >= 12 && &head[4..8] == b"ftyp"
}

/// Index kind/mime/ext can lie (Windows WAV blobs often land as `file/*.bin`).
/// Magic wins when the first bytes are a WAV or ISO-BMFF video container.
fn classify_media_kind_with_head(kind: &str, mime: &str, ext: &str, head: &[u8]) -> MediaClass {
    if looks_like_wav_head(head) {
        return MediaClass::Audio;
    }
    if looks_like_video_head(head) {
        return MediaClass::Video;
    }
    let kind = kind.trim().to_ascii_lowercase();
    let mime = mime.trim().to_ascii_lowercase();
    let ext = ext.trim().to_ascii_lowercase();
    if kind == "audio"
        || mime.starts_with("audio/")
        || matches!(ext.as_str(), "wav" | "mp3" | "m4a" | "ogg" | "aac" | "flac")
    {
        MediaClass::Audio
    } else if kind == "video"
        || mime.starts_with("video/")
        || matches!(ext.as_str(), "mp4" | "webm" | "mov" | "mkv")
    {
        MediaClass::Video
    } else {
        MediaClass::Image
    }
}

fn extra_image_refs_when_frames_present(has_frames: bool, images: Vec<PathBuf>) -> Vec<PathBuf> {
    if has_frames {
        Vec::new()
    } else {
        images
    }
}

fn extra_video_ref_when_frames_present(has_frames: bool, videos: Vec<PathBuf>) -> Option<PathBuf> {
    if has_frames {
        None
    } else {
        videos.into_iter().next()
    }
}

async fn classify_task_media(
    service: &CanvasService,
    task: &InternalTask,
) -> Result<ClassifiedMedia, AppError> {
    let mut ids = Vec::new();
    ids.extend(task.reference_media_ids.iter().cloned());
    ids.extend(task.audio_media_ids.iter().cloned());
    if let Some(id) = &task.reference_video_media_id {
        ids.push(id.clone());
    }
    let mut seen = HashSet::new();
    let mut out = ClassifiedMedia::default();
    for id in ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let (kind, mime, ext, path) = service.media_kind_mime_path(&id).await?;
        let mut head = [0u8; 12];
        if let Ok(mut file) = tokio::fs::File::open(&path).await {
            use tokio::io::AsyncReadExt;
            let _ = file.read(&mut head).await;
        }
        match classify_media_kind_with_head(&kind, &mime, &ext, &head) {
            MediaClass::Image => out.images.push(path),
            MediaClass::Audio => out.audios.push(path),
            MediaClass::Video => out.videos.push(path),
        }
    }
    Ok(out)
}

async fn resolve_frame_path(
    service: &CanvasService,
    media_id: Option<&str>,
) -> Result<Option<PathBuf>, AppError> {
    let Some(id) = media_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let (kind, mime, ext, path) = service.media_kind_mime_path(id).await?;
    let mut head = [0u8; 12];
    if let Ok(mut file) = tokio::fs::File::open(&path).await {
        use tokio::io::AsyncReadExt;
        let _ = file.read(&mut head).await;
    }
    if classify_media_kind_with_head(&kind, &mime, &ext, &head) != MediaClass::Image {
        return Err(AppError::BadRequest(ERR_FRAME_NOT_IMAGE.into()));
    }
    Ok(Some(path))
}

fn short_title(task_id: &str) -> String {
    let take = task_id.len().min(8);
    format!("gen-{}", &task_id[..take])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_wav_as_audio_even_when_listed_as_a_generic_file() {
        assert_eq!(
            classify_media_kind("file", "application/octet-stream", "wav"),
            MediaClass::Audio
        );
        assert_eq!(
            classify_media_kind("audio", "audio/wav", "wav"),
            MediaClass::Audio
        );
        assert_eq!(
            classify_media_kind("image", "image/png", "png"),
            MediaClass::Image
        );
        assert_eq!(
            classify_media_kind("video", "video/mp4", "mp4"),
            MediaClass::Video
        );
        let wav = b"RIFF\0\0\0\0WAVEfmt ";
        assert_eq!(
            classify_media_kind_with_head("image", "image/png", "png", wav),
            MediaClass::Audio
        );
        assert_eq!(
            classify_media_kind_with_head("file", "application/octet-stream", "bin", wav),
            MediaClass::Audio
        );
        let png = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 0];
        assert_eq!(
            classify_media_kind_with_head("file", "application/octet-stream", "bin", &png),
            MediaClass::Image
        );
    }

    #[test]
    fn seedance_frame_roles_drop_extra_images_but_keep_audio() {
        let images = vec![PathBuf::from("a.png"), PathBuf::from("b.png")];
        let videos = vec![PathBuf::from("clip.mp4")];
        assert!(extra_image_refs_when_frames_present(true, images.clone()).is_empty());
        assert_eq!(
            extra_image_refs_when_frames_present(false, images.clone()),
            images
        );
        assert!(extra_video_ref_when_frames_present(true, videos.clone()).is_none());
        assert_eq!(
            extra_video_ref_when_frames_present(false, videos),
            Some(PathBuf::from("clip.mp4"))
        );
    }

    #[test]
    fn h3_rejects_reference_audio_capacity() {
        assert_eq!(max_reference_audio("flowy/MiniMax-H3"), 0);
        assert!(max_reference_audio("AIPC-Doubao-Seedance-2.0") > 0);
        assert!(max_reference_audio("flowy/wan3.0-video") > 0);
    }
}

//! Flowy image/video generation for canvas tasks (Seedream / Seedance via flowy-cloud).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomi_vimax::{FlowyImage, FlowyVideo, FlowyVimaxServices, VimaxImage, VimaxVideo};
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
    let refs = resolve_media_paths(service, &task.reference_media_ids).await?;
    let ref_refs: Vec<&Path> = refs.iter().map(PathBuf::as_path).collect();

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

    let first_path = match &task.first_frame_media_id {
        Some(id) => Some(service.media_file_path(id).await?),
        None => None,
    };
    let last_path = match &task.last_frame_media_id {
        Some(id) => Some(service.media_file_path(id).await?),
        None => None,
    };
    let ref_images: Vec<PathBuf> = if first_path.is_some() || last_path.is_some() {
        Vec::new()
    } else {
        resolve_media_paths(service, &task.reference_media_ids).await?
    };
    let ref_refs: Vec<&Path> = ref_images.iter().map(PathBuf::as_path).collect();
    // Seedance 系列常用 4–15s；默认 5s，与 Agent 规划预算一致。
    let duration = task.duration_secs.unwrap_or(5).clamp(2, 15);

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

async fn resolve_media_paths(
    service: &CanvasService,
    ids: &[String],
) -> Result<Vec<PathBuf>, AppError> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(service.media_file_path(id).await?);
    }
    Ok(out)
}

fn short_title(task_id: &str) -> String {
    let take = task_id.len().min(8);
    format!("gen-{}", &task_id[..take])
}

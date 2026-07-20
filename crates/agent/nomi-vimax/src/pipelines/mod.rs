//! Script2Video / Idea2Video / Novel2Video pipeline entrypoints.

mod idea2video;
mod novel2video;
mod script2video;

pub use idea2video::Idea2VideoPipeline;
pub use novel2video::Novel2VideoPipeline;
pub use script2video::Script2VideoPipeline;

use std::path::Path;
use std::sync::Arc;

use crate::backends::{VimaxChat, VimaxImage, VimaxVideo};
use crate::error::VimaxResult;
use crate::progress::ProgressCallback;
use crate::session::{read_json_artifact, write_json_artifact, write_text_artifact};

/// Shared backend handles for pipelines.
#[derive(Clone)]
pub struct PipelineBackends {
    pub chat: Arc<dyn VimaxChat>,
    pub image: Arc<dyn VimaxImage>,
    pub video: Arc<dyn VimaxVideo>,
    /// Optional Flowy handle for embeddings-backed RAG.
    pub flowy: Option<crate::backends::FlowyVimaxServices>,
}

pub(crate) fn emit(progress: &Option<ProgressCallback>, stage: &str, message: &str) {
    if let Some(cb) = progress {
        cb(stage, message, None);
    }
}

pub(crate) fn emit_pct(
    progress: &Option<ProgressCallback>,
    stage: &str,
    message: &str,
    pct: f32,
) {
    if let Some(cb) = progress {
        cb(
            stage,
            message,
            Some(serde_json::json!({ "progress": pct })),
        );
    }
}

pub(crate) async fn load_or_write_json<T, F, Fut>(
    path: &Path,
    generate: F,
) -> VimaxResult<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = VimaxResult<T>>,
{
    if path.exists() {
        return read_json_artifact(path).await;
    }
    let value = generate().await?;
    write_json_artifact(path, &value).await?;
    Ok(value)
}

pub(crate) async fn load_or_write_text<F, Fut>(path: &Path, generate: F) -> VimaxResult<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = VimaxResult<String>>,
{
    if path.exists() {
        return Ok(tokio::fs::read_to_string(path).await?);
    }
    let value = generate().await?;
    write_text_artifact(path, &value).await?;
    Ok(value)
}

pub(crate) fn group_shots_into_cameras(
    shot_descriptions: &[crate::domain::ShotDescription],
) -> Vec<crate::domain::Camera> {
    use std::collections::BTreeMap;
    let mut cameras_by_idx: BTreeMap<i32, crate::domain::Camera> = BTreeMap::new();
    for shot in shot_descriptions {
        let cam = cameras_by_idx.entry(shot.cam_idx).or_insert_with(|| {
            crate::domain::Camera {
                idx: shot.cam_idx,
                active_shot_idxs: vec![],
                parent_cam_idx: None,
                parent_shot_idx: None,
                reason: None,
                is_parent_fully_covers_child: None,
                missing_info: None,
            }
        });
        cam.active_shot_idxs.push(shot.idx);
    }
    cameras_by_idx.into_values().collect()
}

pub(crate) fn safe_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

//! Script2Video / Idea2Video / Novel2Video pipeline entrypoints.

mod idea2video;
mod novel2video;
mod script2video;

pub use idea2video::Idea2VideoPipeline;
pub use novel2video::Novel2VideoPipeline;
pub use script2video::Script2VideoPipeline;

use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

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
    /// When cancelled, pipelines stop before the next video API call.
    pub cancel: Option<CancellationToken>,
}

impl PipelineBackends {
    pub fn is_cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|t| t.is_cancelled())
    }
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

/// Fix invalid LLM camera-tree edges that would deadlock frame generation.
/// Common failure: `parent_shot_idx` points at a shot owned by the same camera
/// (e.g. cam 3 waits for shot 7 while shots=[6,7]).
pub(crate) fn sanitize_camera_tree(cameras: &mut [crate::domain::Camera]) {
    use std::collections::{HashMap, HashSet};

    let mut shot_owner: HashMap<i32, i32> = HashMap::new();
    let cam_idxs: HashSet<i32> = cameras.iter().map(|c| c.idx).collect();
    for cam in cameras.iter() {
        for &shot in &cam.active_shot_idxs {
            shot_owner.insert(shot, cam.idx);
        }
    }

    for cam in cameras.iter_mut() {
        let mut clear_parent = false;

        if cam.parent_cam_idx == Some(cam.idx) {
            clear_parent = true;
        }

        if let Some(ps) = cam.parent_shot_idx {
            if cam.active_shot_idxs.contains(&ps) {
                // Self-owned parent shot → impossible dependency.
                clear_parent = true;
            } else if let Some(&owner) = shot_owner.get(&ps) {
                if owner == cam.idx {
                    clear_parent = true;
                } else {
                    // Prefer the camera that actually owns the parent shot.
                    cam.parent_cam_idx = Some(owner);
                }
            } else {
                // Unknown shot index.
                clear_parent = true;
            }
        }

        if cam
            .parent_cam_idx
            .is_some_and(|idx| !cam_idxs.contains(&idx))
        {
            clear_parent = true;
        }

        if clear_parent {
            tracing::warn!(
                camera = cam.idx,
                parent_cam = ?cam.parent_cam_idx,
                parent_shot = ?cam.parent_shot_idx,
                shots = ?cam.active_shot_idxs,
                "clearing invalid camera parent (would deadlock frame generation)"
            );
            cam.parent_cam_idx = None;
            cam.parent_shot_idx = None;
        }
    }

    // Break simple parent_cam cycles by clearing the higher-index edge.
    let parents: HashMap<i32, Option<i32>> = cameras
        .iter()
        .map(|c| (c.idx, c.parent_cam_idx))
        .collect();
    let mut cyclic: HashSet<i32> = HashSet::new();
    for cam in cameras.iter() {
        let mut seen = HashSet::new();
        let mut cur = Some(cam.idx);
        while let Some(idx) = cur {
            if !seen.insert(idx) {
                cyclic.insert(idx);
                break;
            }
            cur = parents.get(&idx).copied().flatten();
        }
    }
    if !cyclic.is_empty() {
        for cam in cameras.iter_mut() {
            if cyclic.contains(&cam.idx) {
                tracing::warn!(camera = cam.idx, "clearing cyclic camera parent");
                cam.parent_cam_idx = None;
                cam.parent_shot_idx = None;
            }
        }
    }

    if let Some(first) = cameras.first_mut() {
        first.parent_cam_idx = None;
        first.parent_shot_idx = None;
    }
}

#[cfg(test)]
mod sanitize_tests {
    use super::{resolve_film_root, sanitize_camera_tree};
    use crate::domain::Camera;

    fn cam(idx: i32, shots: &[i32], parent_shot: Option<i32>) -> Camera {
        Camera {
            idx,
            active_shot_idxs: shots.to_vec(),
            parent_cam_idx: None,
            parent_shot_idx: parent_shot,
            reason: None,
            is_parent_fully_covers_child: None,
            missing_info: None,
        }
    }

    #[test]
    fn clears_self_owned_parent_shot() {
        // Repro: cam 3 parent_shot=7 while shots=[6,7]
        let mut cams = vec![
            cam(0, &[0, 1], None),
            cam(3, &[6, 7], Some(7)),
        ];
        sanitize_camera_tree(&mut cams);
        assert_eq!(cams[1].parent_shot_idx, None);
        assert_eq!(cams[1].parent_cam_idx, None);
    }

    #[test]
    fn rewrites_parent_cam_to_shot_owner() {
        let mut cams = vec![
            cam(0, &[0, 7], None),
            {
                let mut c = cam(3, &[6], Some(7));
                c.parent_cam_idx = Some(9); // wrong
                c
            },
        ];
        sanitize_camera_tree(&mut cams);
        assert_eq!(cams[1].parent_shot_idx, Some(7));
        assert_eq!(cams[1].parent_cam_idx, Some(0));
    }

    #[test]
    fn resolve_film_root_climbs_from_scene() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("idea2video");
        let scene = root.join("scene_1");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(root.join("story.txt"), "once upon").unwrap();
        std::fs::write(scene.join("characters.json"), "[]").unwrap();
        assert_eq!(resolve_film_root(&scene), root);
        assert_eq!(resolve_film_root(&root), root);
    }
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

/// Workflow root that owns the shared cast + portrait registry.
/// Scene dirs climb to the parent idea/novel/script working directory.
pub(crate) fn resolve_film_root(working_dir: &Path) -> std::path::PathBuf {
    let mut best = working_dir.to_path_buf();
    let mut cur = working_dir.to_path_buf();
    for _ in 0..8 {
        if cur.join("story.txt").exists()
            || cur.join("script.json").exists()
            || cur.join("events").is_dir()
        {
            return cur;
        }
        if cur.join("characters.json").exists()
            || cur.join("character_portraits_registry.json").exists()
        {
            best = cur.clone();
        }
        match cur.parent() {
            Some(p) if p.as_os_str() != cur.as_os_str() => cur = p.to_path_buf(),
            _ => break,
        }
    }
    best
}

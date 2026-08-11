//! Builds a [`CreativeFilm`] by reading `scene_plan` / `asset_manifest` /
//! `renders/final.mp4` off disk. Tolerant of partial/missing artifacts — this
//! is a best-effort projection for Canvas, not a validator (that is
//! `artifacts::validate`'s job).

use serde_json::Value;

use crate::error::MontageResult;
use crate::paths::ProjectPaths;
use crate::project::ProjectRecord;

use super::ir::{CreativeFilm, CreativeMediaKind, CreativeMediaRef, CreativeScene, CreativeShot, CREATIVE_IR_VERSION};

pub fn scan_project(paths: &ProjectPaths, record: &ProjectRecord) -> MontageResult<CreativeFilm> {
    let scene_plan = read_artifact_json(paths, "scene_plan");
    let asset_manifest = read_artifact_json(paths, "asset_manifest");

    let mut scenes = Vec::new();
    if let Some(plan) = &scene_plan {
        if let Some(arr) = plan.get("scenes").and_then(|v| v.as_array()) {
            for scene_v in arr {
                let key = scene_v.get("key").and_then(|v| v.as_str()).unwrap_or("scene").to_string();
                let title = scene_v.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let summary = scene_v.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let mut shots = Vec::new();
                if let Some(shot_arr) = scene_v.get("shots").and_then(|v| v.as_array()) {
                    for (i, shot_v) in shot_arr.iter().enumerate() {
                        let idx = shot_v.get("idx").and_then(|v| v.as_i64()).unwrap_or(i as i64);
                        let description = shot_v.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let is_motion = shot_v.get("is_motion").and_then(|v| v.as_bool()).unwrap_or(false);
                        let media = find_shot_media(paths, asset_manifest.as_ref(), &key, idx);
                        shots.push(CreativeShot {
                            idx,
                            description,
                            is_motion,
                            media,
                        });
                    }
                }
                scenes.push(CreativeScene { key, title, summary, shots });
            }
        }
    }

    let final_video_path = paths.final_video_path();
    let final_video = if final_video_path.is_file() {
        Some(media_ref(paths, &final_video_path, "Final Cut"))
    } else {
        None
    };

    Ok(CreativeFilm {
        version: CREATIVE_IR_VERSION,
        project_id: record.id.clone(),
        title: record.title.clone(),
        pipeline: record.pipeline.clone(),
        style_playbook: record.style_playbook.clone(),
        scenes,
        final_video,
    })
}

fn find_shot_media(
    paths: &ProjectPaths,
    asset_manifest: Option<&Value>,
    scene_key: &str,
    shot_idx: i64,
) -> Option<CreativeMediaRef> {
    let manifest = asset_manifest?;
    let scenes = manifest.get("scenes")?.as_array()?;
    let scene = scenes.iter().find(|s| s.get("key").and_then(|v| v.as_str()) == Some(scene_key))?;
    let shots = scene.get("shots")?.as_array()?;
    let shot = shots
        .iter()
        .find(|s| s.get("idx").and_then(|v| v.as_i64()) == Some(shot_idx))?;
    let file_name = shot
        .get("video_name")
        .and_then(|v| v.as_str())
        .or_else(|| shot.get("image_name").and_then(|v| v.as_str()))?;
    let is_video = shot.get("video_name").and_then(|v| v.as_str()).is_some();
    let dir = if is_video { paths.assets_video_dir() } else { paths.assets_images_dir() };
    let abs = dir.join(file_name);
    if !abs.is_file() {
        return None;
    }
    Some(media_ref(paths, &abs, file_name))
}

fn media_ref(paths: &ProjectPaths, abs: &std::path::Path, title: &str) -> CreativeMediaRef {
    let rel = abs
        .strip_prefix(&paths.root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/");
    let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("");
    CreativeMediaRef {
        abs_path: abs.to_path_buf(),
        rel_path: rel,
        kind: CreativeMediaKind::from_extension(ext),
        title: title.to_string(),
    }
}

fn read_artifact_json(paths: &ProjectPaths, name: &str) -> Option<Value> {
    let raw = std::fs::read_to_string(paths.artifact_path(name)).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::VideoGenMode;
    use crate::project::{CreateProjectRequest, ModelSelection, ProjectRecord};

    #[test]
    fn scan_handles_missing_artifacts_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), "p1");
        paths.ensure_dirs().unwrap();
        let record = ProjectRecord::new(
            CreateProjectRequest {
                title: "T".into(),
                pipeline: "cinematic".into(),
                prompt: "x".into(),
                style_playbook: None,
                checkpoint_policy: None,
                models: ModelSelection::default(),
                output: None,
                budget_credits: None,
                reference_video_path: None,
            },
            VideoGenMode::Agent,
            1000,
        );
        let film = scan_project(&paths, &record).unwrap();
        assert!(film.scenes.is_empty());
        assert!(film.final_video.is_none());
    }

    #[test]
    fn scan_reads_scene_plan_and_matches_assets() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), "p2");
        paths.ensure_dirs().unwrap();
        std::fs::write(
            paths.artifact_path("scene_plan"),
            serde_json::json!({
                "scenes": [{"key": "s1", "title": "Opening", "summary": "…", "shots": [{"idx": 0, "description": "wide shot", "is_motion": true}]}]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            paths.artifact_path("asset_manifest"),
            serde_json::json!({
                "scenes": [{"key": "s1", "shots": [{"idx": 0, "video_name": "shot_0.mp4"}]}]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(paths.assets_video_dir().join("shot_0.mp4"), b"fake").unwrap();

        let record = ProjectRecord::new(
            CreateProjectRequest {
                title: "T".into(),
                pipeline: "cinematic".into(),
                prompt: "x".into(),
                style_playbook: None,
                checkpoint_policy: None,
                models: ModelSelection::default(),
                output: None,
                budget_credits: None,
                reference_video_path: None,
            },
            VideoGenMode::Agent,
            1000,
        );
        let film = scan_project(&paths, &record).unwrap();
        assert_eq!(film.scenes.len(), 1);
        assert_eq!(film.scenes[0].shots.len(), 1);
        assert!(film.scenes[0].shots[0].media.is_some());
    }
}

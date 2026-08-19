//! Scan a ViMax session working directory into [`CreativeFilm`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::{Camera, CharacterInScene, ShotBriefDescription, ShotDescription, WorkflowKind};
use crate::error::{VimaxError, VimaxResult};
use crate::pipelines::resolve_film_root;
use crate::session::{SessionRecord, read_json_artifact, resolve_stored_asset_path};

use super::ir::{
    CreativeCharacter, CreativeFilm, CreativeMediaFile, CreativeMediaKind, CreativeScene,
    CreativeShot, CreativeWorldAsset, CreativeWorldKind, CREATIVE_IR_VERSION,
};

/// Build a high-fidelity creative film from an on-disk ViMax session.
pub async fn scan_session_film(
    session: &SessionRecord,
    working_dir: &Path,
) -> VimaxResult<CreativeFilm> {
    if !working_dir.is_dir() {
        return Err(VimaxError::msg(format!(
            "working dir missing: {}",
            working_dir.display()
        )));
    }

    let wf_root_name = session.workflow.artifact_root();
    let film_dir = working_dir.join(wf_root_name);
    if !film_dir.is_dir() {
        return Err(VimaxError::InvalidParams(format!(
            "workflow artifacts not found under {wf_root_name}/ — plan or render first"
        )));
    }

    let film_root = resolve_film_root(&film_dir);
    let film_characters = load_characters_with_portraits(&film_root).await?;
    let world_assets = load_world_assets(working_dir, &film_root).await?;

    let scenes = match session.workflow {
        WorkflowKind::Script2Video => {
            vec![scan_scene_dir(working_dir, &film_dir, "main", "主场景").await?]
        }
        WorkflowKind::Action2Video => {
            return Err(VimaxError::InvalidParams(
                "action imitation has no storyboard to open in Canvas".into(),
            ));
        }
        WorkflowKind::Idea2Video | WorkflowKind::Novel2Video => {
            scan_multi_scenes(working_dir, &film_dir).await?
        }
    };

    if scenes.is_empty() {
        return Err(VimaxError::InvalidParams(
            "no scannable scenes with storyboard/shots found".into(),
        ));
    }

    let final_video = media_if_exists(
        working_dir,
        &film_dir.join("final_video.mp4"),
        CreativeMediaKind::Video,
        "成片",
    );
    let cover = session
        .cover
        .as_deref()
        .map(|rel| working_dir.join(rel))
        .filter(|p| p.is_file())
        .map(|abs| {
            media_file(
                working_dir,
                &abs,
                CreativeMediaKind::Image,
                "封面",
            )
        })
        .or_else(|| {
            media_if_exists(
                working_dir,
                &film_dir.join("cover.png"),
                CreativeMediaKind::Image,
                "封面",
            )
        });

    Ok(CreativeFilm {
        version: CREATIVE_IR_VERSION,
        session_id: session.session_id.clone(),
        title: if session.title.trim().is_empty() {
            format!("ViMax · {}", session.session_id)
        } else {
            session.title.clone()
        },
        workflow: session.workflow,
        style: session.style.clone(),
        aspect_ratio: session.aspect_ratio.clone(),
        resolution: session.resolution.clone(),
        fps: session.fps,
        target_duration_secs: session.target_duration_secs,
        llm_model: session.llm_model.clone(),
        image_model: session.image_model.clone(),
        video_model: session.video_model.clone(),
        characters: film_characters,
        world_assets,
        scenes,
        final_video,
        cover,
    })
}

async fn scan_multi_scenes(
    working_dir: &Path,
    film_dir: &Path,
) -> VimaxResult<Vec<CreativeScene>> {
    let mut scenes = Vec::new();
    let mut rd = tokio::fs::read_dir(film_dir).await.map_err(VimaxError::Io)?;
    let mut entries = Vec::new();
    while let Some(entry) = rd.next_entry().await.map_err(VimaxError::Io)? {
        let name = entry.file_name().to_string_lossy().to_string();
        if !entry
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        if name.starts_with("scene_") {
            entries.push((name, entry.path()));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, path) in entries {
        if !path.join("storyboard.json").exists()
            && !path.join("shot_descriptions.json").exists()
            && !path.join("shots").is_dir()
        {
            continue;
        }
        let title = format!("场景 {}", name.trim_start_matches("scene_"));
        scenes.push(scan_scene_dir(working_dir, &path, &name, &title).await?);
    }

    // Fallback: treat film root itself as a scene if no scene_* dirs.
    if scenes.is_empty()
        && (film_dir.join("storyboard.json").exists() || film_dir.join("shots").is_dir())
    {
        scenes.push(scan_scene_dir(working_dir, film_dir, "main", "主场景").await?);
    }
    Ok(scenes)
}

async fn scan_scene_dir(
    working_dir: &Path,
    scene_dir: &Path,
    key: &str,
    title: &str,
) -> VimaxResult<CreativeScene> {
    let storyboard: Vec<ShotBriefDescription> =
        read_json_optional(scene_dir.join("storyboard.json")).await?;
    let shot_descriptions: Vec<ShotDescription> =
        read_json_optional(scene_dir.join("shot_descriptions.json")).await?;
    let camera_tree: Vec<Camera> = read_json_optional(scene_dir.join("camera_tree.json")).await?;
    let characters = load_characters_with_portraits(scene_dir).await?;
    let script = tokio::fs::read_to_string(scene_dir.join("script.txt"))
        .await
        .unwrap_or_default();

    let shots = collect_shots(working_dir, scene_dir, &storyboard, &shot_descriptions).await?;

    let artifact_root_rel = rel_path(working_dir, scene_dir);
    let final_video = media_if_exists(
        working_dir,
        &scene_dir.join("final_video.mp4"),
        CreativeMediaKind::Video,
        &format!("{title} 成片"),
    );

    Ok(CreativeScene {
        key: key.to_string(),
        title: title.to_string(),
        artifact_root_rel,
        script,
        characters,
        storyboard,
        shot_descriptions,
        camera_tree,
        shots,
        final_video,
    })
}

async fn collect_shots(
    working_dir: &Path,
    scene_dir: &Path,
    storyboard: &[ShotBriefDescription],
    shot_descriptions: &[ShotDescription],
) -> VimaxResult<Vec<CreativeShot>> {
    let mut idxs: Vec<i32> = shot_descriptions.iter().map(|s| s.idx).collect();
    if idxs.is_empty() {
        idxs = storyboard.iter().map(|s| s.idx).collect();
    }
    if idxs.is_empty() {
        // Discover from shots/ directory.
        let shots_dir = scene_dir.join("shots");
        if shots_dir.is_dir() {
            let mut rd = tokio::fs::read_dir(&shots_dir).await.map_err(VimaxError::Io)?;
            while let Some(entry) = rd.next_entry().await.map_err(VimaxError::Io)? {
                if !entry
                    .file_type()
                    .await
                    .map(|t| t.is_dir())
                    .unwrap_or(false)
                {
                    continue;
                }
                if let Ok(idx) = entry.file_name().to_string_lossy().parse::<i32>() {
                    idxs.push(idx);
                }
            }
            idxs.sort_unstable();
        }
    }

    let cam_by_shot: HashMap<i32, i32> = storyboard
        .iter()
        .map(|s| (s.idx, s.cam_idx))
        .chain(shot_descriptions.iter().map(|s| (s.idx, s.cam_idx)))
        .collect();

    let mut shots = Vec::with_capacity(idxs.len());
    for idx in idxs {
        let shot_dir = scene_dir.join("shots").join(idx.to_string());
        let cam_idx = cam_by_shot.get(&idx).copied().unwrap_or(idx);
        let artifact_rel = rel_path(working_dir, &shot_dir.join("video.mp4"));
        shots.push(CreativeShot {
            idx,
            cam_idx,
            artifact_rel,
            video: media_if_exists(
                working_dir,
                &shot_dir.join("video.mp4"),
                CreativeMediaKind::Video,
                &format!("镜头 {idx}"),
            ),
            first_frame: media_if_exists(
                working_dir,
                &shot_dir.join("first_frame.png"),
                CreativeMediaKind::Image,
                &format!("镜头 {idx} 首帧"),
            ),
            last_frame: media_if_exists(
                working_dir,
                &shot_dir.join("last_frame.png"),
                CreativeMediaKind::Image,
                &format!("镜头 {idx} 尾帧"),
            ),
            video_last_frame: media_if_exists(
                working_dir,
                &shot_dir.join("video_last_frame.png"),
                CreativeMediaKind::Image,
                &format!("镜头 {idx} 视频末帧"),
            ),
        });
    }
    Ok(shots)
}

async fn load_world_assets(
    working_dir: &Path,
    film_root: &Path,
) -> VimaxResult<Vec<CreativeWorldAsset>> {
    let registry_path = film_root.join("world_assets_registry.json");
    let registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
        read_json_optional(registry_path).await?;
    let mut out = Vec::new();
    for (group, kind) in [
        ("environments", CreativeWorldKind::Environment),
        ("props", CreativeWorldKind::Prop),
    ] {
        let Some(map) = registry.get(group) else {
            continue;
        };
        let mut keys: Vec<_> = map.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let Some(item) = map.get(&key) else {
                continue;
            };
            let Some(stored) = item.get("path").map(String::as_str).filter(|s| !s.trim().is_empty())
            else {
                continue;
            };
            let abs = resolve_stored_asset_path(stored, film_root);
            if !abs.is_file() {
                continue;
            }
            let desc = item
                .get("description")
                .cloned()
                .unwrap_or_else(|| key.clone());
            let title = match kind {
                CreativeWorldKind::Environment => format!("环境 · {key}"),
                CreativeWorldKind::Prop => format!("道具 · {key}"),
            };
            out.push(CreativeWorldAsset {
                kind,
                key,
                description: desc,
                media: media_file(working_dir, &abs, CreativeMediaKind::Image, &title),
            });
        }
    }
    Ok(out)
}

async fn load_characters_with_portraits(dir: &Path) -> VimaxResult<Vec<CreativeCharacter>> {
    let characters: Vec<CharacterInScene> =
        read_json_optional(dir.join("characters.json")).await?;
    let film_root = resolve_film_root(dir);
    let registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
        read_json_optional(film_root.join("character_portraits_registry.json")).await?;

    let mut out = Vec::with_capacity(characters.len());
    for c in &characters {
        let mut creative = CreativeCharacter::from_domain(c);
        if let Some(views) = registry.get(&c.identifier_in_scene) {
            for (view, paths) in views {
                // Prefer front / cameo / sheet / first available path value.
                let preferred = ["path", "file", "abs", "rel", "png", "image"];
                let mut path_str: Option<&String> = None;
                for key in preferred {
                    if let Some(v) = paths.get(key) {
                        if !v.trim().is_empty() {
                            path_str = Some(v);
                            break;
                        }
                    }
                }
                if path_str.is_none() {
                    path_str = paths.values().find(|v| !v.trim().is_empty());
                }
                let Some(stored) = path_str else { continue };
                let abs = resolve_stored_asset_path(stored, &film_root);
                if !abs.is_file() {
                    continue;
                }
                creative.portraits.insert(
                    view.clone(),
                    media_file(
                        dir,
                        &abs,
                        CreativeMediaKind::Image,
                        &format!("{} · {view}", c.identifier_in_scene),
                    ),
                );
            }
        }
        out.push(creative);
    }
    Ok(out)
}

async fn read_json_optional<T: for<'de> serde::Deserialize<'de>>(path: PathBuf) -> VimaxResult<T>
where
    T: Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    // Prefer typed JSON; fall back to empty when file is present but empty array/object mismatch.
    match read_json_artifact::<T>(&path).await {
        Ok(v) => Ok(v),
        Err(e) => {
            // Allow empty / null files for optional artifacts.
            let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "null" || trimmed == "[]" || trimmed == "{}" {
                return Ok(T::default());
            }
            // Try Value → T for slightly mismatched shapes.
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                if let Ok(v) = serde_json::from_value::<T>(val) {
                    return Ok(v);
                }
            }
            Err(e)
        }
    }
}

fn media_if_exists(
    working_dir: &Path,
    abs: &Path,
    kind: CreativeMediaKind,
    title: &str,
) -> Option<CreativeMediaFile> {
    if abs.is_file() {
        Some(media_file(working_dir, abs, kind, title))
    } else {
        None
    }
}

fn media_file(
    working_dir: &Path,
    abs: &Path,
    kind: CreativeMediaKind,
    title: &str,
) -> CreativeMediaFile {
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or(match kind {
            CreativeMediaKind::Video => "mp4",
            CreativeMediaKind::Image => "png",
            CreativeMediaKind::Audio => "mp3",
            CreativeMediaKind::File => "bin",
        })
        .to_ascii_lowercase();
    let mime = match (kind, ext.as_str()) {
        (CreativeMediaKind::Video, _) => "video/mp4".into(),
        (CreativeMediaKind::Audio, _) => "audio/mpeg".into(),
        (CreativeMediaKind::Image, "jpg" | "jpeg") => "image/jpeg".into(),
        (CreativeMediaKind::Image, "webp") => "image/webp".into(),
        (CreativeMediaKind::Image, _) => "image/png".into(),
        _ => "application/octet-stream".into(),
    };
    CreativeMediaFile {
        abs_path: abs.to_path_buf(),
        rel_path: rel_path(working_dir, abs),
        kind,
        mime,
        ext,
        title: title.to_string(),
    }
}

fn rel_path(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().replace('\\', "/"))
}

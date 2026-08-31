//! Materialize a ViMax Agent session into a Canvas project (high-fidelity).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_vimax::{
    build_canvas_document, media_local, scan_session_film, CreativeFilm, IngestedMedia, MediaIdMap,
    SessionRecord,
};
use nomifun_canvas::CanvasService;
use nomifun_common::AppError;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::service::VimaxApiService;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MaterializeToCanvasResult {
    pub project_id: String,
    pub title: String,
    pub session_id: String,
    pub node_count: u32,
    pub media_count: u32,
    pub scene_count: u32,
    pub shot_count: u32,
    pub warnings: Vec<String>,
    /// True when an existing canvas for this session was reopened (no new project).
    pub reused: bool,
}

#[derive(Debug, Deserialize)]
pub struct SyncFromCanvasRequest {
    pub project_id: String,
    /// When empty, sync every shot video node that has `alloVimax.kind == shot_video`.
    #[serde(default)]
    pub shots: Vec<SyncShotUpdate>,
    /// Re-concat scene / film finals after replacing clips. Default true.
    #[serde(default = "default_true")]
    pub reconcat: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct SyncShotUpdate {
    pub scene_key: String,
    pub shot_idx: i32,
    pub media_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncFromCanvasResult {
    pub session_id: String,
    pub updated_shots: u32,
    pub final_video: Option<String>,
    pub warnings: Vec<String>,
}

pub async fn materialize_session_to_canvas(
    vimax: &VimaxApiService,
    canvas: &Arc<CanvasService>,
    session_id: &str,
) -> Result<MaterializeToCanvasResult, AppError> {
    let session = vimax.get_session(session_id)?;

    // Idempotent: one Agent session maps to one Canvas project.
    if let Some(existing) = canvas.find_project_for_vimax_session(session_id).await? {
        let project = canvas.get_project(&existing.project_id).await?;
        let shot_count = project
            .doc
            .pointer("/alloCreative/film/scenes")
            .and_then(|v| v.as_array())
            .map(|scenes| {
                scenes
                    .iter()
                    .map(|s| {
                        s.get("shots")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len() as u32)
                            .unwrap_or(0)
                    })
                    .sum()
            })
            .unwrap_or(existing.node_count);
        let scene_count = project
            .doc
            .pointer("/alloCreative/film/scenes")
            .and_then(|v| v.as_array())
            .map(|a| a.len() as u32)
            .unwrap_or(0);
        info!(
            session_id = %session_id,
            project_id = %existing.project_id,
            "reusing existing canvas for vimax session"
        );
        return Ok(MaterializeToCanvasResult {
            project_id: existing.project_id,
            title: existing.title,
            session_id: session.session_id,
            node_count: existing.node_count,
            media_count: 0,
            scene_count,
            shot_count,
            warnings: vec![],
            reused: true,
        });
    }

    let working_dir = vimax.working_dir(session_id)?;

    let film = scan_session_film(&session, &working_dir)
        .await
        .map_err(map_vimax)?;

    let mut warnings = Vec::new();
    validate_film_quality(&film, &mut warnings);

    let media_ids = ingest_film_media(canvas, &film, &mut warnings).await?;
    if media_ids.is_empty()
        && film
            .scenes
            .iter()
            .all(|s| s.storyboard.is_empty() && s.shot_descriptions.is_empty() && s.shots.is_empty())
    {
        return Err(AppError::BadRequest(
            "nothing to materialize — run Plan (or Render) first".into(),
        ));
    }
    let doc = build_canvas_document(&film, &media_ids);

    let title = format!("{}（Canvas）", film.title);
    let meta = canvas
        .create_project_for_vimax_session(session_id, Some(title.clone()))
        .await?;
    let meta = canvas.put_doc(&meta.project_id, doc).await?;
    // put_doc may rewrite meta without the session binding — reaffirm link + field.
    let meta = canvas
        .set_vimax_session_on_project(&meta.project_id, session_id)
        .await?;

    let shot_count = film.scenes.iter().map(|s| s.shots.len() as u32).sum();
    info!(
        session_id = %session_id,
        project_id = %meta.project_id,
        media = media_ids.len(),
        shots = shot_count,
        "materialized vimax session to canvas"
    );

    Ok(MaterializeToCanvasResult {
        project_id: meta.project_id,
        title: meta.title,
        session_id: session.session_id,
        node_count: meta.node_count,
        media_count: media_ids.len() as u32,
        scene_count: film.scenes.len() as u32,
        shot_count,
        warnings,
        reused: false,
    })
}

pub async fn sync_canvas_shots_to_session(
    vimax: &VimaxApiService,
    canvas: &Arc<CanvasService>,
    session_id: &str,
    req: SyncFromCanvasRequest,
) -> Result<SyncFromCanvasResult, AppError> {
    let session = vimax.get_session(session_id)?;
    let working_dir = vimax.working_dir(session_id)?;

    let project = canvas.get_project(&req.project_id).await?;
    let allo = project
        .doc
        .get("alloCreative")
        .cloned()
        .unwrap_or(json!({}));
    let linked = allo
        .get("sessionId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if linked != session_id {
        return Err(AppError::BadRequest(format!(
            "canvas project is linked to session `{linked}`, not `{session_id}`"
        )));
    }

    let updates = if req.shots.is_empty() {
        collect_shot_updates_from_doc(&project.doc)?
    } else {
        req.shots
    };

    // No updates means nothing changed in Canvas — return early without error.
    if updates.is_empty() {
        return Ok(SyncFromCanvasResult {
            session_id: session_id.to_string(),
            updated_shots: 0,
            final_video: session.final_video.clone(),
            warnings: vec![],
        });
    }

    let mut warnings = Vec::new();
    let mut updated = 0_u32;
    let mut touched_scenes: HashMap<String, PathBuf> = HashMap::new();

    for upd in &updates {
        let scene_dir = resolve_scene_dir(&working_dir, &session, &upd.scene_key)?;
        let dest = scene_dir
            .join("shots")
            .join(upd.shot_idx.to_string())
            .join("video.mp4");
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(format!("create shot dir: {e}")))?;
        }
        let src = canvas.media_file_path(&upd.media_id).await?;
        tokio::fs::copy(&src, &dest)
            .await
            .map_err(|e| AppError::Internal(format!("write shot video: {e}")))?;
        // Invalidate derived last-frame so next Agent render regenerates continuity.
        let _ = tokio::fs::remove_file(scene_dir.join("shots").join(upd.shot_idx.to_string()).join("video_last_frame.png")).await;
        touched_scenes.insert(upd.scene_key.clone(), scene_dir);
        updated += 1;
    }

    let mut final_video = session.final_video.clone();
    if req.reconcat {
        for (scene_key, scene_dir) in &touched_scenes {
            match reconcat_scene(&scene_dir).await {
                Ok(path) => {
                    info!(%scene_key, path = %path.display(), "re-concatenated scene after canvas sync");
                }
                Err(e) => {
                    // 拼接失败不阻断写回（Canvas 写回的 shot 已落盘），但要把
                    // "为什么没拼成片" 透传给前端，便于用户在画布里继续补齐缺失
                    // 的 shot 后再触发一次写回。
                    warnings.push(format!("scene `{scene_key}` concat skipped: {e}"))
                }
            }
        }
        // Film-level concat for multi-scene or single scene final. 单 scene 情况下
        // reconcat_film 内部会复用 reconcat_scene 的「所有 shot 必须齐备」校验，
        // 任何缺失都不会污染 final_video。
        match reconcat_film(&working_dir, &session).await {
            Ok(Some(rel)) => {
                final_video = Some(rel);
                vimax
                    .set_session_final_video(session_id, final_video.clone())?;
            }
            Ok(None) => {
                // 无 scene final 且无 shots/，保持现有 final_video 不动
            }
            Err(e) => warnings.push(format!("film concat skipped: {e}")),
        }
    }

    Ok(SyncFromCanvasResult {
        session_id: session_id.to_string(),
        updated_shots: updated,
        final_video,
        warnings,
    })
}

async fn ingest_film_media(
    canvas: &CanvasService,
    film: &CreativeFilm,
    warnings: &mut Vec<String>,
) -> Result<MediaIdMap, AppError> {
    let mut map = MediaIdMap::new();
    for media in film.all_media_files() {
        if map.contains_key(&media.rel_path) {
            continue;
        }
        if !media.abs_path.is_file() {
            warnings.push(format!("missing media: {}", media.rel_path));
            continue;
        }
        match canvas
            .ingest_local_file(
                &media.abs_path,
                media.kind.as_str(),
                &media.mime,
                &media.ext,
                media.title.clone(),
            )
            .await
        {
            Ok(meta) => {
                map.insert(
                    media.rel_path.clone(),
                    IngestedMedia {
                        media_id: meta.media_id,
                        bytes: meta.bytes,
                        mime: meta.mime,
                    },
                );
            }
            Err(e) => warnings.push(format!("ingest {}: {e}", media.rel_path)),
        }
    }
    Ok(map)
}

fn validate_film_quality(film: &CreativeFilm, warnings: &mut Vec<String>) {
    let any_video = film.scenes.iter().any(|s| s.shots.iter().any(|sh| sh.video.is_some()))
        || film.final_video.is_some();
    if !any_video {
        warnings.push(
            "no shot/final videos found — Canvas will open with storyboard + characters only"
                .into(),
        );
    }
    for scene in &film.scenes {
        if scene.camera_tree.is_empty() && !scene.shots.is_empty() {
            warnings.push(format!(
                "scene `{}` missing camera_tree.json — camera continuity sidecar incomplete",
                scene.key
            ));
        }
        let missing_voice = film
            .characters
            .iter()
            .chain(scene.characters.iter())
            .filter(|c| c.is_visible && c.voice_profile.as_ref().is_none_or(|v| !v.is_usable()))
            .count();
        if missing_voice > 0 {
            warnings.push(format!(
                "scene `{}`: {missing_voice} visible character(s) lack VoiceProfile — \
cross-shot voice lock may weaken on Canvas regenerations",
                scene.key
            ));
        }
    }
}

fn collect_shot_updates_from_doc(doc: &serde_json::Value) -> Result<Vec<SyncShotUpdate>, AppError> {
    let nodes = doc
        .get("nodes")
        .and_then(|n| n.as_array())
        .ok_or_else(|| AppError::BadRequest("canvas doc missing nodes".into()))?;
    let mut out = Vec::new();

    for node in nodes {
        let meta = node.get("metadata").unwrap_or(&serde_json::Value::Null);
        let allo = meta.get("alloVimax").unwrap_or(&serde_json::Value::Null);

        // Only consider nodes with explicit alloVimax.kind === "shot_video"
        // that have been regenerated in Canvas (identified by versionOfNodeId).
        //
        // Note: videoStartFrameNodeId / videoEndFrameNodeId are set during initial
        // materialization, so they are NOT indicators of Canvas-side changes.
        if allo.get("kind").and_then(|v| v.as_str()) == Some("shot_video") {
            let version_of = meta.get("versionOfNodeId");

            // Skip if this is the original shot from Agent materialization
            // (no versionOfNodeId means it hasn't been regenerated in Canvas)
            if version_of.is_none() {
                continue;
            }

            let scene_key = allo
                .get("sceneKey")
                .and_then(|v| v.as_str())
                .unwrap_or("main")
                .to_string();
            let shot_idx = allo
                .get("shotIdx")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| AppError::BadRequest("shot node missing shotIdx".into()))?
                as i32;
            let media_id = extract_media_id(meta);
            if !media_id.is_empty() {
                out.push(SyncShotUpdate {
                    scene_key,
                    shot_idx,
                    media_id,
                });
            }
        }
    }

    Ok(out)
}

/// Extract media_id from node metadata, checking multiple possible sources.
fn extract_media_id(meta: &serde_json::Value) -> String {
    // Try storageKey first (preferred format: "resource:{media_id}")
    if let Some(storage) = meta.get("storageKey").and_then(|v| v.as_str()) {
        if let Some(rest) = storage.strip_prefix("resource:") {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }

    // Try content field (may contain URL or just media_id)
    if let Some(content) = meta.get("content").and_then(|v| v.as_str()) {
        if !content.is_empty() {
            // If it's a URL, extract the media_id from the path
            // URLs like "/api/video-canvas/media/{media_id}" or "{media_id}?..."
            if content.starts_with('/') {
                // Extract from URL path
                let media_id = content
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .split('#')
                    .next()
                    .unwrap_or("");
                if !media_id.is_empty() {
                    return media_id.to_string();
                }
            } else {
                // Might be just the media_id
                let media_id = content.split(|c| c == '?' || c == '#').next().unwrap_or("");
                if !media_id.is_empty() && media_id.len() >= 8 {
                    return media_id.to_string();
                }
            }
        }
    }

    // Try resourceReloadAvailable if it contains media info
    if let Some(reload) = meta.get("resourceReloadAvailable").and_then(|v| v.as_bool()) {
        if reload {
            // Try to get from storageKey or content (already checked above)
        }
    }

    String::new()
}

fn resolve_scene_dir(
    working_dir: &Path,
    session: &SessionRecord,
    scene_key: &str,
) -> Result<PathBuf, AppError> {
    let film = working_dir.join(session.workflow.artifact_root());
    let dir = if scene_key == "main" {
        film
    } else {
        film.join(scene_key)
    };
    if !dir.is_dir() {
        return Err(AppError::NotFound(format!(
            "scene dir missing: {}",
            dir.display()
        )));
    }
    Ok(dir)
}

async fn reconcat_scene(scene_dir: &Path) -> Result<PathBuf, String> {
    let shots_dir = scene_dir.join("shots");
    let mut idxs = Vec::new();
    let mut rd = tokio::fs::read_dir(&shots_dir)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
        if entry
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            if let Ok(idx) = entry.file_name().to_string_lossy().parse::<i32>() {
                idxs.push(idx);
            }
        }
    }
    idxs.sort_unstable();

    // 拼接成片前必须确认所有 shot 视频都已生成且有效。
    // 通过 shot_descriptions.json（如果存在）锁定"应该有几个 shot"，
    // 没产物文件则保守地要求 shots/ 下每一个 idx 子目录都产出可用的 video.mp4，
    // 缺失或损坏的 shot 不允许拼成成片。
    let planned = read_planned_shots(scene_dir).await.unwrap_or_default();
    let expected_shot_idxs: Vec<i32> = planned.iter().map(|(idx, _)| *idx).collect();
    let target_idxs: &[i32] = if expected_shot_idxs.is_empty() {
        &idxs
    } else {
        &expected_shot_idxs
    };
    if target_idxs.is_empty() {
        return Err("no shots defined for scene".into());
    }
    let mut missing: Vec<i32> = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::with_capacity(target_idxs.len());
    for idx in target_idxs {
        let video = shots_dir.join(idx.to_string()).join("video.mp4");
        if media_local::is_usable_video_file(&video) {
            paths.push(video);
        } else {
            missing.push(*idx);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "skip concat: scene `{scene}` has {missing_count} incomplete shot(s) [{missing_list}]",
            scene = scene_dir.display(),
            missing_count = missing.len(),
            missing_list = missing
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if paths.len() < 2 {
        return Err("skip concat: fewer than 2 usable shots".into());
    }
    let out = scene_dir.join("final_video.mp4");
    // Reproduce the renderer's splice shape: shots that share a camera kept
    // rolling (their head replays the previous ending and must be trimmed), a
    // camera change is a real cut. Without a planner artifact we cannot tell
    // them apart, so `cams` stays empty and nothing is trimmed.
    let cams: Vec<i32> = if planned.len() == paths.len() {
        planned.iter().filter_map(|(_, cam)| *cam).collect()
    } else {
        Vec::new()
    };
    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
    let clips = media_local::ConcatClip::scene(&refs, &cams, scene_opening_seam(scene_dir).await);
    media_local::concat_videos(&clips, &out)
        .await
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Read `(idx, cam_idx)` per shot from `shot_descriptions.json` when present, in
/// timeline order, so the canonical shot list comes from the planner (not from
/// whatever happens to be on disk).
async fn read_planned_shots(scene_dir: &Path) -> Result<Vec<(i32, Option<i32>)>, String> {
    let path = scene_dir.join("shot_descriptions.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let arr = parsed
        .as_array()
        .ok_or_else(|| "shot_descriptions.json is not an array".to_string())?;
    let mut shots: Vec<(i32, Option<i32>)> = arr
        .iter()
        .filter_map(|s| {
            let idx = s.get("idx").and_then(|v| v.as_i64())? as i32;
            let cam = s.get("cam_idx").and_then(|v| v.as_i64()).map(|n| n as i32);
            Some((idx, cam))
        })
        .collect();
    shots.sort_unstable_by_key(|(idx, _)| *idx);
    shots.dedup_by_key(|(idx, _)| *idx);
    Ok(shots)
}

/// How this scene joins the film in front of it.
///
/// Only the film's first scene starts from silence; a later scene opens
/// mid-soundtrack, so fading its first shot up would dip the film at every
/// scene boundary. A scene dir that is not named `scene_*` is the film root
/// (single-scene render), i.e. the opening.
async fn scene_opening_seam(scene_dir: &Path) -> media_local::SpliceSeam {
    let Some(name) = scene_dir.file_name().and_then(|s| s.to_str()) else {
        return media_local::SpliceSeam::Cut;
    };
    if !name.starts_with("scene_") {
        return media_local::SpliceSeam::Cut;
    }
    let Some(parent) = scene_dir.parent() else {
        return media_local::SpliceSeam::Cut;
    };
    let earlier = ordered_scene_names(parent)
        .await
        .into_iter()
        .any(|other| scene_sort_key(&other) < scene_sort_key(name));
    if earlier {
        media_local::SpliceSeam::MatchCut
    } else {
        media_local::SpliceSeam::Cut
    }
}

/// `scene_*` child dirs of `film`, in timeline order.
///
/// Sorted on the numeric suffix: a plain string sort puts `scene_10` before
/// `scene_2` and would splice a long film out of order.
async fn ordered_scene_names(film: &Path) -> Vec<String> {
    let Ok(mut rd) = tokio::fs::read_dir(film).await else {
        return Vec::new();
    };
    let mut names = Vec::new();
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("scene_")
            && entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
        {
            names.push(name);
        }
    }
    names.sort_by_key(|n| scene_sort_key(n));
    names
}

/// Numeric-then-lexical order key for a `scene_*` dir name.
fn scene_sort_key(name: &str) -> (u64, String) {
    let n = name
        .strip_prefix("scene_")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    (n, name.to_string())
}

async fn reconcat_film(
    working_dir: &Path,
    session: &SessionRecord,
) -> Result<Option<String>, String> {
    let film = working_dir.join(session.workflow.artifact_root());
    // Collect scene finals in order when multi-scene; else use film/script final.
    let mut scene_finals = Vec::new();
    let scene_names = ordered_scene_names(&film).await;
    for name in &scene_names {
        let p = film.join(name).join("final_video.mp4");
        if media_local::is_usable_video_file(&p) {
            scene_finals.push(p);
        }
    }

    let out = film.join("final_video.mp4");
    // Multi-scene: 每个 scene final 都已就绪才允许拼成成片；任何缺失的 scene final
    // 视为未完成，跳过 film 级别的拼接，避免半成片。
    if scene_names.len() >= 2 {
        if scene_finals.len() < scene_names.len() {
            return Err(format!(
                "skip film concat: {}/{} scene finals are usable",
                scene_finals.len(),
                scene_names.len()
            ));
        }
        // Scene N+1's opening shot match-cuts from scene N's tail frame, so a
        // rebuilt film keeps the same seam treatment as the original render.
        let refs: Vec<&Path> = scene_finals.iter().map(|p| p.as_path()).collect();
        media_local::concat_videos(&media_local::ConcatClip::film(&refs), &out)
            .await
            .map_err(|e| e.to_string())?;
    } else if film.join("shots").is_dir() {
        // script2video / single scene at film root
        reconcat_scene(&film).await?;
    } else if scene_finals.len() == 1 {
        // 单 scene 且 final_video 不存在：从唯一 scene final 拷贝。
        if !media_local::is_usable_video_file(&out) {
            tokio::fs::copy(&scene_finals[0], &out)
                .await
                .map_err(|e| e.to_string())?;
        }
    } else if !out.is_file() {
        return Ok(None);
    }

    let rel = out
        .strip_prefix(working_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| out.to_string_lossy().replace('\\', "/"));
    Ok(Some(rel))
}

fn map_vimax(e: nomi_vimax::VimaxError) -> AppError {
    match e {
        nomi_vimax::VimaxError::SessionNotFound(id) => AppError::NotFound(format!("session {id}")),
        nomi_vimax::VimaxError::InvalidParams(m) => AppError::BadRequest(m),
        other => AppError::Internal(other.to_string()),
    }
}

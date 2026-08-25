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
    if updates.is_empty() {
        return Err(AppError::BadRequest(
            "no shot updates found — regenerate a shot in Canvas first, or pass shots[]".into(),
        ));
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
                Err(e) => warnings.push(format!("scene `{scene_key}` concat failed: {e}")),
            }
        }
        // Film-level concat for multi-scene or single scene final.
        match reconcat_film(&working_dir, &session).await {
            Ok(Some(rel)) => {
                final_video = Some(rel);
                vimax
                    .set_session_final_video(session_id, final_video.clone())?;
            }
            Ok(None) => {}
            Err(e) => warnings.push(format!("film concat failed: {e}")),
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
        if allo.get("kind").and_then(|v| v.as_str()) != Some("shot_video") {
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
        let storage = meta
            .get("storageKey")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let media_id = if let Some(rest) = storage.strip_prefix("resource:") {
            rest.to_string()
        } else if let Some(content) = meta.get("content").and_then(|v| v.as_str()) {
            content
                .rsplit('/')
                .next()
                .unwrap_or("")
                .split('?')
                .next()
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        if media_id.is_empty() {
            continue;
        }
        out.push(SyncShotUpdate {
            scene_key,
            shot_idx,
            media_id,
        });
    }
    Ok(out)
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
    let paths: Vec<PathBuf> = idxs
        .iter()
        .map(|i| shots_dir.join(i.to_string()).join("video.mp4"))
        .filter(|p| p.is_file())
        .collect();
    if paths.len() < 1 {
        return Err("no shot videos to concat".into());
    }
    let out = scene_dir.join("final_video.mp4");
    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
    media_local::concat_videos(&refs, &out)
        .await
        .map_err(|e| e.to_string())?;
    Ok(out)
}

async fn reconcat_film(
    working_dir: &Path,
    session: &SessionRecord,
) -> Result<Option<String>, String> {
    let film = working_dir.join(session.workflow.artifact_root());
    // Collect scene finals in order when multi-scene; else use film/script final.
    let mut scene_finals = Vec::new();
    let mut rd = tokio::fs::read_dir(&film)
        .await
        .map_err(|e| e.to_string())?;
    let mut scene_names = Vec::new();
    while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("scene_")
            && entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
        {
            scene_names.push(name);
        }
    }
    scene_names.sort();
    for name in &scene_names {
        let p = film.join(name).join("final_video.mp4");
        if p.is_file() {
            scene_finals.push(p);
        }
    }

    let out = film.join("final_video.mp4");
    if scene_finals.len() >= 2 {
        let refs: Vec<&Path> = scene_finals.iter().map(|p| p.as_path()).collect();
        media_local::concat_videos(&refs, &out)
            .await
            .map_err(|e| e.to_string())?;
    } else if film.join("shots").is_dir() {
        // script2video / single scene at film root
        reconcat_scene(&film).await?;
    } else if scene_finals.len() == 1 {
        tokio::fs::copy(&scene_finals[0], &out)
            .await
            .map_err(|e| e.to_string())?;
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

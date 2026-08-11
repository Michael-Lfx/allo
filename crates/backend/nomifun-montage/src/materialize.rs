//! Materialize a Montage project into a Canvas project (and sync shot media back).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_montage::creative::{CreativeFilm, CreativeMediaKind, CreativeMediaRef};
use nomifun_canvas::CanvasService;
use nomifun_common::AppError;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

use crate::service::MontageApiService;

pub type MediaIdMap = HashMap<String, String>;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MaterializeToCanvasResult {
    pub project_id: String,
    pub title: String,
    pub montage_project_id: String,
    pub node_count: u32,
    pub media_count: u32,
    pub scene_count: u32,
    pub shot_count: u32,
    pub warnings: Vec<String>,
    /// True when an existing canvas for this montage project was reopened.
    pub reused: bool,
}

#[derive(Debug, Deserialize)]
pub struct SyncFromCanvasRequest {
    pub project_id: String,
    /// When empty, sync every shot video node that has `alloMontage.kind == shot_video`.
    #[serde(default)]
    pub shots: Vec<SyncShotUpdate>,
}

#[derive(Debug, Deserialize)]
pub struct SyncShotUpdate {
    pub scene_key: String,
    pub shot_idx: i64,
    pub media_id: String,
    /// Relative path inside the montage project (from CreativeFilm). Optional when
    /// the canvas node carries `alloMontage.relPath`.
    #[serde(default)]
    pub rel_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncFromCanvasResult {
    pub montage_project_id: String,
    pub updated_shots: u32,
    pub warnings: Vec<String>,
}

pub async fn materialize_project_to_canvas(
    montage: &MontageApiService,
    canvas: &Arc<CanvasService>,
    montage_project_id: &str,
) -> Result<MaterializeToCanvasResult, AppError> {
    let detail = montage.get_project(montage_project_id).await?;

    if let Some(existing) = canvas
        .find_project_for_montage_project(montage_project_id)
        .await?
    {
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
            montage_project_id = %montage_project_id,
            project_id = %existing.project_id,
            "reusing existing canvas for montage project"
        );
        return Ok(MaterializeToCanvasResult {
            project_id: existing.project_id,
            title: existing.title,
            montage_project_id: detail.record.id,
            node_count: existing.node_count,
            media_count: 0,
            scene_count,
            shot_count,
            warnings: vec![],
            reused: true,
        });
    }

    let film = montage.scan_creative_film(montage_project_id).await?;
    let mut warnings = Vec::new();
    if film.total_shots() == 0 && film.final_video.is_none() {
        warnings.push(
            "no scenes/shots yet — Canvas will open with an empty board until the pipeline produces media"
                .into(),
        );
    }

    let media_ids = ingest_film_media(canvas, &film, &mut warnings).await?;
    let doc = build_canvas_document(&film, &media_ids);

    let title = format!("{}（Canvas）", film.title);
    let meta = canvas
        .create_project_for_montage_project(montage_project_id, Some(title))
        .await?;
    let meta = canvas.put_doc(&meta.project_id, doc).await?;
    let meta = canvas
        .set_montage_project_on_project(&meta.project_id, montage_project_id)
        .await?;

    let shot_count = film.total_shots() as u32;
    info!(
        montage_project_id = %montage_project_id,
        project_id = %meta.project_id,
        media = media_ids.len(),
        shots = shot_count,
        "materialized montage project to canvas"
    );

    Ok(MaterializeToCanvasResult {
        project_id: meta.project_id,
        title: meta.title,
        montage_project_id: detail.record.id,
        node_count: meta.node_count,
        media_count: media_ids.len() as u32,
        scene_count: film.scenes.len() as u32,
        shot_count,
        warnings,
        reused: false,
    })
}

pub async fn sync_canvas_shots_to_project(
    montage: &MontageApiService,
    canvas: &Arc<CanvasService>,
    montage_project_id: &str,
    req: SyncFromCanvasRequest,
) -> Result<SyncFromCanvasResult, AppError> {
    let _ = montage.get_project(montage_project_id).await?;
    let root = montage.project_root(montage_project_id)?;

    let project = canvas.get_project(&req.project_id).await?;
    let allo = project
        .doc
        .get("alloCreative")
        .cloned()
        .unwrap_or(json!({}));
    let linked = allo
        .get("montageProjectId")
        .and_then(|v| v.as_str())
        .or_else(|| allo.get("sessionId").and_then(|v| v.as_str()))
        .unwrap_or("");
    if linked != montage_project_id {
        return Err(AppError::BadRequest(format!(
            "canvas project is linked to montage project `{linked}`, not `{montage_project_id}`"
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

    let film = montage.scan_creative_film(montage_project_id).await?;
    let mut warnings = Vec::new();
    let mut updated = 0_u32;

    for upd in &updates {
        let rel = upd
            .rel_path
            .clone()
            .or_else(|| resolve_rel_path_from_film(&film, &upd.scene_key, upd.shot_idx));
        let Some(rel) = rel.filter(|s| !s.trim().is_empty()) else {
            warnings.push(format!(
                "shot {}.{} has no rel_path — skipped",
                upd.scene_key, upd.shot_idx
            ));
            continue;
        };
        let dest = safe_join(&root, &rel)?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(format!("create media dir: {e}")))?;
        }
        let src = canvas.media_file_path(&upd.media_id).await?;
        tokio::fs::copy(&src, &dest)
            .await
            .map_err(|e| AppError::Internal(format!("write shot media: {e}")))?;
        updated += 1;
    }

    Ok(SyncFromCanvasResult {
        montage_project_id: montage_project_id.to_string(),
        updated_shots: updated,
        warnings,
    })
}

async fn ingest_film_media(
    canvas: &CanvasService,
    film: &CreativeFilm,
    warnings: &mut Vec<String>,
) -> Result<MediaIdMap, AppError> {
    let mut map = MediaIdMap::new();
    for media in film.all_media() {
        if map.contains_key(&media.rel_path) {
            continue;
        }
        if !media.abs_path.is_file() {
            warnings.push(format!("missing media: {}", media.rel_path));
            continue;
        }
        let (kind, mime, ext) = media_kind_meta(media);
        match canvas
            .ingest_local_file(
                &media.abs_path,
                kind,
                mime,
                ext,
                if media.title.is_empty() {
                    media.rel_path.clone()
                } else {
                    media.title.clone()
                },
            )
            .await
        {
            Ok(meta) => {
                map.insert(media.rel_path.clone(), meta.media_id);
            }
            Err(e) => warnings.push(format!("ingest {}: {e}", media.rel_path)),
        }
    }
    Ok(map)
}

fn media_kind_meta(media: &CreativeMediaRef) -> (&'static str, &'static str, &'static str) {
    let ext = media
        .abs_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match media.kind {
        CreativeMediaKind::Image => match ext.as_str() {
            "jpg" | "jpeg" => ("image", "image/jpeg", "jpg"),
            "webp" => ("image", "image/webp", "webp"),
            "gif" => ("image", "image/gif", "gif"),
            _ => ("image", "image/png", "png"),
        },
        CreativeMediaKind::Video => ("video", "video/mp4", "mp4"),
        CreativeMediaKind::Audio => ("audio", "audio/mpeg", "mp3"),
        CreativeMediaKind::File => ("file", "application/octet-stream", "bin"),
    }
}

fn build_canvas_document(film: &CreativeFilm, media_ids: &MediaIdMap) -> Value {
    let mut nodes = Vec::new();
    let mut y = 80.0_f64;
    for scene in &film.scenes {
        let mut x = 80.0_f64;
        for shot in &scene.shots {
            let Some(media) = &shot.media else {
                continue;
            };
            let Some(media_id) = media_ids.get(&media.rel_path) else {
                continue;
            };
            let kind = match media.kind {
                CreativeMediaKind::Video => "video",
                CreativeMediaKind::Image => "image",
                CreativeMediaKind::Audio => "audio",
                CreativeMediaKind::File => "file",
            };
            let node_kind = if media.kind == CreativeMediaKind::Video {
                "shot_video"
            } else {
                "shot_image"
            };
            let url = format!("/api/video-canvas/media/{media_id}");
            nodes.push(json!({
                "id": format!("shot-{}-{}", scene.key, shot.idx),
                "type": kind,
                "x": x,
                "y": y,
                "width": 320,
                "height": 180,
                "metadata": {
                    "title": format!("{} · shot {}", scene.title, shot.idx),
                    "storageKey": format!("resource:{media_id}"),
                    "content": url,
                    "alloMontage": {
                        "kind": node_kind,
                        "sceneKey": scene.key,
                        "shotIdx": shot.idx,
                        "relPath": media.rel_path,
                        "description": shot.description,
                    }
                }
            }));
            x += 360.0;
        }
        y += 220.0;
    }

    if let Some(final_v) = &film.final_video {
        if let Some(media_id) = media_ids.get(&final_v.rel_path) {
            nodes.push(json!({
                "id": "final-video",
                "type": "video",
                "x": 80,
                "y": y,
                "width": 480,
                "height": 270,
                "metadata": {
                    "title": "Final Cut",
                    "storageKey": format!("resource:{media_id}"),
                    "content": format!("/api/video-canvas/media/{media_id}"),
                    "alloMontage": {
                        "kind": "final_video",
                        "relPath": final_v.rel_path,
                    }
                }
            }));
        }
    }

    let film_sidecar = json!({
        "version": film.version,
        "title": film.title,
        "pipeline": film.pipeline,
        "stylePlaybook": film.style_playbook,
        "scenes": film.scenes.iter().map(|s| json!({
            "key": s.key,
            "title": s.title,
            "summary": s.summary,
            "shots": s.shots.iter().map(|sh| json!({
                "idx": sh.idx,
                "description": sh.description,
                "isMotion": sh.is_motion,
                "relPath": sh.media.as_ref().map(|m| &m.rel_path),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "finalVideoRelPath": film.final_video.as_ref().map(|m| &m.rel_path),
    });

    json!({
        "schemaVersion": 1,
        "nodes": nodes,
        "edges": [],
        "alloCreative": {
            "source": "nomifun-montage",
            "montageProjectId": film.project_id,
            "film": film_sidecar,
        }
    })
}

fn collect_shot_updates_from_doc(doc: &Value) -> Result<Vec<SyncShotUpdate>, AppError> {
    let nodes = doc
        .get("nodes")
        .and_then(|n| n.as_array())
        .ok_or_else(|| AppError::BadRequest("canvas doc missing nodes".into()))?;
    let mut out = Vec::new();
    for node in nodes {
        let meta = node.get("metadata").unwrap_or(&Value::Null);
        let allo = meta
            .get("alloMontage")
            .or_else(|| meta.get("alloVimax"))
            .unwrap_or(&Value::Null);
        let kind = allo.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "shot_video" && kind != "shot_image" {
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
            .ok_or_else(|| AppError::BadRequest("shot node missing shotIdx".into()))?;
        let rel_path = allo
            .get("relPath")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
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
            rel_path,
        });
    }
    Ok(out)
}

fn resolve_rel_path_from_film(
    film: &CreativeFilm,
    scene_key: &str,
    shot_idx: i64,
) -> Option<String> {
    film.scenes
        .iter()
        .find(|s| s.key == scene_key)?
        .shots
        .iter()
        .find(|s| s.idx == shot_idx)?
        .media
        .as_ref()
        .map(|m| m.rel_path.clone())
}

fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let rel = rel.replace('\\', "/");
    if rel.contains("..") || Path::new(&rel).is_absolute() {
        return Err(AppError::BadRequest(format!("unsafe rel_path: {rel}")));
    }
    Ok(root.join(rel))
}

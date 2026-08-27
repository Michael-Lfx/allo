//! Project a [`CreativeFilm`] into an open-ai-canvas–compatible document JSON.
//!
//! Quality rules:
//! - Full `camera_tree` / voice profiles / shot continuity frames are preserved in
//!   `alloCreative` (and per-node `alloVimax` sidecar).
//! - Storyboard rows carry Seedance-ready prompts including FIXED SPEAKER VOICE.
//! - Media URLs use `/api/video-canvas/media/{id}` + `resource:{id}` storage keys.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::domain::{Camera, ShotBriefDescription, ShotDescription};

use super::ir::{
    CreativeCharacter, CreativeFilm, CreativeMediaFile, CreativeMediaKind, CreativeScene,
    CreativeWorldKind,
};

/// Ingested canvas media reference: media_id plus lookup hints so the frontend
/// can skip HEAD probes when opening the materialized canvas.
#[derive(Debug, Clone)]
pub struct IngestedMedia {
    pub media_id: String,
    pub bytes: u64,
    pub mime: String,
}

/// Map from media `rel_path` → ingested canvas media after materialize.
pub type MediaIdMap = HashMap<String, IngestedMedia>;

pub fn build_canvas_document(film: &CreativeFilm, media_ids: &MediaIdMap) -> Value {
    let mut nodes: Vec<Value> = Vec::new();
    let mut connections: Vec<Value> = Vec::new();

    // ── Layout columns (mirrors Agent multi-ref pipeline left→right) ─────────
    // style | cast | env/prop | storyboard | continuity frames | shot videos | final
    const X_STYLE: f64 = 40.0;
    const X_CAST: f64 = 40.0;
    const X_WORLD: f64 = 340.0;
    const X_SCRIPT: f64 = 720.0;
    const X_FRAME: f64 = 1220.0;
    const X_SHOT: f64 = 1520.0;
    const X_FINAL: f64 = 1880.0;

    let style_id = "vimax-style".to_string();
    nodes.push(text_node(
        &style_id,
        "视觉风格 / 需求",
        X_STYLE,
        40.0,
        &compose_brief(film),
        Some("styleboard"),
    ));

    let cast = if film.characters.is_empty() {
        film.scenes
            .first()
            .map(|s| s.characters.as_slice())
            .unwrap_or(&[])
    } else {
        film.characters.as_slice()
    };

    // ── Cast column ─────────────────────────────────────────────────────────
    let mut char_node_ids: HashMap<i32, String> = HashMap::new();
    let mut y_char = 220.0_f64;
    for ch in cast {
        let node_id = format!("vimax-char-{}", ch.idx);
        let portrait = preferred_portrait(ch);
        let (content, storage_key, status) = media_content(portrait, media_ids);
        let voice_clause = ch.seedance_voice_clause();
        let mut meta = json!({
            "status": status,
            "workflowKind": "character",
            "workflowTitle": "角色定妆",
            "characterName": ch.identifier_in_scene,
            "characterPrompt": ch.static_features,
            "characterDefinition": {
                "idx": ch.idx,
                "identifier_in_scene": ch.identifier_in_scene,
                "is_visible": ch.is_visible,
                "static_features": ch.static_features,
                "dynamic_features": ch.dynamic_features,
                "voice_profile": ch.voice_profile,
            },
            "assetCategory": "character",
            "alloVimax": {
                "kind": "character",
                "sessionId": film.session_id,
                "characterIdx": ch.idx,
                "voiceClause": voice_clause,
            }
        });
        if let Some(url) = &content {
            meta["content"] = json!(url);
            meta["characterCoverUrl"] = json!(url);
        }
        if let Some((sk, entry)) = &storage_key {
            meta["storageKey"] = json!(sk);
            meta["mimeType"] = json!(entry.mime);
            meta["bytes"] = json!(entry.bytes);
        }
        if let Some(vp) = &ch.voice_profile {
            meta["characterVoiceProfile"] = json!({
                "name": ch.identifier_in_scene,
                "provider": "vimax",
                "language": "zh",
                "timbre": vp.timbre,
            });
            meta["characterVoiceInstructions"] = json!(voice_clause);
            meta["characterVoiceStatus"] = json!("ready");
        }
        if let Some(voice_media) = ch.portraits.get("voice_ref") {
            if let Some(entry) = media_ids.get(&voice_media.rel_path) {
                meta["sampleResourceId"] = json!(entry.media_id);
            }
        }
        nodes.push(json!({
            "id": node_id,
            "type": if content.is_some() { "image" } else { "text" },
            "title": format!("角色 · {}", ch.identifier_in_scene),
            "position": { "x": X_CAST, "y": y_char },
            "width": 240.0,
            "height": if content.is_some() { 260.0 } else { 180.0 },
            "metadata": meta,
        }));
        connections.push(conn(
            &format!("conn-style-char-{}", ch.idx),
            &style_id,
            &node_id,
            None,
        ));

        let mut view_y = y_char;
        for (view, media) in &ch.portraits {
            if preferred_portrait(ch).is_some_and(|p| p.rel_path == media.rel_path) {
                continue;
            }
            let vid = format!("vimax-char-{}-{}", ch.idx, sanitize_id(view));
            let (_, Some((_, entry)), st) = media_content(Some(media), media_ids) else {
                continue;
            };
            let node = if media.kind == CreativeMediaKind::Audio {
                audio_node(
                    &vid,
                    &format!("{} · 音色参考", ch.identifier_in_scene),
                    X_CAST + 260.0,
                    view_y,
                    entry,
                    st,
                    json!({
                        "workflowKind": "character",
                        "characterName": ch.identifier_in_scene,
                        "characterView": view,
                        "assetCategory": "character_voice",
                        "alloVimax": {
                            "kind": "voice_ref",
                            "sessionId": film.session_id,
                            "characterIdx": ch.idx,
                            "view": view,
                        }
                    }),
                )
            } else {
                image_node(
                    &vid,
                    &format!("{} · {view}", ch.identifier_in_scene),
                    X_CAST + 260.0,
                    view_y,
                    entry,
                    st,
                    json!({
                        "workflowKind": "character",
                        "characterName": ch.identifier_in_scene,
                        "characterView": view,
                        "assetCategory": "character",
                        "alloVimax": {
                            "kind": "portrait",
                            "sessionId": film.session_id,
                            "characterIdx": ch.idx,
                            "view": view,
                        }
                    }),
                )
            };
            nodes.push(node);
            connections.push(conn(
                &format!("conn-char-{}-{view}", ch.idx),
                &node_id,
                &vid,
                None,
            ));
            view_y += 200.0;
        }
        char_node_ids.insert(ch.idx, node_id);
        y_char += 300.0;
    }

    // ── World assets (env / prop) — Seedance multi-ref plates ────────────────
    let mut world_node_ids: Vec<String> = Vec::new();
    let mut y_world = 220.0_f64;
    for (i, wa) in film.world_assets.iter().enumerate() {
        let node_id = format!(
            "vimax-world-{}-{}",
            wa.kind.as_str(),
            sanitize_id(&wa.key)
        );
        let Some(entry) = media_ids.get(&wa.media.rel_path) else {
            continue;
        };
        let title = match wa.kind {
            CreativeWorldKind::Environment => format!("环境 · {}", wa.key),
            CreativeWorldKind::Prop => format!("道具 · {}", wa.key),
        };
        let category = match wa.kind {
            CreativeWorldKind::Environment => "environment",
            CreativeWorldKind::Prop => "prop",
        };
        nodes.push(image_node(
            &node_id,
            &title,
            X_WORLD,
            y_world,
            entry,
            "success",
            json!({
                "status": "success",
                "workflowKind": wa.kind.workflow_kind(),
                "workflowTitle": title,
                "workflowDescription": wa.description,
                "prompt": wa.description,
                "assetCategory": category,
                "alloVimax": {
                    "kind": wa.kind.as_str(),
                    "sessionId": film.session_id,
                    "assetKey": wa.key,
                    "artifactRel": wa.media.rel_path,
                }
            }),
        ));
        connections.push(conn(
            &format!("conn-style-world-{i}"),
            &style_id,
            &node_id,
            None,
        ));
        world_node_ids.push(node_id);
        y_world += 240.0;
    }

    // ── Scenes ──────────────────────────────────────────────────────────────
    let mut scene_y = 40.0_f64;
    let mut global_shot_offset = 0_i32;
    let mut all_shot_video_ids: Vec<String> = Vec::new();
    for scene in &film.scenes {
        let built = build_scene_block(
            film,
            scene,
            media_ids,
            &char_node_ids,
            &world_node_ids,
            &style_id,
            cast,
            scene_y,
            global_shot_offset,
            X_SCRIPT,
            X_FRAME,
            X_SHOT,
        );
        global_shot_offset += scene.shots.len() as i32;
        scene_y = built.next_y;
        all_shot_video_ids.extend(built.shot_video_ids);
        nodes.extend(built.nodes);
        connections.extend(built.connections);
    }

    // ── Film final ← all shot videos ────────────────────────────────────────
    if let Some(final_v) = &film.final_video {
        if let Some(entry) = media_ids.get(&final_v.rel_path) {
            nodes.push(video_node(
                "vimax-final",
                "成片 · Final",
                X_FINAL,
                40.0,
                entry,
                json!({
                    "status": "success",
                    "workflowKind": "final",
                    "workflowTitle": "成片",
                    "videoEditOperation": "concat",
                    "alloVimax": {
                        "kind": "final",
                        "sessionId": film.session_id,
                        "artifactRel": final_v.rel_path,
                    }
                }),
            ));
            for (i, shot_id) in all_shot_video_ids.iter().enumerate() {
                connections.push(conn(
                    &format!("conn-shot-final-{i}"),
                    shot_id,
                    "vimax-final",
                    None,
                ));
            }
            connections.push(conn("conn-style-final", &style_id, "vimax-final", None));
        }
    }

    if let Some(cover) = &film.cover {
        if let Some(entry) = media_ids.get(&cover.rel_path) {
            nodes.push(image_node(
                "vimax-cover",
                "封面",
                X_FINAL,
                300.0,
                entry,
                "success",
                json!({
                    "workflowKind": "styleboard",
                    "alloVimax": {
                        "kind": "cover",
                        "sessionId": film.session_id,
                        "artifactRel": cover.rel_path,
                    }
                }),
            ));
            connections.push(conn("conn-style-cover", &style_id, "vimax-cover", None));
            if film.final_video.is_some() {
                connections.push(conn("conn-final-cover", "vimax-final", "vimax-cover", None));
            }
        }
    }

    let allo_creative = build_allo_creative_sidecar(film, media_ids);

    json!({
        "schema": 1,
        "title": format!("{}（Canvas）", film.title),
        "nodes": nodes,
        "connections": connections,
        "viewport": { "x": 0, "y": 0, "k": 0.45 },
        "backgroundMode": "lines",
        "alloCreative": allo_creative,
    })
}

struct SceneBlock {
    nodes: Vec<Value>,
    connections: Vec<Value>,
    shot_video_ids: Vec<String>,
    next_y: f64,
}

fn build_scene_block(
    film: &CreativeFilm,
    scene: &CreativeScene,
    media_ids: &MediaIdMap,
    char_node_ids: &HashMap<i32, String>,
    world_node_ids: &[String],
    style_id: &str,
    film_cast: &[CreativeCharacter],
    origin_y: f64,
    shot_number_offset: i32,
    x_script: f64,
    x_frame: f64,
    x_shot: f64,
) -> SceneBlock {
    let mut nodes = Vec::new();
    let mut connections = Vec::new();
    let script_id = format!("vimax-script-{}", scene.key);
    let cast_for_scene: Vec<&CreativeCharacter> = if scene.characters.is_empty() {
        film_cast.iter().collect()
    } else {
        scene.characters.iter().collect()
    };

    let default_duration = estimate_shot_duration(film, scene);
    let mut rows = Vec::new();
    let shot_desc_by_idx: HashMap<i32, &ShotDescription> = scene
        .shot_descriptions
        .iter()
        .map(|s| (s.idx, s))
        .collect();
    let brief_by_idx: HashMap<i32, &ShotBriefDescription> =
        scene.storyboard.iter().map(|s| (s.idx, s)).collect();
    let camera_by_idx: HashMap<i32, &Camera> =
        scene.camera_tree.iter().map(|c| (c.idx, c)).collect();

    let shot_order: Vec<i32> = if !scene.shot_descriptions.is_empty() {
        scene.shot_descriptions.iter().map(|s| s.idx).collect()
    } else if !scene.storyboard.is_empty() {
        scene.storyboard.iter().map(|s| s.idx).collect()
    } else {
        scene.shots.iter().map(|s| s.idx).collect()
    };

    // (video_id, shot_idx, char_idxs, ff_id, continuity_in_id, vlf_id)
    let mut video_nodes_meta: Vec<(
        String,
        i32,
        Vec<i32>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = Vec::new();
    let mut prev_vlf_id: Option<String> = None;
    let mut shot_video_ids: Vec<String> = Vec::new();

    for (order, &shot_idx) in shot_order.iter().enumerate() {
        let brief = brief_by_idx.get(&shot_idx).copied();
        let desc = shot_desc_by_idx.get(&shot_idx).copied();
        let shot_media = scene.shots.iter().find(|s| s.idx == shot_idx);
        let cam_idx = desc
            .map(|d| d.cam_idx)
            .or_else(|| brief.map(|b| b.cam_idx))
            .or_else(|| shot_media.map(|s| s.cam_idx))
            .unwrap_or(shot_idx);
        let camera = camera_by_idx.get(&cam_idx).copied();

        let row_id = format!("vimax-row-{}-{shot_idx}", scene.key);
        let video_id = format!("vimax-shot-{}-{shot_idx}", scene.key);
        let ff_id = format!("vimax-ff-{}-{shot_idx}", scene.key);
        let vlf_id = format!("vimax-vlf-{}-{shot_idx}", scene.key);
        let y_row = origin_y + 80.0 + order as f64 * 300.0;

        let visual = desc
            .map(|d| d.visual_desc.as_str())
            .or_else(|| brief.map(|b| b.visual_desc.as_str()))
            .unwrap_or("");
        let motion = desc.map(|d| d.motion_desc.as_str()).unwrap_or("");
        let audio = desc
            .and_then(|d| d.audio_desc.as_deref())
            .or_else(|| brief.and_then(|b| b.audio_desc.as_deref()))
            .unwrap_or("");
        let ff_desc = desc.map(|d| d.ff_desc.as_str()).unwrap_or("");
        let lf_desc = desc.map(|d| d.lf_desc.as_str()).unwrap_or("");

        let char_idxs: Vec<i32> = desc
            .map(|d| {
                let mut v = d.ff_vis_char_idxs.clone();
                for i in &d.lf_vis_char_idxs {
                    if !v.contains(i) {
                        v.push(*i);
                    }
                }
                v
            })
            .unwrap_or_default();

        let row_chars: Vec<Value> = char_idxs
            .iter()
            .filter_map(|idx| cast_for_scene.iter().find(|c| c.idx == *idx))
            .map(|c| {
                json!({
                    "characterName": c.identifier_in_scene,
                    "characterDescription": c.static_features,
                    "characterImageNodeId": char_node_ids.get(&c.idx),
                })
            })
            .collect();

        let voice_clauses: Vec<String> = if char_idxs.iter().any(|idx| {
            cast_for_scene
                .iter()
                .find(|c| c.idx == *idx)
                .is_some_and(|c| c.portraits.contains_key("voice_ref"))
        }) {
            Vec::new()
        } else {
            char_idxs
                .iter()
                .filter_map(|idx| cast_for_scene.iter().find(|c| c.idx == *idx))
                .filter_map(|c| c.seedance_voice_clause())
                .collect()
        };

        let camera_label = format_camera_label(cam_idx, camera);
        let image_prompt = compose_image_prompt(visual, ff_desc, &cast_for_scene, &char_idxs);
        let use_voice_ref = char_idxs.iter().any(|idx| {
            cast_for_scene
                .iter()
                .find(|c| c.idx == *idx)
                .is_some_and(|c| c.portraits.contains_key("voice_ref"))
        });
        let video_prompt = compose_video_prompt(
            visual,
            motion,
            audio,
            ff_desc,
            lf_desc,
            &voice_clauses,
            film,
            use_voice_ref,
        );

        let mut reference_node_ids: Vec<String> = char_idxs
            .iter()
            .filter_map(|idx| char_node_ids.get(idx).cloned())
            .collect();
        reference_node_ids.extend(world_node_ids.iter().cloned());

        let mut row = json!({
            "id": row_id,
            "shotNumber": shot_number_offset + order as i32 + 1,
            "durationSeconds": default_duration,
            "plotDescription": visual,
            "dialogue": audio,
            "characters": row_chars,
            "narrativeIntent": desc.map(|d| d.variation_reason.clone()).unwrap_or_default(),
            "viewerPOV": "",
            "performanceBlocking": "",
            "shotSize": "",
            "emotion": "",
            "lightingAndAtmosphere": "",
            "audioEffects": audio,
            "camera": camera_label,
            "motion": motion,
            "timeBeats": "",
            "imageGenerationPrompt": image_prompt,
            "videoMotionPrompt": video_prompt,
            "mustHave": [],
            "optionalDetails": [],
            "continuityOut": lf_desc,
            "negativePrompt": "",
            "referenceNodeIds": reference_node_ids,
            "status": if shot_media.and_then(|s| s.video.as_ref()).is_some() { "success" } else { "idle" },
        });

        let continuity_in = prev_vlf_id.clone();
        let mut ff_node_id = None;
        let mut this_vlf_id = None;

        if let Some(shot) = shot_media {
            // Optional legacy first_frame (revise path); default Agent render skips it.
            if let Some(ff) = &shot.first_frame {
                if let Some(entry) = media_ids.get(&ff.rel_path) {
                    nodes.push(image_node(
                        &ff_id,
                        &format!("S{shot_idx} 首帧"),
                        x_frame,
                        y_row,
                        entry,
                        "success",
                        json!({
                            "status": "success",
                            "workflowKind": "shot",
                            "shotIndex": shot_idx,
                            "alloVimax": {
                                "kind": "first_frame",
                                "sessionId": film.session_id,
                                "sceneKey": scene.key,
                                "shotIdx": shot_idx,
                                "artifactRel": ff.rel_path,
                            }
                        }),
                    ));
                    ff_node_id = Some(ff_id.clone());
                    row["imageNodeId"] = json!(ff_id);
                }
            }

            // video_last_frame — primary continuity asset for next Seedance R2V shot
            let vlf_src = shot
                .video_last_frame
                .as_ref()
                .or(shot.last_frame.as_ref());
            if let Some(vlf) = vlf_src {
                if let Some(entry) = media_ids.get(&vlf.rel_path) {
                    nodes.push(image_node(
                        &vlf_id,
                        &format!("S{shot_idx} 连续末帧"),
                        x_frame,
                        y_row + 140.0,
                        entry,
                        "success",
                        json!({
                            "status": "success",
                            "workflowKind": "shot",
                            "shotIndex": shot_idx,
                            "alloVimax": {
                                "kind": "video_last_frame",
                                "sessionId": film.session_id,
                                "sceneKey": scene.key,
                                "shotIdx": shot_idx,
                                "artifactRel": vlf.rel_path,
                            }
                        }),
                    ));
                    this_vlf_id = Some(vlf_id.clone());
                }
            }

            if let Some(video) = &shot.video {
                if let Some(entry) = media_ids.get(&video.rel_path) {
                    let edit_op = if ff_node_id.is_some() || continuity_in.is_some() {
                        "image_to_video"
                    } else {
                        "text_to_video"
                    };
                    let mut vmeta = json!({
                        "status": "success",
                        "workflowKind": "shot",
                        "workflowTitle": format!("镜头 {shot_idx}"),
                        "shotIndex": shot_idx,
                        "sceneId": scene.key,
                        "prompt": video_prompt,
                        "videoEditOperation": edit_op,
                        "generateAudio": "true",
                        "seconds": default_duration.to_string(),
                        "size": film.aspect_ratio,
                        "vquality": film.resolution,
                        "model": film.video_model,
                        "alloVimax": {
                            "kind": "shot_video",
                            "sessionId": film.session_id,
                            "sceneKey": scene.key,
                            "shotIdx": shot_idx,
                            "camIdx": cam_idx,
                            "artifactRel": video.rel_path,
                            "voiceClauses": voice_clauses,
                            "ffDesc": ff_desc,
                            "lfDesc": lf_desc,
                            "motionDesc": motion,
                            "visualDesc": visual,
                            "characterIdxs": char_idxs,
                            "cameraTree": scene.camera_tree,
                            "worldAssetKeys": film.world_assets.iter().map(|w| w.key.clone()).collect::<Vec<_>>(),
                        }
                    });
                    if let Some(ref id) = ff_node_id {
                        vmeta["videoStartFrameNodeId"] = json!(id);
                    } else if let Some(ref id) = continuity_in {
                        // Agent default: prev video_last_frame is continuity Image 1
                        vmeta["videoStartFrameNodeId"] = json!(id);
                    }
                    if let Some(ref id) = this_vlf_id {
                        vmeta["videoEndFrameNodeId"] = json!(id);
                    }
                    nodes.push(video_node(
                        &video_id,
                        &format!("镜头 {shot_idx}"),
                        x_shot,
                        y_row,
                        entry,
                        vmeta,
                    ));
                    row["videoNodeId"] = json!(video_id);
                    shot_video_ids.push(video_id.clone());
                    video_nodes_meta.push((
                        video_id.clone(),
                        shot_idx,
                        char_idxs.clone(),
                        ff_node_id.clone(),
                        continuity_in.clone(),
                        this_vlf_id.clone(),
                    ));
                }
            }
        }

        prev_vlf_id = this_vlf_id;
        rows.push(row);
    }

    let mut storyboard_refs: Vec<String> = char_node_ids.values().cloned().collect();
    storyboard_refs.extend(world_node_ids.iter().cloned());

    let script_height = (180.0 + rows.len() as f64 * 72.0).clamp(280.0, 900.0);
    nodes.push(json!({
        "id": script_id,
        "type": "script",
        "title": format!("分镜 · {}", scene.title),
        "position": { "x": x_script, "y": origin_y },
        "width": 420.0,
        "height": script_height,
        "metadata": {
            "status": "success",
            "workflowKind": "storyboard",
            "workflowTitle": scene.title,
            "workflowDescription": format!(
                "{} 个镜头 · Agent {} · 多参考图 R2V（角色+环境+道具+连续末帧）",
                rows.len(),
                film.workflow.as_str()
            ),
            "chapterId": scene.key,
            "chapterTitle": scene.title,
            "content": scene.script,
            "storyboard": {
                "rows": rows,
                "visibleColumns": [
                    "shotNumber",
                    "durationSeconds",
                    "plotDescription",
                    "dialogue",
                    "camera",
                    "motion",
                    "imageGenerationPrompt",
                    "videoMotionPrompt",
                    "continuityOut"
                ],
                "referenceNodeIds": storyboard_refs,
            },
            "alloVimax": {
                "kind": "storyboard",
                "sessionId": film.session_id,
                "sceneKey": scene.key,
                "artifactRootRel": scene.artifact_root_rel,
                "cameraTree": scene.camera_tree,
            }
        }
    }));

    // Style / cast / world → storyboard (Agent bible feeding the plan)
    connections.push(conn(
        &format!("conn-style-script-{}", scene.key),
        style_id,
        &script_id,
        None,
    ));
    for (idx, char_id) in char_node_ids.iter() {
        connections.push(conn(
            &format!("conn-char-script-{}-{idx}", scene.key),
            char_id,
            &script_id,
            Some("storyboard:context"),
        ));
    }
    for (i, world_id) in world_node_ids.iter().enumerate() {
        connections.push(conn(
            &format!("conn-world-script-{}-{i}", scene.key),
            world_id,
            &script_id,
            Some("storyboard:context"),
        ));
    }

    // Per-shot wiring matching Seedance multi-ref order:
    // continuity_in + cast + world → video ← script row; video → video_last_frame
    for (video_id, shot_idx, char_idxs, ff_id, continuity_in, vlf_out) in &video_nodes_meta {
        let row_id = format!("vimax-row-{}-{shot_idx}", scene.key);
        connections.push(conn(
            &format!("conn-row-{video_id}"),
            &script_id,
            video_id,
            Some(&format!("row:{row_id}")),
        ));
        if let Some(ff) = ff_id {
            connections.push(conn(
                &format!("conn-ff-{video_id}"),
                ff,
                video_id,
                None,
            ));
        }
        if let Some(cont) = continuity_in {
            connections.push(conn(
                &format!("conn-cont-{video_id}"),
                cont,
                video_id,
                None,
            ));
        }
        for idx in char_idxs {
            if let Some(cid) = char_node_ids.get(idx) {
                connections.push(conn(
                    &format!("conn-cast-{video_id}-{idx}"),
                    cid,
                    video_id,
                    None,
                ));
            }
        }
        for (i, wid) in world_node_ids.iter().enumerate() {
            connections.push(conn(
                &format!("conn-world-{video_id}-{i}"),
                wid,
                video_id,
                None,
            ));
        }
        if let Some(vlf) = vlf_out {
            connections.push(conn(
                &format!("conn-vlf-out-{video_id}"),
                video_id,
                vlf,
                None,
            ));
        }
    }

    // Scene-level final
    if let Some(sf) = &scene.final_video {
        if film.final_video.as_ref().map(|f| f.rel_path.as_str()) != Some(sf.rel_path.as_str()) {
            if let Some(entry) = media_ids.get(&sf.rel_path) {
                let fid = format!("vimax-scene-final-{}", scene.key);
                nodes.push(video_node(
                    &fid,
                    &format!("{} 成片", scene.title),
                    x_shot + 360.0,
                    origin_y,
                    entry,
                    json!({
                        "status": "success",
                        "workflowKind": "final",
                        "videoEditOperation": "concat",
                        "alloVimax": {
                            "kind": "scene_final",
                            "sessionId": film.session_id,
                            "sceneKey": scene.key,
                            "artifactRel": sf.rel_path,
                        }
                    }),
                ));
                for (i, sid) in shot_video_ids.iter().enumerate() {
                    connections.push(conn(
                        &format!("conn-scene-final-{}-{i}", scene.key),
                        sid,
                        &fid,
                        None,
                    ));
                }
            }
        }
    }

    let next_y = origin_y + script_height.max(80.0 + shot_order.len() as f64 * 300.0) + 120.0;
    SceneBlock {
        nodes,
        connections,
        shot_video_ids,
        next_y,
    }
}

fn build_allo_creative_sidecar(film: &CreativeFilm, media_ids: &MediaIdMap) -> Value {
    // Serialize film but replace abs paths with media ids for portability inside canvas doc.
    let mut film_json = serde_json::to_value(film).unwrap_or_else(|_| json!({}));
    rewrite_media_paths(&mut film_json, media_ids);
    json!({
        "version": film.version,
        "source": "nomifun-vimax",
        "materializedAt": chrono::Utc::now().to_rfc3339(),
        "sessionId": film.session_id,
        "workflow": film.workflow.as_str(),
        "title": film.title,
        "style": film.style,
        "aspectRatio": film.aspect_ratio,
        "resolution": film.resolution,
        "fps": film.fps,
        "targetDurationSecs": film.target_duration_secs,
        "models": {
            "llm": film.llm_model,
            "image": film.image_model,
            "video": film.video_model,
        },
        "film": film_json,
        "writeBack": {
            "enabled": true,
            "sessionId": film.session_id,
            "policy": "explicit_sync",
        }
    })
}

fn rewrite_media_paths(value: &mut Value, media_ids: &MediaIdMap) {
    match value {
        Value::Object(map) => {
            if let (Some(Value::String(rel)), Some(entry)) = (
                map.get("rel_path").cloned(),
                map.get("rel_path")
                    .and_then(|v| v.as_str())
                    .and_then(|r| media_ids.get(r)),
            ) {
                let _ = rel;
                map.insert("media_id".into(), json!(entry.media_id));
                map.insert(
                    "url".into(),
                    json!(format!("/api/video-canvas/media/{}", entry.media_id)),
                );
                map.insert("bytes".into(), json!(entry.bytes));
                map.insert("mime".into(), json!(entry.mime));
                // Drop machine-local abs_path from the canvas sidecar.
                map.remove("abs_path");
            }
            for v in map.values_mut() {
                rewrite_media_paths(v, media_ids);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                rewrite_media_paths(v, media_ids);
            }
        }
        _ => {}
    }
}

fn compose_brief(film: &CreativeFilm) -> String {
    let mut parts = Vec::new();
    if !film.style.trim().is_empty() {
        parts.push(format!("风格：{}", film.style.trim()));
    }
    if !film.aspect_ratio.trim().is_empty() {
        parts.push(format!("画幅：{}", film.aspect_ratio.trim()));
    }
    if !film.resolution.trim().is_empty() {
        parts.push(format!("分辨率：{}", film.resolution.trim()));
    }
    if film.target_duration_secs > 0 {
        parts.push(format!("目标时长：{}s", film.target_duration_secs));
    }
    parts.push(format!(
        "来源：Agent {} · session {}",
        film.workflow.as_str(),
        film.session_id
    ));
    if !film.world_assets.is_empty() {
        let env_n = film
            .world_assets
            .iter()
            .filter(|w| w.kind == crate::creative::CreativeWorldKind::Environment)
            .count();
        let prop_n = film.world_assets.len() - env_n;
        parts.push(format!("世界资产：{env_n} 环境板 · {prop_n} 道具板"));
    }
    parts.push(
        "质量护栏：重生成视频时保留 FIXED SPEAKER VOICE、角色定妆、环境/道具参考与连续末帧连线；机位树见分镜 alloVimax.cameraTree。"
            .into(),
    );
    parts.join("\n")
}

fn compose_image_prompt(
    visual: &str,
    ff_desc: &str,
    cast: &[&CreativeCharacter],
    char_idxs: &[i32],
) -> String {
    let mut parts = Vec::new();
    if !ff_desc.trim().is_empty() {
        parts.push(ff_desc.trim().to_string());
    } else if !visual.trim().is_empty() {
        parts.push(visual.trim().to_string());
    }
    for idx in char_idxs {
        if let Some(c) = cast.iter().find(|c| c.idx == *idx) {
            let feats = c.static_features.trim();
            if !feats.is_empty() {
                parts.push(format!(
                    "Character <{}>: {feats}. Lock identity.",
                    c.identifier_in_scene
                ));
            }
        }
    }
    parts.join("\n")
}

fn compose_video_prompt(
    visual: &str,
    motion: &str,
    audio: &str,
    ff_desc: &str,
    lf_desc: &str,
    voice_clauses: &[String],
    film: &CreativeFilm,
    use_voice_audio_ref: bool,
) -> String {
    let mut parts = Vec::new();
    if !visual.trim().is_empty() {
        parts.push(visual.trim().to_string());
    }
    if !motion.trim().is_empty() {
        parts.push(format!("Motion: {}", motion.trim()));
    }
    if !ff_desc.trim().is_empty() {
        parts.push(format!("Starts on: {}", ff_desc.trim()));
    }
    if !lf_desc.trim().is_empty() {
        parts.push(format!("Ends on: {}", lf_desc.trim()));
    }
    if use_voice_audio_ref {
        parts.push(
            "REFERENCE AUDIO: match reference_audio for speaker timbre; no background music — only dialogue and essential on-screen foley."
                .into(),
        );
    } else if !audio.trim().is_empty() {
        parts.push(format!("Audio / dialogue: {}", audio.trim()));
    }
    if !use_voice_audio_ref {
        for clause in voice_clauses {
            if !parts.iter().any(|p| p.contains(clause)) {
                parts.push(clause.clone());
            }
        }
    }
    if !film.aspect_ratio.trim().is_empty() {
        parts.push(format!("Aspect ratio {}", film.aspect_ratio.trim()));
    }
    parts.join("\n")
}

fn format_camera_label(cam_idx: i32, camera: Option<&Camera>) -> String {
    let mut s = format!("cam #{cam_idx}");
    if let Some(c) = camera {
        if let Some(parent) = c.parent_cam_idx {
            s.push_str(&format!(" ← parent cam #{parent}"));
        }
        if let Some(ps) = c.parent_shot_idx {
            s.push_str(&format!(" (from shot {ps})"));
        }
        if let Some(reason) = c.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            s.push_str(&format!(" — {reason}"));
        }
    }
    s
}

fn estimate_shot_duration(film: &CreativeFilm, scene: &CreativeScene) -> u32 {
    let n = scene.shots.len().max(scene.storyboard.len()).max(1) as u32;
    if film.target_duration_secs > 0 {
        (film.target_duration_secs / n).clamp(4, 12)
    } else {
        5
    }
}

fn preferred_portrait(ch: &CreativeCharacter) -> Option<&CreativeMediaFile> {
    for key in ["cameo", "sheet", "front", "side", "back"] {
        if let Some(m) = ch.portraits.get(key) {
            if m.kind != CreativeMediaKind::Audio {
                return Some(m);
            }
        }
    }
    ch.portraits
        .values()
        .find(|m| m.kind != CreativeMediaKind::Audio)
}

fn media_content<'a>(
    media: Option<&CreativeMediaFile>,
    map: &'a MediaIdMap,
) -> (
    Option<String>,
    Option<(String, &'a IngestedMedia)>,
    &'static str,
) {
    match media.and_then(|m| map.get(&m.rel_path)) {
        Some(entry) => (
            Some(format!("/api/video-canvas/media/{}", entry.media_id)),
            Some((format!("resource:{}", entry.media_id), entry)),
            "success",
        ),
        None => (None, None, "idle"),
    }
}

fn text_node(
    id: &str,
    title: &str,
    x: f64,
    y: f64,
    content: &str,
    workflow: Option<&str>,
) -> Value {
    json!({
        "id": id,
        "type": "text",
        "title": title,
        "position": { "x": x, "y": y },
        "width": 280.0,
        "height": 160.0,
        "metadata": {
            "content": content,
            "status": "success",
            "workflowKind": workflow.unwrap_or("free"),
            "fontSize": 13,
        }
    })
}

fn image_node(
    id: &str,
    title: &str,
    x: f64,
    y: f64,
    entry: &IngestedMedia,
    status: &str,
    extra: Value,
) -> Value {
    let mut meta = extra;
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "content".into(),
            json!(format!("/api/video-canvas/media/{}", entry.media_id)),
        );
        obj.insert(
            "storageKey".into(),
            json!(format!("resource:{}", entry.media_id)),
        );
        obj.insert("mediaId".into(), json!(entry.media_id));
        obj.insert("mimeType".into(), json!(entry.mime));
        obj.insert("bytes".into(), json!(entry.bytes));
        obj.insert("status".into(), json!(status));
    }
    json!({
        "id": id,
        "type": "image",
        "title": title,
        "position": { "x": x, "y": y },
        "width": 220.0,
        "height": 200.0,
        "metadata": meta,
    })
}

fn audio_node(
    id: &str,
    title: &str,
    x: f64,
    y: f64,
    entry: &IngestedMedia,
    status: &str,
    extra: Value,
) -> Value {
    let mut meta = extra;
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "content".into(),
            json!(format!("/api/video-canvas/media/{}", entry.media_id)),
        );
        obj.insert(
            "storageKey".into(),
            json!(format!("resource:{}", entry.media_id)),
        );
        obj.insert("mediaId".into(), json!(entry.media_id));
        obj.insert("mimeType".into(), json!(entry.mime));
        obj.insert("bytes".into(), json!(entry.bytes));
        obj.insert("status".into(), json!(status));
    }
    json!({
        "id": id,
        "type": "audio",
        "title": title,
        "position": { "x": x, "y": y },
        "width": 220.0,
        "height": 120.0,
        "metadata": meta,
    })
}

fn video_node(
    id: &str,
    title: &str,
    x: f64,
    y: f64,
    entry: &IngestedMedia,
    mut meta: Value,
) -> Value {
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "content".into(),
            json!(format!("/api/video-canvas/media/{}", entry.media_id)),
        );
        obj.insert(
            "storageKey".into(),
            json!(format!("resource:{}", entry.media_id)),
        );
        obj.insert("mediaId".into(), json!(entry.media_id));
        obj.insert("mimeType".into(), json!(entry.mime));
        obj.insert("bytes".into(), json!(entry.bytes));
    }
    json!({
        "id": id,
        "type": "video",
        "title": title,
        "position": { "x": x, "y": y },
        "width": 280.0,
        "height": 220.0,
        "metadata": meta,
    })
}

fn conn(id: &str, from: &str, to: &str, from_handle: Option<&str>) -> Value {
    let mut c = json!({
        "id": id,
        "fromNodeId": from,
        "toNodeId": to,
    });
    if let Some(h) = from_handle {
        c["fromHandleId"] = json!(h);
    }
    c
}

fn sanitize_id(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii() {
                c
            } else {
                '_'
            }
        })
        .collect();
    let out = out.trim_matches('_').chars().take(48).collect::<String>();
    if out.is_empty() {
        "asset".into()
    } else {
        out
    }
}

//! Unit tests for Creative IR scan + canvas projection (quality guards).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::creative::{
    build_canvas_document, CreativeFilm, CreativeMediaFile, CreativeMediaKind, CreativeScene,
    CreativeShot, IngestedMedia, CREATIVE_IR_VERSION,
};
use crate::domain::{
    Camera, CharacterInScene, ShotBriefDescription, ShotDescription, VoiceProfile, WorkflowKind,
};
use crate::creative::ir::CreativeCharacter;

fn media(rel: &str, kind: CreativeMediaKind) -> CreativeMediaFile {
    CreativeMediaFile {
        abs_path: PathBuf::from(format!("/tmp/{rel}")),
        rel_path: rel.to_string(),
        kind,
        mime: match kind {
            CreativeMediaKind::Video => "video/mp4".into(),
            _ => "image/png".into(),
        },
        ext: match kind {
            CreativeMediaKind::Video => "mp4".into(),
            _ => "png".into(),
        },
        title: rel.to_string(),
    }
}

fn sample_film() -> CreativeFilm {
    let mut portraits = std::collections::BTreeMap::new();
    portraits.insert("front".into(), media("script2video/character_portraits/0_hero/front.png", CreativeMediaKind::Image));

    let mut voice = VoiceProfile {
        timbre: "清亮女中音".into(),
        volume: Some("normal".into()),
        pitch: Some("mid".into()),
        speaking_style: "语速平稳".into(),
        caption_clause: None,
        tts_voice: None,
    };
    voice.normalize("Alice");

    CreativeFilm {
        version: CREATIVE_IR_VERSION,
        session_id: "sess-1".into(),
        title: "Demo".into(),
        workflow: WorkflowKind::Script2Video,
        style: "cinematic".into(),
        aspect_ratio: "16:9".into(),
        resolution: "720p".into(),
        fps: 24,
        target_duration_secs: 20,
        llm_model: String::new(),
        image_model: String::new(),
        video_model: "seedance".into(),
        characters: vec![CreativeCharacter {
            idx: 0,
            identifier_in_scene: "Alice".into(),
            is_visible: true,
            static_features: "短发，红衣".into(),
            dynamic_features: None,
            voice_profile: Some(voice),
            portraits,
        }],
        world_assets: vec![
            crate::creative::CreativeWorldAsset {
                kind: crate::creative::CreativeWorldKind::Environment,
                key: "码头".into(),
                description: "EMPTY dock environment plate".into(),
                media: media(
                    "script2video/environments/0_码头/码头_environment_plate.png",
                    CreativeMediaKind::Image,
                ),
            },
            crate::creative::CreativeWorldAsset {
                kind: crate::creative::CreativeWorldKind::Prop,
                key: "木箱".into(),
                description: "wooden crate prop".into(),
                media: media("script2video/props/0_木箱/木箱_prop.png", CreativeMediaKind::Image),
            },
        ],
        scenes: vec![CreativeScene {
            key: "main".into(),
            title: "主场景".into(),
            artifact_root_rel: "script2video".into(),
            script: "Alice walks.".into(),
            characters: vec![],
            storyboard: vec![ShotBriefDescription {
                idx: 0,
                is_last: true,
                cam_idx: 0,
                visual_desc: "Alice walks into frame".into(),
                audio_desc: Some("你好".into()),
                beats: Vec::new(),
            }],
            shot_descriptions: vec![ShotDescription {
                idx: 0,
                is_last: true,
                cam_idx: 0,
                visual_desc: "Alice walks into frame".into(),
                variation_type: "establishing".into(),
                variation_reason: "open".into(),
                ff_desc: "wide shot Alice".into(),
                ff_vis_char_idxs: vec![0],
                lf_desc: "Alice mid-frame".into(),
                lf_vis_char_idxs: vec![0],
                motion_desc: "slow dolly in".into(),
                audio_desc: Some("你好".into()),
                beats: Vec::new(),
            }],
            camera_tree: vec![Camera {
                idx: 0,
                active_shot_idxs: vec![0],
                parent_cam_idx: None,
                parent_shot_idx: None,
                reason: Some("master".into()),
                is_parent_fully_covers_child: None,
                missing_info: None,
            }],
            shots: vec![CreativeShot {
                idx: 0,
                cam_idx: 0,
                artifact_rel: "script2video/shots/0/video.mp4".into(),
                video: Some(media("script2video/shots/0/video.mp4", CreativeMediaKind::Video)),
                first_frame: Some(media("script2video/shots/0/first_frame.png", CreativeMediaKind::Image)),
                last_frame: None,
                video_last_frame: None,
            }],
            final_video: Some(media("script2video/final_video.mp4", CreativeMediaKind::Video)),
        }],
        final_video: Some(media("script2video/final_video.mp4", CreativeMediaKind::Video)),
        cover: None,
    }
}

#[test]
fn canvas_doc_preserves_camera_tree_and_voice() {
    let film = sample_film();
    let mut ids = HashMap::new();
    for m in film.all_media_files() {
        let index = ids.len();
        ids.insert(
            m.rel_path.clone(),
            IngestedMedia {
                media_id: format!("media-{index}"),
                bytes: 4096 + index as u64,
                mime: m.mime.clone(),
            },
        );
    }
    let doc = build_canvas_document(&film, &ids);
    let allo = doc.get("alloCreative").expect("alloCreative sidecar");
    assert_eq!(allo.get("sessionId").and_then(|v| v.as_str()), Some("sess-1"));
    assert!(allo.get("writeBack").is_some());

    let film_side = allo.get("film").expect("film");
    let scenes = film_side.get("scenes").and_then(|v| v.as_array()).unwrap();
    let cam = &scenes[0]["camera_tree"];
    assert!(cam.as_array().unwrap().len() == 1);

    let nodes = doc.get("nodes").and_then(|v| v.as_array()).unwrap();
    let script = nodes
        .iter()
        .find(|n| n.get("type").and_then(|t| t.as_str()) == Some("script"))
        .expect("script node");
    let row = &script["metadata"]["storyboard"]["rows"][0];
    let video_prompt = row["videoMotionPrompt"].as_str().unwrap();
    assert!(
        video_prompt.contains("FIXED SPEAKER VOICE"),
        "video prompt must inject voice bible: {video_prompt}"
    );
    assert!(
        video_prompt.contains("Frame: 16:9 landscape"),
        "canvas video prompt must use the film's user ratio, not a hardcoded 9:16: {video_prompt}"
    );
    assert_eq!(row["videoNodeId"].as_str(), Some("vimax-shot-main-0"));

    let video = nodes
        .iter()
        .find(|n| n.get("id").and_then(|t| t.as_str()) == Some("vimax-shot-main-0"))
        .expect("video node");
    assert_eq!(
        video["metadata"]["alloVimax"]["kind"].as_str(),
        Some("shot_video")
    );
    assert!(video["metadata"]["storageKey"]
        .as_str()
        .unwrap()
        .starts_with("resource:"));
    // Ingested lookup hints land on materialized nodes so the frontend skips
    // HEAD probes when opening the canvas.
    assert_eq!(video["metadata"]["mimeType"].as_str(), Some("video/mp4"));
    assert!(video["metadata"]["bytes"].as_u64().unwrap() > 0);

    let world = nodes
        .iter()
        .find(|n| n.get("id").and_then(|t| t.as_str()) == Some("vimax-world-environment-码头"))
        .expect("environment node");
    assert_eq!(world["type"].as_str(), Some("image"));
    assert_eq!(world["metadata"]["mimeType"].as_str(), Some("image/png"));
    assert!(world["metadata"]["bytes"].as_u64().unwrap() > 0);

    let conns = doc.get("connections").and_then(|v| v.as_array()).unwrap();
    let has = |from: &str, to: &str| {
        conns.iter().any(|c| {
            c["fromNodeId"].as_str() == Some(from) && c["toNodeId"].as_str() == Some(to)
        })
    };
    assert!(has("vimax-style", "vimax-char-0"), "style→cast");
    assert!(has("vimax-style", "vimax-world-environment-码头"), "style→env");
    assert!(has("vimax-char-0", "vimax-script-main"), "cast→script");
    assert!(has("vimax-world-environment-码头", "vimax-script-main"), "env→script");
    assert!(has("vimax-char-0", "vimax-shot-main-0"), "cast→shot video");
    assert!(has("vimax-world-prop-木箱", "vimax-shot-main-0"), "prop→shot video");
    assert!(has("vimax-shot-main-0", "vimax-final"), "shot→final");
}

#[test]
fn character_from_domain_keeps_voice() {
    let c = CharacterInScene {
        idx: 1,
        identifier_in_scene: "Bob".into(),
        is_visible: true,
        static_features: "tall".into(),
        dynamic_features: None,
        voice_profile: Some(VoiceProfile {
            timbre: "低沉".into(),
            volume: Some("normal".into()),
            pitch: Some("low".into()),
            speaking_style: "稳重".into(),
            caption_clause: None,
            tts_voice: None,
        }),
    };
    let creative = CreativeCharacter::from_domain(&c);
    let clause = creative.seedance_voice_clause().expect("voice");
    assert!(clause.contains("FIXED SPEAKER VOICE"));
    assert!(clause.contains("Bob"));
}

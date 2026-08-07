//! Canonical creative IR shared by Agent (ViMax) materialization and Canvas projection.
//!
//! This is intentionally richer than either UI surface: Canvas may display a subset,
//! but must never drop camera trees, voice bibles, or continuity frames when projecting.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::{Camera, CharacterInScene, ShotBriefDescription, ShotDescription, VoiceProfile, WorkflowKind};

/// Schema version for Creative IR / Canvas `alloCreative` sidecar.
pub const CREATIVE_IR_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeFilm {
    pub version: u32,
    pub session_id: String,
    pub title: String,
    pub workflow: WorkflowKind,
    pub style: String,
    pub aspect_ratio: String,
    pub resolution: String,
    pub fps: u32,
    pub target_duration_secs: u32,
    pub llm_model: String,
    pub image_model: String,
    pub video_model: String,
    /// Film-level cast (shared across scenes when present).
    pub characters: Vec<CreativeCharacter>,
    /// Global environment / prop plates (Seedance multi-ref R2V assets).
    #[serde(default)]
    pub world_assets: Vec<CreativeWorldAsset>,
    pub scenes: Vec<CreativeScene>,
    /// Film-level final cut when present.
    #[serde(default)]
    pub final_video: Option<CreativeMediaFile>,
    #[serde(default)]
    pub cover: Option<CreativeMediaFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeWorldAsset {
    pub kind: CreativeWorldKind,
    pub key: String,
    pub description: String,
    pub media: CreativeMediaFile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeWorldKind {
    Environment,
    Prop,
}

impl CreativeWorldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment",
            Self::Prop => "prop",
        }
    }

    pub fn workflow_kind(self) -> &'static str {
        match self {
            Self::Environment => "scene",
            Self::Prop => "reference_set",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeCharacter {
    pub idx: i32,
    pub identifier_in_scene: String,
    pub is_visible: bool,
    pub static_features: String,
    #[serde(default)]
    pub dynamic_features: Option<String>,
    #[serde(default)]
    pub voice_profile: Option<VoiceProfile>,
    /// Portrait views keyed by view name (`front` / `side` / `back` / `cameo` / `sheet`).
    #[serde(default)]
    pub portraits: BTreeMap<String, CreativeMediaFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeScene {
    /// Stable key: `main` for script2video, `scene_0` / `scene_1` for multi-scene.
    pub key: String,
    pub title: String,
    /// Relative path of scene root under session working dir (e.g. `script2video`).
    pub artifact_root_rel: String,
    #[serde(default)]
    pub script: String,
    #[serde(default)]
    pub characters: Vec<CreativeCharacter>,
    pub storyboard: Vec<ShotBriefDescription>,
    pub shot_descriptions: Vec<ShotDescription>,
    /// Full camera tree — must survive Canvas round-trip as sidecar.
    pub camera_tree: Vec<Camera>,
    pub shots: Vec<CreativeShot>,
    #[serde(default)]
    pub final_video: Option<CreativeMediaFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeShot {
    pub idx: i32,
    pub cam_idx: i32,
    pub artifact_rel: String,
    #[serde(default)]
    pub video: Option<CreativeMediaFile>,
    #[serde(default)]
    pub first_frame: Option<CreativeMediaFile>,
    #[serde(default)]
    pub last_frame: Option<CreativeMediaFile>,
    #[serde(default)]
    pub video_last_frame: Option<CreativeMediaFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeMediaFile {
    /// Absolute path on disk at scan time (materializer copies from here).
    pub abs_path: PathBuf,
    /// Path relative to session working dir (for write-back).
    pub rel_path: String,
    pub kind: CreativeMediaKind,
    pub mime: String,
    pub ext: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeMediaKind {
    Image,
    Video,
    Audio,
    File,
}

impl CreativeMediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::File => "file",
        }
    }
}

impl CreativeCharacter {
    pub fn from_domain(c: &CharacterInScene) -> Self {
        Self {
            idx: c.idx,
            identifier_in_scene: c.identifier_in_scene.clone(),
            is_visible: c.is_visible,
            static_features: c.static_features.clone(),
            dynamic_features: c.dynamic_features.clone(),
            voice_profile: c.voice_profile.clone(),
            portraits: BTreeMap::new(),
        }
    }

    /// Seedance-ready FIXED SPEAKER VOICE clause when a voice bible exists.
    pub fn seedance_voice_clause(&self) -> Option<String> {
        self.voice_profile.as_ref().and_then(|vp| {
            if !vp.is_usable() {
                return None;
            }
            Some(vp.seedance_clause(&self.identifier_in_scene))
        })
    }
}

impl CreativeFilm {
    pub fn all_media_files(&self) -> Vec<&CreativeMediaFile> {
        let mut out = Vec::new();
        if let Some(v) = &self.final_video {
            out.push(v);
        }
        if let Some(c) = &self.cover {
            out.push(c);
        }
        for ch in &self.characters {
            out.extend(ch.portraits.values());
        }
        for wa in &self.world_assets {
            out.push(&wa.media);
        }
        for scene in &self.scenes {
            if let Some(v) = &scene.final_video {
                out.push(v);
            }
            for ch in &scene.characters {
                out.extend(ch.portraits.values());
            }
            for shot in &scene.shots {
                if let Some(v) = &shot.video {
                    out.push(v);
                }
                if let Some(v) = &shot.first_frame {
                    out.push(v);
                }
                if let Some(v) = &shot.last_frame {
                    out.push(v);
                }
                if let Some(v) = &shot.video_last_frame {
                    out.push(v);
                }
            }
        }
        out
    }

    pub fn scene_by_key(&self, key: &str) -> Option<&CreativeScene> {
        self.scenes.iter().find(|s| s.key == key)
    }
}

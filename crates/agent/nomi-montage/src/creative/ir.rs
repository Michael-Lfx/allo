//! Creative IR — the minimal shape Canvas materialization needs from a Montage
//! project. Deliberately smaller than a full film model (contrast the retired
//! ViMax `CreativeFilm`): Montage's source of truth is the artifact set, so
//! this IR is a *read-only projection* of `scene_plan` + `asset_manifest` +
//! `renders/final.mp4`, not a parallel domain model to keep in sync.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const CREATIVE_IR_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreativeMediaKind {
    Image,
    Video,
    Audio,
    File,
}

impl CreativeMediaKind {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "gif" => Self::Image,
            "mp4" | "mov" | "webm" | "mkv" => Self::Video,
            "mp3" | "wav" | "aac" | "m4a" => Self::Audio,
            _ => Self::File,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeMediaRef {
    pub abs_path: PathBuf,
    /// Relative to the project root — stable across machines for write-back.
    pub rel_path: String,
    pub kind: CreativeMediaKind,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeShot {
    pub idx: i64,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_motion: bool,
    #[serde(default)]
    pub media: Option<CreativeMediaRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeScene {
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub shots: Vec<CreativeShot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreativeFilm {
    pub version: u32,
    pub project_id: String,
    pub title: String,
    pub pipeline: String,
    #[serde(default)]
    pub style_playbook: Option<String>,
    #[serde(default)]
    pub scenes: Vec<CreativeScene>,
    #[serde(default)]
    pub final_video: Option<CreativeMediaRef>,
}

impl CreativeFilm {
    pub fn total_shots(&self) -> usize {
        self.scenes.iter().map(|s| s.shots.len()).sum()
    }

    pub fn all_media(&self) -> Vec<&CreativeMediaRef> {
        let mut out: Vec<&CreativeMediaRef> = self
            .scenes
            .iter()
            .flat_map(|s| s.shots.iter())
            .filter_map(|shot| shot.media.as_ref())
            .collect();
        if let Some(v) = &self.final_video {
            out.push(v);
        }
        out
    }
}

//! Shot description models (ViMax `interfaces/shot_description.py`).

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotBriefDescription {
    pub idx: i32,
    pub is_last: bool,
    pub cam_idx: i32,
    pub visual_desc: String,
    #[serde(default)]
    pub audio_desc: Option<String>,
}

impl fmt::Display for ShotBriefDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Shot {}:", self.idx)?;
        writeln!(f, "Camera Index: {}", self.cam_idx)?;
        writeln!(f, "Visual: {}", self.visual_desc)?;
        if let Some(audio) = &self.audio_desc {
            write!(f, "Audio: {audio}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotDescription {
    pub idx: i32,
    pub is_last: bool,
    pub cam_idx: i32,
    pub visual_desc: String,
    pub variation_type: String,
    pub variation_reason: String,
    pub ff_desc: String,
    #[serde(default)]
    pub ff_vis_char_idxs: Vec<i32>,
    pub lf_desc: String,
    #[serde(default)]
    pub lf_vis_char_idxs: Vec<i32>,
    pub motion_desc: String,
    #[serde(default)]
    pub audio_desc: Option<String>,
}

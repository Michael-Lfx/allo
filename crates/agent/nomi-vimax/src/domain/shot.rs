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

/// One beat inside a clip: a single spoken line or a single visual event.
///
/// A clip normally renders exactly one beat, so [`ShotDescription::beats`] stays
/// empty. When planning merges adjacent shots that share a camera — one clip
/// instead of two means one splice fewer to stutter on — the absorbed beats are
/// preserved here in timeline order so the prompt can lay them out against the
/// clip's final duration and duration estimation can price the whole run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotBeat {
    pub motion_desc: String,
    #[serde(default)]
    pub audio_desc: Option<String>,
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
    /// Timeline-ordered beats this clip plays in one continuous take.
    ///
    /// Empty for the usual one-beat clip. Two or more marks a merged clip, which
    /// must never be merged again (see [`ShotDescription::is_merged`]).
    #[serde(default)]
    pub beats: Vec<ShotBeat>,
}

impl ShotDescription {
    /// True when this clip already absorbed adjacent shots.
    ///
    /// Merging is not idempotent — absorbing an already-merged clip would replay
    /// its beats — so every producer of merged clips checks this first.
    pub fn is_merged(&self) -> bool {
        self.beats.len() >= 2
    }
}

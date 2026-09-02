//! Shot description models (ViMax `interfaces/shot_description.py`).

use serde::{Deserialize, Serialize};
use std::fmt;

/// One beat inside a planned storyboard row (one generated video).
///
/// Empty [`ShotBriefDescription::beats`] means the row is a single beat.
/// Two or more means planning already packed adjacent events — including a
/// reverse-angle CUT — into this row, so the storyboard UI and the renderer
/// share the same clip count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotBriefBeat {
    pub visual_desc: String,
    #[serde(default)]
    pub audio_desc: Option<String>,
    pub cam_idx: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotBriefDescription {
    pub idx: i32,
    pub is_last: bool,
    pub cam_idx: i32,
    pub visual_desc: String,
    #[serde(default)]
    pub audio_desc: Option<String>,
    /// Timeline-ordered beats this row plays in one generation.
    ///
    /// Empty for a single-beat row. Two or more is an inner timeline of the
    /// SAME generated file: packing may have absorbed adjacent rows (a reverse
    /// CUT keeps a different `cam_idx`), or a same-camera densify split one
    /// prose line into performance beats. Extra beats are never extra files.
    #[serde(default)]
    pub beats: Vec<ShotBriefBeat>,
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

impl ShotBriefDescription {
    /// True when this storyboard row already absorbed adjacent planner shots.
    pub fn is_merged(&self) -> bool {
        self.beats.len() >= 2
    }

    /// Camera the row *ends* on — what the next row's seam compares against.
    pub fn exit_cam_idx(&self) -> i32 {
        self.beats.last().map(|beat| beat.cam_idx).unwrap_or(self.cam_idx)
    }
}

/// One beat inside a clip: a spoken line, a visual event, or a native camera cut.
///
/// A clip normally renders exactly one beat, so [`ShotDescription::beats`] stays
/// empty. When planning packs adjacent events into one storyboard row — one clip
/// instead of two means one splice fewer to stutter on — the absorbed beats are
/// preserved here in timeline order so the prompt can lay them out against the
/// clip's final duration and duration estimation can price the whole run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotBeat {
    pub motion_desc: String,
    #[serde(default)]
    pub audio_desc: Option<String>,
    /// Camera this beat was planned on. `None` on artifacts written before
    /// packing recorded per-beat cameras — treated as the parent clip's
    /// [`ShotDescription::cam_idx`].
    #[serde(default)]
    pub cam_idx: Option<i32>,
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
    /// Timeline-ordered beats this clip plays in one generation.
    ///
    /// Empty for the usual one-beat clip. Two or more marks a packed clip, which
    /// must never be packed again (see [`ShotDescription::is_merged`]). Same
    /// camera → one continuous take; different [`ShotBeat::cam_idx`] → native
    /// multi-shot (CUT inside the file).
    #[serde(default)]
    pub beats: Vec<ShotBeat>,
}

impl ShotDescription {
    /// True when this clip already absorbed adjacent shots.
    ///
    /// Packing is not idempotent — absorbing an already-packed clip would replay
    /// its beats — so every producer of packed clips checks this first.
    pub fn is_merged(&self) -> bool {
        self.beats.len() >= 2
    }

    /// Camera the clip *ends* on — what the next clip's seam compares against.
    ///
    /// Equals [`Self::cam_idx`] for an unpacked shot or a same-camera pack.
    /// A native multi-shot that cut to a reverse angle exits on that later
    /// camera, so the next clip must not be told it is still rolling on the
    /// opening setup.
    pub fn exit_cam_idx(&self) -> i32 {
        self.beats
            .iter()
            .rev()
            .find_map(|beat| beat.cam_idx)
            .unwrap_or(self.cam_idx)
    }

    /// True when packed beats change camera inside this generation.
    pub fn has_camera_cuts(&self) -> bool {
        self.beats
            .iter()
            .any(|beat| beat.cam_idx.is_some_and(|cam| cam != self.cam_idx))
    }
}

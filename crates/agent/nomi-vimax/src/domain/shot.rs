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
    /// World-asset slugline this row belongs to (e.g. `INT. CAFE - NIGHT`).
    ///
    /// Empty on artifacts written before location binding. Render binds the
    /// environment plate by this field, not token overlap with the visual line.
    #[serde(default)]
    pub location_id: String,
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

/// One beat inside a legacy multi-beat clip.
///
/// A published row leaves [`ShotDescription::beats`] empty: one row is one
/// camera and one performance. A non-empty list is an old artifact the prompt
/// still plays in order so a line is not dropped.
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
    /// World-asset slugline this clip belongs to. Empty means "unknown" —
    /// the renderer will not guess the first environment plate.
    #[serde(default)]
    pub location_id: String,
    /// Legacy beats played in order inside one generation.
    ///
    /// Empty on a published row. Two or more marks an old packed clip, which
    /// must not be packed again (see [`ShotDescription::is_merged`]). A camera
    /// change belongs in the next row, not in this list.
    #[serde(default)]
    pub beats: Vec<ShotBeat>,
}

impl ShotDescription {
    /// True when this clip still carries a legacy multi-beat list.
    ///
    /// A published row leaves [`Self::beats`] empty. Joining an already-listed
    /// clip again would replay those beats, so producers check this first.
    pub fn is_merged(&self) -> bool {
        self.beats.len() >= 2
    }

    /// Camera the clip ends on — what the next clip's seam compares against.
    ///
    /// Equals [`Self::cam_idx`] unless this file changes camera inside the
    /// generation.
    pub fn exit_cam_idx(&self) -> i32 {
        self.beats
            .iter()
            .rev()
            .find_map(|beat| beat.cam_idx)
            .unwrap_or(self.cam_idx)
    }

    /// True when this generation changes camera inside the file.
    pub fn has_camera_cuts(&self) -> bool {
        self.beats
            .iter()
            .any(|beat| beat.cam_idx.is_some_and(|cam| cam != self.cam_idx))
    }

    /// The audio the storyboard wrote for this clip: `audio_desc`, or the packed
    /// beats' audio when the parent row left it empty.
    pub fn audio_source(&self) -> String {
        let top = self.audio_desc.as_deref().unwrap_or("").trim();
        if !top.is_empty() {
            return top.to_string();
        }
        let mut parts = Vec::new();
        for beat in &self.beats {
            if let Some(a) = beat
                .audio_desc
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(a);
            }
        }
        parts.join(" ")
    }

    /// Does this clip carry a spoken line, not only SFX / BGM? A clip without one
    /// is the only kind whose held final frame may be trimmed at concat.
    pub fn has_spoken_dialogue(&self) -> bool {
        crate::planning::text_looks_like_dialogue(&self.audio_source())
            || crate::planning::text_looks_like_dialogue(&self.motion_desc)
            || crate::planning::text_looks_like_dialogue(&self.visual_desc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(motion: &str, audio: Option<&str>) -> ShotDescription {
        ShotDescription {
            idx: 0,
            is_last: false,
            cam_idx: 0,
            visual_desc: String::new(),
            variation_type: "small".into(),
            variation_reason: String::new(),
            ff_desc: String::new(),
            ff_vis_char_idxs: Vec::new(),
            lf_desc: String::new(),
            lf_vis_char_idxs: Vec::new(),
            motion_desc: motion.into(),
            audio_desc: audio.map(str::to_string),
            location_id: String::new(),
            beats: Vec::new(),
        }
    }

    #[test]
    fn a_silent_action_clip_has_no_spoken_dialogue() {
        let silent = shot("她转身。她抬头", Some("环境音：风声，BGM 低沉"));
        assert!(!silent.has_spoken_dialogue());
    }

    #[test]
    fn a_named_line_in_audio_is_spoken() {
        let spoken = shot("她转身", Some("林晚：「今晚别等我。」"));
        assert!(spoken.has_spoken_dialogue());
    }

    /// A packed clip's dialogue lives on its beats when the parent row left
    /// `audio_desc` empty; it must still count as spoken.
    #[test]
    fn a_line_carried_by_a_packed_beat_is_spoken() {
        let mut packed = shot("她转身", None);
        packed.beats = vec![
            ShotBeat {
                motion_desc: "她转身".into(),
                audio_desc: None,
                cam_idx: Some(0),
            },
            ShotBeat {
                motion_desc: "她开口".into(),
                audio_desc: Some("林晚：「你来了。」".into()),
                cam_idx: Some(0),
            },
        ];
        assert_eq!(packed.audio_source(), "林晚：「你来了。」");
        assert!(packed.has_spoken_dialogue());
    }
}

/// True when both sides name a location and they are not the same place.
pub fn location_changed(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    !a.is_empty() && !b.is_empty() && !location_keys_match(a, b)
}

/// Slugline / path matching that ignores whitespace and ASCII case.
pub fn location_keys_match(a: &str, b: &str) -> bool {
    let na = normalize_location_key(a);
    let nb = normalize_location_key(b);
    !na.is_empty() && na == nb
}

pub fn normalize_location_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || (*c as u32) > 127)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

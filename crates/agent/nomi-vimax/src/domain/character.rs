//! Character models (ViMax `interfaces/character.py`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Stable spoken-voice bible for one cast member across all shots.
///
/// Seedance regenerates audio per clip; without an explicit voice lock the same
/// character often changes timbre/volume between shots. Persist once on the
/// character and re-inject into every I2V audio caption.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VoiceProfile {
    /// Timbre / vocal color, e.g. "清亮偏柔的女中音，略带气声".
    #[serde(default)]
    pub timbre: String,
    /// Relative loudness: quiet | normal | loud | booming (or free text).
    #[serde(default)]
    pub volume: Option<String>,
    /// Pitch band: low | mid | high (or free text).
    #[serde(default)]
    pub pitch: Option<String>,
    /// Pace, tone, accent, emotional baseline.
    #[serde(default)]
    pub speaking_style: String,
    /// Precomputed one-line clause for Seedance captions (preferred inject form).
    #[serde(default)]
    pub caption_clause: Option<String>,
}

impl VoiceProfile {
    pub fn is_usable(&self) -> bool {
        !self.timbre.trim().is_empty()
            || !self.speaking_style.trim().is_empty()
            || self
                .caption_clause
                .as_deref()
                .map(str::trim)
                .is_some_and(|s| !s.is_empty())
    }

    /// Compact Seedance-ready clause (stable across shots).
    pub fn seedance_clause(&self, character_name: &str) -> String {
        if let Some(c) = self
            .caption_clause
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return c.to_string();
        }
        let mut parts = Vec::new();
        let timbre = self.timbre.trim();
        if !timbre.is_empty() {
            parts.push(timbre.to_string());
        }
        if let Some(v) = self.volume.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            parts.push(format!("volume {v}"));
        }
        if let Some(p) = self.pitch.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            parts.push(format!("pitch {p}"));
        }
        let style = self.speaking_style.trim();
        if !style.is_empty() {
            parts.push(style.to_string());
        }
        if parts.is_empty() {
            format!("{character_name}: stable distinctive speaking voice, consistent across shots")
        } else {
            format!("{character_name}: {}", parts.join("; "))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterInScene {
    pub idx: i32,
    pub identifier_in_scene: String,
    pub is_visible: bool,
    pub static_features: String,
    #[serde(default)]
    pub dynamic_features: Option<String>,
    /// Film-stable voice bible; generated once and reused on every shot.
    #[serde(default)]
    pub voice_profile: Option<VoiceProfile>,
}

impl fmt::Display for CharacterInScene {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.identifier_in_scene)?;
        if self.is_visible {
            write!(f, "[visible]")?;
        } else {
            write!(f, "[not visible]")?;
        }
        writeln!(f)?;
        writeln!(f, "static features: {}", self.static_features)?;
        writeln!(
            f,
            "dynamic features: {}",
            self.dynamic_features.as_deref().unwrap_or("")
        )?;
        if let Some(vp) = self.voice_profile.as_ref().filter(|v| v.is_usable()) {
            writeln!(f, "voice: {}", vp.seedance_clause(&self.identifier_in_scene))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterInEvent {
    pub index: i32,
    pub identifier_in_event: String,
    pub active_scenes: HashMap<i32, String>,
    pub static_features: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterInNovel {
    pub index: i32,
    pub identifier_in_novel: String,
    pub active_events: HashMap<i32, String>,
    pub static_features: String,
}

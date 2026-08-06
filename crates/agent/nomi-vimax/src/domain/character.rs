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

    /// Fill defaults and rebuild a canonical Seedance inject string.
    ///
    /// Always rebuilds `caption_clause` from structured fields so every shot
    /// gets the identical fingerprint (LLM freeform captions drift too easily).
    pub fn normalize(&mut self, character_name: &str) {
        if self.timbre.trim().is_empty() {
            if let Some(c) = self
                .caption_clause
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                // Salvage freeform caption into timbre when fields were empty.
                self.timbre = c.to_string();
            }
        }
        if self
            .volume
            .as_deref()
            .map(str::trim)
            .is_none_or(|s| s.is_empty())
        {
            self.volume = Some("normal".into());
        }
        if self
            .pitch
            .as_deref()
            .map(str::trim)
            .is_none_or(|s| s.is_empty())
        {
            self.pitch = Some("mid".into());
        }
        if self.speaking_style.trim().is_empty() {
            self.speaking_style =
                "语速自然平稳，咬字清楚，跨镜头音色音量保持一致".into();
        }
        self.volume = self.volume.take().map(|v| normalize_volume_token(&v));
        self.pitch = self.pitch.take().map(|p| normalize_pitch_token(&p));
        self.caption_clause = Some(self.build_canonical_clause(character_name));
    }

    /// Compact Seedance-ready clause (stable across shots).
    pub fn seedance_clause(&self, character_name: &str) -> String {
        if let Some(c) = self
            .caption_clause
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            // Prefer already-normalized FIXED SPEAKER VOICE form.
            if c.contains("FIXED SPEAKER VOICE") {
                return c.to_string();
            }
        }
        self.build_canonical_clause(character_name)
    }

    fn build_canonical_clause(&self, character_name: &str) -> String {
        let name = character_name.trim();
        let timbre = self.timbre.trim();
        let volume = self
            .volume
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("normal");
        let pitch = self
            .pitch
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("mid");
        let style = self.speaking_style.trim();

        let mut core = Vec::new();
        if !timbre.is_empty() {
            core.push(format!("timbre「{timbre}」"));
        }
        core.push(format!("pitch {pitch}"));
        core.push(format!("volume {volume}"));
        if !style.is_empty() {
            core.push(style.to_string());
        }
        if core.is_empty() {
            return format!(
                "{name}: FIXED SPEAKER VOICE — stable distinctive speaking voice; \
identical timbre/pitch/age/gender every shot, never reinvent"
            );
        }
        format!(
            "{name}: FIXED SPEAKER VOICE — {}; \
SAME exact speaker identity every shot (never change timbre/pitch band/age/gender; \
emotion intensity may shift slightly only)",
            core.join("; ")
        )
    }
}

fn normalize_volume_token(raw: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    if t.is_empty() {
        return "normal".into();
    }
    if t.contains("quiet") || t.contains("soft") || t.contains("低") || t.contains("轻") || t.contains("小")
    {
        return "quiet".into();
    }
    if t.contains("boom") || t.contains("loud") || t.contains("大") || t.contains("高声") || t.contains("洪")
    {
        return "loud".into();
    }
    if t == "normal" || t.contains("中") || t.contains("常") {
        return "normal".into();
    }
    // Keep short free text; clamp length so captions stay compact.
    raw.chars().take(16).collect()
}

fn normalize_pitch_token(raw: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    if t.is_empty() {
        return "mid".into();
    }
    if t.contains("mid-high") || t.contains("midhigh") || t.contains("中高") {
        return "mid-high".into();
    }
    if t.contains("mid-low") || t.contains("midlow") || t.contains("中低") {
        return "mid-low".into();
    }
    if t.contains("high") || t.contains("高") || t.contains("尖") {
        return "high".into();
    }
    if t.contains("low") || t.contains("低") || t.contains("沉") {
        return "low".into();
    }
    if t.contains("mid") || t.contains("中") {
        return "mid".into();
    }
    raw.chars().take(16).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_builds_fixed_speaker_clause() {
        let mut vp = VoiceProfile {
            timbre: "清亮柔和的女中音，气息稳定".into(),
            volume: Some("Normal".into()),
            pitch: Some("mid-high".into()),
            speaking_style: "语速平稳".into(),
            caption_clause: Some("stale freeform".into()),
        };
        vp.normalize("李薇");
        let clause = vp.seedance_clause("李薇");
        assert!(clause.contains("FIXED SPEAKER VOICE"));
        assert!(clause.contains("李薇"));
        assert!(clause.contains("清亮柔和的女中音"));
        assert!(clause.contains("pitch mid-high"));
        assert!(clause.contains("volume normal"));
        assert_eq!(vp.caption_clause.as_deref(), Some(clause.as_str()));
    }

    #[test]
    fn seedance_clause_rebuilds_when_caption_not_canonical() {
        let vp = VoiceProfile {
            timbre: "沉稳男中音".into(),
            volume: Some("normal".into()),
            pitch: Some("mid".into()),
            speaking_style: "克制".into(),
            caption_clause: Some("老旧写法".into()),
        };
        let clause = vp.seedance_clause("阿强");
        assert!(clause.contains("FIXED SPEAKER VOICE"));
        assert!(clause.contains("沉稳男中音"));
    }
}

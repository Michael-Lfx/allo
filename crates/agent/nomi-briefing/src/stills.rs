//! Optional atmosphere stills for opening / evidence cards.
//! Graphic plates stay original compositor geometry; these photos sit underneath.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::BriefingResult;
use crate::ir::Beat;

/// Cards that benefit from a generated 16:9 atmosphere plate.
pub const ATMOSPHERE_CARDS: [&str; 2] = ["title_desk", "evidence_tour"];

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ImageChoice {
    pub provider_id: String,
    pub model: String,
}

impl ImageChoice {
    pub fn from_parts(provider_id: Option<&str>, model: Option<&str>) -> Option<Self> {
        let provider_id = provider_id.map(str::trim).filter(|s| !s.is_empty())?;
        let model = model.map(str::trim).filter(|s| !s.is_empty())?;
        Some(Self {
            provider_id: provider_id.to_string(),
            model: model.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedStill {
    pub bytes: Vec<u8>,
    pub mime: String,
}

pub trait StillSynth: Send + Sync {
    fn generate_still(&self, prompt: &str, choice: &ImageChoice) -> Result<GeneratedStill, String>;
}

pub fn uses_atmosphere(card: &str) -> bool {
    ATMOSPHERE_CARDS.contains(&card)
}

pub fn atmosphere_prompt(beat: &Beat) -> String {
    let topic = beat
        .on_screen
        .trim()
        .chars()
        .take(80)
        .collect::<String>();
    let topic = if topic.is_empty() {
        beat.spoken_text.trim().chars().take(80).collect::<String>()
    } else {
        topic
    };
    format!(
        "Editorial night newsroom still photograph, ink-navy shadows, brass desk lamp, \
cream paper stacks on a walnut desk, shallow depth of field, no readable text, \
no logos, no watermarks, no people facing camera, cinematic 16:9 atmosphere \
for a sourced news briefing about: {topic}"
    )
}

fn still_extension(mime: &str, bytes: &[u8]) -> &'static str {
    let mime = mime.to_ascii_lowercase();
    if mime.contains("jpeg") || mime.contains("jpg") || bytes.starts_with(&[0xFF, 0xD8]) {
        return "jpg";
    }
    if mime.contains("webp") || bytes.starts_with(b"RIFF") {
        return "webp";
    }
    "png"
}

/// Write `stills/bg_NNN.{png,jpg}` for atmosphere cards. Failures skip the beat
/// so graphic PPM plates still compose.
pub fn persist_atmosphere_stills(
    working_dir: &Path,
    beats: &[Beat],
    synth: &dyn StillSynth,
    choice: &ImageChoice,
) -> BriefingResult<usize> {
    let dir = working_dir.join("stills");
    std::fs::create_dir_all(&dir)?;
    let mut written = 0usize;
    for (index, beat) in beats.iter().enumerate() {
        if !uses_atmosphere(&beat.card) {
            continue;
        }
        let prompt = atmosphere_prompt(beat);
        let Ok(still) = synth.generate_still(&prompt, choice) else {
            continue;
        };
        if still.bytes.len() < 64 {
            continue;
        }
        let ext = still_extension(&still.mime, &still.bytes);
        let path = dir.join(format!("bg_{index:03}.{ext}"));
        std::fs::write(&path, &still.bytes)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_cards_use_atmosphere() {
        assert!(uses_atmosphere("title_desk"));
        assert!(uses_atmosphere("evidence_tour"));
        assert!(!uses_atmosphere("subtitle_plain"));
    }
}

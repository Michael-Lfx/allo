use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResearchDepth {
    #[default]
    Fast,
    Deep,
}

impl ResearchDepth {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "fast" => Some(Self::Fast),
            "deep" => Some(Self::Deep),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Deep => "deep",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisualKind {
    EvidenceScreenshot,
    GeneratedInfographic,
    LicensedBroll,
    #[default]
    UserAsset,
}

impl VisualKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceScreenshot => "evidence_screenshot",
            Self::GeneratedInfographic => "generated_infographic",
            Self::LicensedBroll => "licensed_broll",
            Self::UserAsset => "user_asset",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Citation {
    pub url: String,
    pub domain: String,
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub retrieved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claim {
    pub text: String,
    #[serde(default)]
    pub citation_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WordAnchor {
    pub word: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Beat {
    pub id: String,
    pub spoken_text: String,
    #[serde(default)]
    pub on_screen: String,
    #[serde(default)]
    pub visual: VisualKind,
    #[serde(default)]
    pub card: String,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub citations: Vec<Citation>,
    /// Filled by ASR align. Must stay empty until align — never hand-typed.
    #[serde(default)]
    pub anchors: Vec<WordAnchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BeatScript {
    pub format_secs: u32,
    #[serde(default)]
    pub beats: Vec<Beat>,
    #[serde(default)]
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ResearchPlan {
    pub intent: String,
    #[serde(default)]
    pub questions: Vec<String>,
    pub time_window_hours: u32,
    pub depth: ResearchDepth,
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Dossier {
    pub sources: Vec<Citation>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub unknowns: Vec<String>,
}

const HANZI_DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// Spoken-only numerals for TTS (talkcraft ① methodology, original impl).
pub fn spoken_numerals(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(digit) = ch.to_digit(10) {
            out.push(HANZI_DIGITS[digit as usize]);
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn domain_of(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.trim().trim_start_matches("www.");
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_numerals_hanziify_digits() {
        assert_eq!(spoken_numerals("增长 12%"), "增长 一二%");
    }
}

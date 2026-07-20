//! Robust JSON extraction from LLM responses (ViMax `robust_json_parser` spirit).

use crate::error::{VimaxError, VimaxResult};
use regex::Regex;
use serde::de::DeserializeOwned;
use std::sync::OnceLock;

fn fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```(?:json)?\s*(.*?)\s*```").expect("regex"))
}

/// Strip markdown fences and extract the outermost JSON object/array.
pub fn extract_json_str(raw: &str) -> VimaxResult<String> {
    let trimmed = raw.trim();
    if let Some(caps) = fence_re().captures(trimmed) {
        return Ok(caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default());
    }
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return Ok(trimmed[start..=end].to_string());
            }
        }
    }
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end > start {
                return Ok(trimmed[start..=end].to_string());
            }
        }
    }
    Err(VimaxError::Llm(format!(
        "no JSON object/array found in LLM response: {}",
        &trimmed.chars().take(200).collect::<String>()
    )))
}

/// Remove trailing commas before `}` / `]` (common LLM quirk).
pub fn strip_trailing_commas(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r",(\s*[}\]])").expect("regex"));
    re.replace_all(s, "$1").into_owned()
}

pub fn parse_llm_json<T: DeserializeOwned>(raw: &str) -> VimaxResult<T> {
    let extracted = extract_json_str(raw)?;
    let cleaned = strip_trailing_commas(&extracted);
    serde_json::from_str(&cleaned).map_err(|e| {
        VimaxError::Llm(format!(
            "failed to parse LLM JSON: {e}; body={}",
            &cleaned.chars().take(300).collect::<String>()
        ))
    })
}

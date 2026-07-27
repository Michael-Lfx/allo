//! Robust JSON extraction from LLM responses (ViMax `robust_json_parser` spirit).

use crate::error::{VimaxError, VimaxResult};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::OnceLock;

fn fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```(?:json)?\s*(.*?)\s*```").expect("regex"))
}

/// Strip markdown fences and extract the outermost JSON object/array.
pub fn extract_json_str(raw: &str) -> VimaxResult<String> {
    let trimmed = raw.trim();
    if let Some(caps) = fence_re().captures(trimmed) {
        return Ok(caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default());
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

/// Parse LLM JSON into `T`, tolerating common model quirks:
/// - markdown fences
/// - trailing commas
/// - **duplicate object keys** (keep the last value; typed `serde` rejects these)
pub fn parse_llm_json<T: DeserializeOwned>(raw: &str) -> VimaxResult<T> {
    let extracted = extract_json_str(raw)?;
    let cleaned = strip_trailing_commas(&extracted);
    // Via `Value` first: object maps overwrite duplicate keys. Direct
    // `from_str::<Struct>` fails with `duplicate field \`foo\``.
    let value: Value = serde_json::from_str(&cleaned).map_err(|e| {
        VimaxError::Llm(format!(
            "failed to parse LLM JSON: {e}; body={}",
            &cleaned.chars().take(300).collect::<String>()
        ))
    })?;
    serde_json::from_value(value).map_err(|e| {
        VimaxError::Llm(format!(
            "failed to parse LLM JSON: {e}; body={}",
            &cleaned.chars().take(300).collect::<String>()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Decomp {
        ff_desc: String,
        #[serde(default)]
        ff_vis_char_idxs: Vec<i32>,
        lf_desc: String,
        #[serde(default)]
        lf_vis_char_idxs: Vec<i32>,
        motion_desc: String,
    }

    #[test]
    fn parse_llm_json_keeps_last_duplicate_key() {
        let raw = r#"{
          "ff_desc": "first",
          "ff_vis_char_idxs": [0],
          "ff_vis_char_idxs": [0, 1],
          "lf_desc": "last",
          "lf_vis_char_idxs": [0],
          "motion_desc": "walks"
        }"#;
        let d: Decomp = parse_llm_json(raw).expect("parse");
        assert_eq!(d.ff_vis_char_idxs, vec![0, 1]);
        assert_eq!(d.ff_desc, "first");
    }

    #[test]
    fn parse_llm_json_strips_fence_and_trailing_comma() {
        let raw = "```json\n{\"ff_desc\":\"a\",\"ff_vis_char_idxs\":[0],\"lf_desc\":\"b\",\"lf_vis_char_idxs\":[],\"motion_desc\":\"c\",}\n```";
        let d: Decomp = parse_llm_json(raw).expect("parse");
        assert_eq!(d.ff_desc, "a");
        assert_eq!(d.motion_desc, "c");
    }
}

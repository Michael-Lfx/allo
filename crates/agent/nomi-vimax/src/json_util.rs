//! Robust JSON extraction from LLM responses (ViMax `robust_json_parser` spirit).
//!
//! Also provides chat/vision helpers that re-prompt when the model returns
//! unparseable or schema-invalid JSON (common with smaller / local LLMs).

use crate::backends::VimaxChat;
use crate::error::{VimaxError, VimaxResult};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::Path;
use std::sync::OnceLock;

/// Attempts for chat → JSON parse (includes the first try).
pub const LLM_JSON_PARSE_ATTEMPTS: u32 = 3;

fn fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```(?:json)?\s*(.*?)\s*```").expect("regex"))
}

/// Strip markdown fences and extract the outermost JSON object/array.
pub fn extract_json_str(raw: &str) -> VimaxResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(VimaxError::Llm("empty LLM response (no JSON)".into()));
    }
    if let Some(caps) = fence_re().captures(trimmed) {
        let inner = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        if inner.is_empty() {
            return Err(VimaxError::Llm(
                "empty JSON fence in LLM response".into(),
            ));
        }
        return Ok(inner);
    }
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return Ok(trimmed[start..=end].to_string());
            }
        }
        // Truncated object: take from first `{` and try to close later.
        return Ok(trimmed[start..].to_string());
    }
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end > start {
                return Ok(trimmed[start..=end].to_string());
            }
        }
        return Ok(trimmed[start..].to_string());
    }
    Err(VimaxError::Llm(format!(
        "no JSON object/array found in LLM response: {}",
        &trimmed.chars().take(200).collect::<String>()
    )))
}

/// Normalize fullwidth punctuation LLMs often emit as JSON structure.
///
/// Intentionally does **not** remap Chinese/smart quotation marks to ASCII `"`,
/// because dialogue like 「咚」or “咚” inside string values would then break JSON.
pub fn sanitize_llm_json_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let mapped = match ch {
            '，' => ',',
            '：' => ':',
            '；' => ';',
            '（' => '(',
            '）' => ')',
            '【' => '[',
            '】' => ']',
            '\u{00a0}' | '\u{3000}' => ' ',
            '\u{feff}' => continue, // BOM
            _ => ch,
        };
        out.push(mapped);
    }
    out
}

/// Escape ASCII `"` that appear inside JSON string values (common in screenplays:
/// `发出"咚"的一声`). A quote is treated as a string terminator only when the
/// next non-whitespace char is `,` `}` `]` `:` or EOF.
pub fn escape_inner_unescaped_quotes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0usize;
    let mut in_string = false;
    let mut escape = false;
    while i < chars.len() {
        let ch = chars[i];
        if !in_string {
            out.push(ch);
            if ch == '"' {
                in_string = true;
            }
            i += 1;
            continue;
        }
        if escape {
            out.push(ch);
            escape = false;
            i += 1;
            continue;
        }
        if ch == '\\' {
            out.push(ch);
            escape = true;
            i += 1;
            continue;
        }
        if ch == '"' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let is_terminator = match chars.get(j).copied() {
                None => true,
                Some(',' | '}' | ']' | ':') => true,
                Some(_) => false,
            };
            if is_terminator {
                out.push('"');
                in_string = false;
            } else {
                out.push('\\');
                out.push('"');
            }
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Remove trailing commas before `}` / `]` (common LLM quirk).
pub fn strip_trailing_commas(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r",(\s*[}\]])").expect("regex"));
    re.replace_all(s, "$1").into_owned()
}

/// If the model truncated mid-object, close open strings / braces / brackets.
pub fn repair_truncated_json(s: &str) -> String {
    let mut in_string = false;
    let mut escape = false;
    let mut stack: Vec<char> = Vec::new();
    for ch in s.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if stack.last() == Some(&ch) {
                    stack.pop();
                }
            }
            _ => {}
        }
    }
    let mut out = s.to_string();
    if in_string {
        out.push('"');
    }
    while let Some(closer) = stack.pop() {
        out.push(closer);
    }
    out
}

fn prepare_candidates(extracted: &str) -> Vec<String> {
    let sanitized = sanitize_llm_json_text(extracted);
    let cleaned = strip_trailing_commas(&sanitized);
    let escaped = escape_inner_unescaped_quotes(&cleaned);
    let repaired = repair_truncated_json(&escaped);
    let mut out = Vec::new();
    for c in [cleaned, escaped, repaired] {
        let c = strip_trailing_commas(&c);
        if !out.iter().any(|x| x == &c) {
            out.push(c);
        }
    }
    out
}

fn parse_error(e: impl std::fmt::Display, body: &str) -> VimaxError {
    VimaxError::Llm(format!(
        "failed to parse LLM JSON: {e}; body={}",
        &body.chars().take(300).collect::<String>()
    ))
}

/// Parse LLM JSON into `T`, tolerating common model quirks:
/// - markdown fences
/// - fullwidth structural punctuation
/// - unescaped ASCII quotes inside string values
/// - trailing commas
/// - lightly truncated braces/brackets
/// - **duplicate object keys** (keep the last value; typed `serde` rejects these)
pub fn parse_llm_json<T: DeserializeOwned>(raw: &str) -> VimaxResult<T> {
    let extracted = extract_json_str(raw)?;
    let mut last_err: Option<VimaxError> = None;
    for candidate in prepare_candidates(&extracted) {
        match serde_json::from_str::<Value>(&candidate) {
            Ok(value) => match serde_json::from_value::<T>(value) {
                Ok(v) => return Ok(v),
                Err(e) => last_err = Some(parse_error(e, &candidate)),
            },
            Err(e) => last_err = Some(parse_error(e, &candidate)),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        VimaxError::Llm("failed to parse LLM JSON: unknown error".into())
    }))
}

fn retry_user_prompt(user: &str, attempt: u32, last_err: &VimaxError) -> String {
    let hint: String = last_err.to_string().chars().take(220).collect();
    format!(
        "{user}\n\n\
---\n\
RETRY {attempt}/{LLM_JSON_PARSE_ATTEMPTS}: Previous reply was NOT valid JSON matching the schema.\n\
Parse error: {hint}\n\
Respond with ONLY one valid JSON value for the requested schema. \
Use ASCII commas/colons for structure. Inside string values NEVER put raw ASCII \
double quotes — use 「」 for dialogue/emphasis, or escape as \\\". \
Do not truncate. No markdown fences, no commentary."
    )
}

/// Call the chat model and parse JSON, re-prompting on parse / schema failures.
pub async fn complete_and_parse_llm_json<T: DeserializeOwned>(
    chat: &dyn VimaxChat,
    system: &str,
    user: &str,
) -> VimaxResult<T> {
    let mut last_err: Option<VimaxError> = None;
    for attempt in 1..=LLM_JSON_PARSE_ATTEMPTS {
        let prompted = match &last_err {
            None => user.to_string(),
            Some(err) => retry_user_prompt(user, attempt, err),
        };
        match chat.complete_text(system, &prompted).await {
            Ok(raw) if raw.trim().is_empty() => {
                tracing::warn!(attempt, "LLM chat complete returned empty body");
                last_err = Some(VimaxError::Llm(
                    "empty chat completion (model returned no content)".into(),
                ));
            }
            Ok(raw) => match parse_llm_json::<T>(&raw) {
                Ok(v) => {
                    if attempt > 1 {
                        tracing::info!(attempt, "LLM JSON parse succeeded after retry");
                    }
                    return Ok(v);
                }
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "LLM JSON parse failed");
                    last_err = Some(e);
                }
            },
            Err(e) => {
                tracing::warn!(attempt, error = %e, "LLM chat complete failed");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| VimaxError::Llm("LLM JSON parse failed".into())))
}

/// Vision chat + JSON parse with the same retry policy.
pub async fn complete_vision_and_parse_llm_json<T: DeserializeOwned>(
    chat: &dyn VimaxChat,
    system: &str,
    user_text: &str,
    image_paths: &[&Path],
) -> VimaxResult<T> {
    let mut last_err: Option<VimaxError> = None;
    for attempt in 1..=LLM_JSON_PARSE_ATTEMPTS {
        let prompted = match &last_err {
            None => user_text.to_string(),
            Some(err) => retry_user_prompt(user_text, attempt, err),
        };
        match chat
            .complete_vision(system, &prompted, image_paths)
            .await
        {
            Ok(raw) if raw.trim().is_empty() => {
                tracing::warn!(attempt, "LLM vision complete returned empty body");
                last_err = Some(VimaxError::Llm(
                    "empty vision completion (model returned no content)".into(),
                ));
            }
            Ok(raw) => match parse_llm_json::<T>(&raw) {
                Ok(v) => {
                    if attempt > 1 {
                        tracing::info!(attempt, "LLM vision JSON parse succeeded after retry");
                    }
                    return Ok(v);
                }
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "LLM vision JSON parse failed");
                    last_err = Some(e);
                }
            },
            Err(e) => {
                tracing::warn!(attempt, error = %e, "LLM vision complete failed");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| VimaxError::Llm("LLM vision JSON parse failed".into())))
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

    #[test]
    fn parse_llm_json_sanitizes_fullwidth_punctuation() {
        let raw = r#"{"ff_desc"："a"，"ff_vis_char_idxs":[0]，"lf_desc"："b"，"lf_vis_char_idxs":[]，"motion_desc"："c"}"#;
        let d: Decomp = parse_llm_json(raw).expect("parse");
        assert_eq!(d.ff_desc, "a");
        assert_eq!(d.lf_desc, "b");
    }

    #[test]
    fn parse_llm_json_repairs_truncated_object() {
        let raw = r#"{"ff_desc":"a","ff_vis_char_idxs":[0],"lf_desc":"b","lf_vis_char_idxs":[],"motion_desc":"c"#;
        let d: Decomp = parse_llm_json(raw).expect("parse truncated");
        assert_eq!(d.motion_desc, "c");
    }

    #[test]
    fn parse_llm_json_escapes_inner_ascii_quotes_in_strings() {
        #[derive(Debug, Deserialize)]
        struct Scenes {
            scenes: Vec<String>,
        }
        let raw = r#"{
  "scenes": [
    "马县长把酒杯一顿,发出"咚"的一声脆响。\n黄四郎:匪嘛,什么时候都有"
  ]
}"#;
        let s: Scenes = parse_llm_json(raw).expect("inner quotes");
        assert!(s.scenes[0].contains("咚"));
        assert!(s.scenes[0].contains("脆响"));
    }

    #[test]
    fn sanitize_maps_chinese_commas() {
        assert!(sanitize_llm_json_text("{\"a\":1，\"b\":2}").contains(','));
    }

    #[test]
    fn sanitize_keeps_cjk_quotation_marks() {
        let s = sanitize_llm_json_text(r#"{"a":"他说「你好」"}"#);
        assert!(s.contains('「'));
        assert!(!s.contains(r#"他说""#));
    }

    #[test]
    fn extract_json_str_rejects_empty_and_empty_fence() {
        assert!(extract_json_str("").is_err());
        assert!(extract_json_str("   ").is_err());
        assert!(extract_json_str("```json\n\n```").is_err());
        assert!(extract_json_str("```\n```").is_err());
    }

    #[test]
    fn parse_llm_json_empty_is_llm_error_not_serde_eof() {
        let err = parse_llm_json::<serde_json::Value>("")
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty LLM"), "{err}");
        assert!(!err.starts_with("JSON error:"), "{err}");
    }
}

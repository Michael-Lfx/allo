//! Truncation and secret redaction for trace previews.

use nomi_redact::redact_secrets;
use serde_json::Value;

/// Maximum characters kept in any preview string after redaction.
pub const MAX_PREVIEW_CHARS: usize = 2000;

/// Truncate `s` to at most `max` Unicode scalar values (char-boundary safe).
///
/// When truncation occurs, appends `…(truncated)`.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return "…(truncated)".to_owned();
    }
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…(truncated)")
}

/// Redact secrets then truncate to [`MAX_PREVIEW_CHARS`].
pub fn redact_preview(s: &str) -> String {
    let redacted = redact_secrets(s);
    truncate_chars(redacted.as_ref(), MAX_PREVIEW_CHARS)
}

/// Deep-clone a JSON value, redacting and truncating every string leaf.
///
/// Write path uses [`crate::capture::capture_canonical_request`]. This helper
/// remains as the unit-test oracle for bulky/sensitive key rules.
#[cfg(test)]
fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(s) => Value::String(redact_preview(s)),
        Value::Array(items) => Value::Array(items.iter().map(redact_json_value).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if should_omit_bulky(k, v) {
                    out.insert(k.clone(), bulky_placeholder(v));
                    continue;
                }
                let redacted = if is_sensitive_key(k) {
                    match v {
                        Value::String(_) => {
                            Value::String("[REDACTED_SECRET]".to_owned())
                        }
                        other => redact_json_value(other),
                    }
                } else {
                    redact_json_value(v)
                };
                out.insert(k.clone(), redacted);
            }
            Value::Object(out)
        }
    }
}

pub(crate) fn should_omit_bulky(key: &str, value: &Value) -> bool {
    // Message `content` is an array of blocks and must be walked (media lives there).
    if matches!(key, "content" | "contents") && matches!(value, Value::Array(_)) {
        return false;
    }
    is_bulky_tool_arg_key(key)
}

pub(crate) fn is_bulky_tool_arg_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "content"
            | "contents"
            | "file_text"
            | "filetext"
            | "old_string"
            | "new_string"
            | "old_str"
            | "new_str"
            | "oldtext"
            | "newtext"
            | "patch"
            | "diff"
            | "data"
            | "bytes"
            | "base64"
    )
}

pub(crate) fn bulky_placeholder(value: &Value) -> Value {
    let chars = match value {
        Value::String(s) => s.chars().count(),
        Value::Array(items) => items.len(),
        Value::Object(map) => map.len(),
        _ => 0,
    };
    Value::String(format!("[{chars} omitted]"))
}

pub(crate) fn is_sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    matches!(
        normalized.as_str(),
        "apikey"
            | "token"
            | "secret"
            | "password"
            | "passwd"
            | "accesstoken"
            | "refreshtoken"
            | "authorization"
            | "auth"
            | "credential"
            | "credentials"
            | "privatekey"
    ) || normalized.contains("apikey")
        || normalized.ends_with("token")
        || normalized.ends_with("secret")
        || normalized.ends_with("password")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate_chars("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_marks_truncated() {
        let long: String = "字".repeat(10);
        let out = truncate_chars(&long, 3);
        assert!(out.starts_with("字字字"));
        assert!(out.contains("…(truncated)"));
        assert!(!out.contains(&"字".repeat(4)));
    }

    #[test]
    fn redact_preview_strips_openai_key() {
        let out = redact_preview("key is sk-ABCDEFGHIJ0123456789xyz here");
        assert!(out.contains("[REDACTED_SECRET]"));
        assert!(!out.contains("sk-ABCDEFGHIJ"));
    }

    #[test]
    fn redact_json_omits_bulky_write_content() {
        let value = json!({
            "file_path": "a.py",
            "content": "x".repeat(10_000)
        });
        let out = redact_json_value(&value);
        assert_eq!(out["file_path"], "a.py");
        let content = out["content"].as_str().unwrap();
        assert!(content.contains("omitted"));
        assert!(!content.contains('x'));
    }

    #[test]
    fn redact_preview_respects_max_chars() {
        let long = format!("sk-ABCDEFGHIJ0123456789xyz {}", "a".repeat(MAX_PREVIEW_CHARS + 50));
        let out = redact_preview(&long);
        assert!(out.chars().count() <= MAX_PREVIEW_CHARS + "…(truncated)".chars().count());
        assert!(out.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn redact_json_value_deep() {
        let v = json!({
            "api_key": "sk-ABCDEFGHIJ0123456789xyz",
            "nested": { "token": "abcdefgh12345678secret" },
            "arr": ["Bearer abcdef0123456789ABCDEF", 1, true],
            "n": 42
        });
        let out = redact_json_value(&v);
        let s = out.to_string();
        assert!(s.contains("[REDACTED_SECRET]"));
        assert!(!s.contains("sk-ABCDEFGHIJ"));
        assert!(!s.contains("abcdefgh12345678secret"));
        assert!(!s.contains("abcdef0123456789ABCDEF"));
        assert_eq!(out["n"], json!(42));
        assert_eq!(out["arr"][1], json!(1));
        assert_eq!(out["arr"][2], json!(true));
    }

    #[test]
    fn redact_json_truncates_long_strings() {
        let long = "x".repeat(MAX_PREVIEW_CHARS + 100);
        let out = redact_json_value(&json!({ "body": long }));
        let body = out["body"].as_str().unwrap();
        assert!(body.contains("…(truncated)"));
    }

    #[test]
    fn redact_json_walks_content_arrays() {
        let value = json!({
            "content": [{ "type": "text", "text": "hi" }]
        });
        let out = redact_json_value(&value);
        assert_eq!(out["content"][0]["text"], "hi");
    }
}

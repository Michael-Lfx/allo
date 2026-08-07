//! Truncation and secret redaction for trace previews.

use nomi_redact::redact_secrets;
use serde_json::{Map, Value};

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
/// Intended for tool arguments and similar structured payloads before they
/// land in span attributes / previews.
///
/// Object keys that look like secret names (`api_key`, `token`, `password`, …)
/// have their string values fully replaced with `[REDACTED_SECRET]`.
pub fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(s) => Value::String(redact_preview(s)),
        Value::Array(items) => Value::Array(items.iter().map(redact_json_value).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
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

fn is_sensitive_key(key: &str) -> bool {
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
}

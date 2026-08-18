//! Capture policy applied **before persist**. Sinks never receive raw payloads.

use base64::Engine;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::redact::{bulky_placeholder, is_sensitive_key, redact_preview, should_omit_bulky};

pub const OMITTED_REASON_BINARY_PAYLOAD: &str = "binary_payload";

/// Walk a canonical request (or any JSON payload): rewrite media to metadata,
/// then redact secrets and truncate strings.
pub fn capture_canonical_request(value: Value) -> Value {
    apply_capture(value)
}

fn apply_capture(value: Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value,
        Value::String(s) => Value::String(redact_preview(&s)),
        Value::Array(items) => Value::Array(items.into_iter().map(apply_capture).collect()),
        Value::Object(map) => apply_capture_object(map),
    }
}

fn apply_capture_object(mut map: Map<String, Value>) -> Value {
    if should_rewrite_media(&map) {
        if let Some(Value::String(data)) = map.remove("data") {
            map.insert("data".into(), media_metadata(&map, &data));
        }
    }

    let mut out = Map::new();
    for (key, value) in map {
        if is_rewritten_media_metadata(&value) {
            out.insert(key, value);
            continue;
        }
        if should_omit_bulky(&key, &value) {
            out.insert(key, bulky_placeholder(&value));
            continue;
        }
        let captured = if is_sensitive_key(&key) {
            match value {
                Value::String(_) => Value::String("[REDACTED_SECRET]".to_owned()),
                other => apply_capture(other),
            }
        } else {
            apply_capture(value)
        };
        out.insert(key, captured);
    }
    Value::Object(out)
}

fn should_rewrite_media(map: &Map<String, Value>) -> bool {
    if matches!(map.get("data"), Some(Value::Object(inner)) if is_rewritten_media_metadata(&Value::Object(inner.clone())))
    {
        return false;
    }
    let has_string_data = matches!(map.get("data"), Some(Value::String(_)));
    if !has_string_data {
        return false;
    }
    map.get("type").and_then(Value::as_str) == Some("image")
        || map.contains_key("media_type")
}

fn is_rewritten_media_metadata(value: &Value) -> bool {
    value.get("omitted_reason").and_then(Value::as_str) == Some(OMITTED_REASON_BINARY_PAYLOAD)
        && value.get("sha256").is_some()
}

fn media_metadata(map: &Map<String, Value>, data: &str) -> Value {
    let mime = map
        .get("media_type")
        .and_then(Value::as_str)
        .or_else(|| map.get("mime").and_then(Value::as_str))
        .unwrap_or("application/octet-stream");
    let (byte_length, digest) = hash_media(data);
    serde_json::json!({
        "mime": mime,
        "byte_length": byte_length,
        "sha256": format!("sha256:{digest}"),
        "omitted_reason": OMITTED_REASON_BINARY_PAYLOAD,
    })
}

fn hash_media(data: &str) -> (u64, String) {
    let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .unwrap_or_else(|_| data.as_bytes().to_vec());
    let digest = Sha256::digest(&bytes);
    (bytes.len() as u64, format!("{digest:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::MAX_PREVIEW_CHARS;
    use serde_json::json;

    #[test]
    fn image_data_becomes_metadata_without_base64() {
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let value = json!({
            "model": "vision",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "media_type": "image/png",
                    "data": png
                }]
            }]
        });
        let out = capture_canonical_request(value);
        let data = &out["messages"][0]["content"][0]["data"];
        assert_eq!(data["omitted_reason"], OMITTED_REASON_BINARY_PAYLOAD);
        assert_eq!(data["mime"], "image/png");
        assert!(data["sha256"].as_str().unwrap().starts_with("sha256:"));
        assert!(data["byte_length"].as_u64().unwrap() > 0);
        let serialized = out.to_string();
        assert!(!serialized.contains(png));
        assert!(!serialized.contains("iVBORw0KGgo"));
    }

    #[test]
    fn capture_is_idempotent_for_rewritten_media() {
        let once = capture_canonical_request(json!({
            "type": "image",
            "media_type": "image/png",
            "data": "aGVsbG8="
        }));
        let twice = capture_canonical_request(once.clone());
        assert_eq!(once, twice);
        assert_eq!(twice["data"]["omitted_reason"], OMITTED_REASON_BINARY_PAYLOAD);
    }

    #[test]
    fn secrets_are_redacted_before_return() {
        let out = capture_canonical_request(json!({
            "api_key": "sk-ABCDEFGHIJ0123456789xyz",
            "body": "ok"
        }));
        assert_eq!(out["api_key"], "[REDACTED_SECRET]");
        assert_eq!(out["body"], "ok");
    }

    #[test]
    fn long_strings_are_truncated() {
        let long = "x".repeat(MAX_PREVIEW_CHARS + 40);
        let out = capture_canonical_request(json!({ "note": long }));
        let note = out["note"].as_str().unwrap();
        assert!(note.contains("…(truncated)"));
    }
}

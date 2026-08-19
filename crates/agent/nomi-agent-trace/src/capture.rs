//! Capture policy applied **before persist**. Sinks never receive raw payloads.

use base64::Engine;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::redact::{
    bulky_placeholder, event_size_limit_placeholder, is_sensitive_key, redact_preview,
    should_omit_bulky, OMITTED_REASON_EVENT_SIZE_LIMIT,
};

pub const OMITTED_REASON_BINARY_PAYLOAD: &str = "binary_payload";
/// Hard cap on one captured event after string/media rewrite. 128 KiB × 128 ≈ 16 MiB.
pub const MAX_EVENT_BYTES: usize = 128 * 1024;

/// Known large fields omitted first when the captured envelope exceeds [`MAX_EVENT_BYTES`].
const SIZE_OMIT_PATHS: &[&[&str]] = &[
    &["request", "tools"],
    &["request", "messages"],
    &["request", "system"],
    &["tools"],
    &["messages"],
    &["system"],
    &["text"],
    &["thinking"],
    &["arguments"],
    &["result"],
    &["response"],
];

/// Walk a canonical request (or any JSON payload): rewrite media to metadata,
/// then redact secrets and truncate strings.
pub fn capture_canonical_request(value: Value) -> Value {
    capture_borrowed(&value)
}

/// Same capture rules as [`capture_canonical_request`], without taking ownership
/// of the source tree. Large strings are truncated into a new tree; they are
/// not cloned first.
pub fn capture_borrowed(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(s) => Value::String(redact_preview(s)),
        Value::Array(items) => Value::Array(items.iter().map(capture_borrowed).collect()),
        Value::Object(map) => capture_borrowed_object(map),
    }
}

/// Capture then enforce the single-event byte budget. Size-limit omit is
/// capture policy (`event_size_limit`) and must not be treated as integrity loss.
pub fn capture_and_size_cap(value: Value) -> Value {
    apply_event_size_budget(capture_borrowed(&value))
}

/// Media stub that does **not** copy or hash payload bytes.
pub fn omitted_binary_payload(mime: &str, byte_length: u64) -> Value {
    serde_json::json!({
        "mime": mime,
        "byte_length": byte_length,
        "sha256": "sha256:not_hashed",
        "omitted_reason": OMITTED_REASON_BINARY_PAYLOAD,
    })
}

pub fn apply_event_size_budget(value: Value) -> Value {
    apply_event_size_budget_with(value, MAX_EVENT_BYTES)
}

pub(crate) fn apply_event_size_budget_with(mut value: Value, max_bytes: usize) -> Value {
    let original_bytes = json_byte_len(&value);
    if original_bytes <= max_bytes {
        return value;
    }
    mark_capture_truncated(&mut value);
    loop {
        let current = json_byte_len(&value);
        if current <= max_bytes {
            return value;
        }
        let Some((path, field_bytes)) = largest_omit_candidate(&value) else {
            return event_size_stub(value, original_bytes);
        };
        omit_at_path(&mut value, &path, field_bytes);
        if json_byte_len(&value) >= current {
            return event_size_stub(value, original_bytes);
        }
    }
}

fn json_byte_len(value: &Value) -> usize {
    serde_json::to_vec(value).map(|bytes| bytes.len()).unwrap_or(0)
}

fn is_size_omitted(value: &Value) -> bool {
    value.get("omitted_reason").and_then(Value::as_str) == Some(OMITTED_REASON_EVENT_SIZE_LIMIT)
}

fn largest_omit_candidate(value: &Value) -> Option<(Vec<String>, usize)> {
    let mut best: Option<(Vec<String>, usize)> = None;
    for path in SIZE_OMIT_PATHS {
        let Some(field) = get_path(value, path) else {
            continue;
        };
        if is_size_omitted(field) {
            continue;
        }
        let field_bytes = json_byte_len(field);
        if field_bytes == 0 {
            continue;
        }
        let placeholder = event_size_limit_placeholder(field_bytes, 0);
        if json_byte_len(&placeholder) >= field_bytes {
            continue;
        }
        if best.as_ref().is_none_or(|(_, size)| field_bytes > *size) {
            best = Some((path.iter().map(|key| (*key).to_owned()).collect(), field_bytes));
        }
    }
    best
}

fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn omit_at_path(value: &mut Value, path: &[String], original_bytes: usize) {
    let placeholder = event_size_limit_placeholder(original_bytes, 0);
    if path.is_empty() {
        *value = placeholder;
        return;
    }
    let mut current = value;
    for key in &path[..path.len() - 1] {
        match current.get_mut(key) {
            Some(next) => current = next,
            None => return,
        }
    }
    if let Some(obj) = current.as_object_mut() {
        obj.insert(path[path.len() - 1].clone(), placeholder);
    }
}

fn mark_capture_truncated(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    match obj.get("capture") {
        Some(Value::Array(items))
            if items.iter().any(|item| item.as_str() == Some("truncated")) => {}
        Some(Value::Array(items)) => {
            let mut items = items.clone();
            items.push(Value::String("truncated".into()));
            obj.insert("capture".into(), Value::Array(items));
        }
        Some(Value::String(flag)) if flag == "truncated" => {}
        Some(other) => {
            obj.insert(
                "capture".into(),
                serde_json::json!(["truncated", other]),
            );
        }
        None => {
            obj.insert("capture".into(), serde_json::json!(["truncated"]));
        }
    }
}

fn event_size_stub(value: Value, original_bytes: usize) -> Value {
    let ids = value.get("ids").cloned().unwrap_or(Value::Null);
    let stub = serde_json::json!({
        "ids": ids,
        "capture": ["truncated"],
        "omitted_reason": OMITTED_REASON_EVENT_SIZE_LIMIT,
        "original_bytes": original_bytes,
        "captured_bytes": 0,
    });
    let captured_bytes = json_byte_len(&stub);
    serde_json::json!({
        "ids": ids,
        "capture": ["truncated"],
        "omitted_reason": OMITTED_REASON_EVENT_SIZE_LIMIT,
        "original_bytes": original_bytes,
        "captured_bytes": captured_bytes,
    })
}

fn capture_borrowed_object(map: &Map<String, Value>) -> Value {
    let rewritten_data = if should_rewrite_media(map) {
        map.get("data")
            .and_then(Value::as_str)
            .map(|data| media_metadata(map, data))
    } else {
        None
    };

    let mut out = Map::new();
    for (key, value) in map {
        if key == "data" {
            if let Some(meta) = rewritten_data.clone() {
                out.insert(key.clone(), meta);
                continue;
            }
        }
        if is_rewritten_media_metadata(value) {
            out.insert(key.clone(), value.clone());
            continue;
        }
        if should_omit_bulky(key, value) {
            out.insert(key.clone(), bulky_placeholder(value));
            continue;
        }
        let captured = if is_sensitive_key(key) {
            match value {
                Value::String(_) => Value::String("[REDACTED_SECRET]".to_owned()),
                other => capture_borrowed(other),
            }
        } else {
            capture_borrowed(value)
        };
        out.insert(key.clone(), captured);
    }
    Value::Object(out)
}

fn should_rewrite_media(map: &Map<String, Value>) -> bool {
    match map.get("data") {
        Some(data) if is_rewritten_media_metadata(data) => return false,
        Some(Value::String(_)) => {}
        _ => return false,
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

    #[test]
    fn capture_borrowed_truncates_and_omits_bulky_without_mutating_source() {
        let long = "x".repeat(MAX_PREVIEW_CHARS + 40);
        let bulky = "y".repeat(10_000);
        let value = json!({
            "note": long,
            "content": bulky,
            "path": "a.py"
        });
        let out = capture_borrowed(&value);
        let note = out["note"].as_str().unwrap();
        assert!(note.contains("…(truncated)"));
        assert_eq!(out["content"], "[10000 omitted]");
        assert_eq!(out["path"], "a.py");
        assert_eq!(
            value["note"].as_str().unwrap().chars().count(),
            MAX_PREVIEW_CHARS + 40
        );
        assert_eq!(value["content"].as_str().unwrap().len(), 10_000);
        assert!(!out.to_string().contains(&"y".repeat(32)));
    }

    fn tool_schema(description_chars: usize, property_count: usize) -> Value {
        let mut properties = serde_json::Map::new();
        for index in 0..property_count {
            properties.insert(
                format!("field_{index}"),
                json!({
                    "type": "string",
                    "description": "x".repeat(description_chars),
                }),
            );
        }
        json!({
            "type": "object",
            "properties": Value::Object(properties),
        })
    }

    fn canonical_request(tool_count: usize, description_chars: usize, property_count: usize) -> Value {
        json!({
            "model": "coding-agent",
            "system": "sys",
            "messages": [{ "role": "user", "content": [{ "type": "text", "text": "hi" }] }],
            "tools": (0..tool_count)
                .map(|index| json!({
                    "name": format!("tool_{index}"),
                    "description": "d".repeat(48),
                    "input_schema": tool_schema(description_chars, property_count),
                    "deferred": false
                }))
                .collect::<Vec<_>>(),
        })
    }

    fn serialized_len(value: &Value) -> usize {
        serde_json::to_vec(value).unwrap().len()
    }

    #[test]
    fn inventory_canonical_request_serialized_sizes() {
        let small = capture_canonical_request(canonical_request(0, 0, 0));
        let typical = capture_canonical_request(canonical_request(5, 40, 6));
        let coding = capture_canonical_request(canonical_request(25, 160, 24));
        let mut sizes = [serialized_len(&small), serialized_len(&typical), serialized_len(&coding)];
        sizes.sort_unstable();
        let p50 = sizes[1];
        let max = sizes[2];
        assert!(
            max > 64 * 1024,
            "coding-agent fixture P95/max must exceed 64 KiB so the default stays 128 KiB; p50={p50} max={max} sizes={sizes:?}"
        );
        assert_eq!(MAX_EVENT_BYTES, 128 * 1024);
        assert!(p50 < MAX_EVENT_BYTES);
    }

    #[test]
    fn event_size_limit_omits_largest_field_and_keeps_ids() {
        let payload = json!({
            "ids": { "conversation_id": "c1", "root_turn_id": "t1" },
            "capture": ["redacted"],
            "request": canonical_request(20, 120, 12)
        });
        let out = apply_event_size_budget_with(payload, 8 * 1024);
        assert!(serialized_len(&out) <= 8 * 1024);
        assert_eq!(out["ids"]["conversation_id"], "c1");
        assert_eq!(out["request"]["tools"]["omitted_reason"], OMITTED_REASON_EVENT_SIZE_LIMIT);
        assert!(out["request"]["tools"]["original_bytes"].as_u64().unwrap() > 0);
        assert_eq!(out["request"]["tools"]["captured_bytes"], 0);
        let capture = out["capture"].as_array().unwrap();
        assert!(capture.iter().any(|item| item.as_str() == Some("truncated")));
        assert!(!out.to_string().contains("input_schema"));
    }

    #[test]
    fn event_size_limit_under_budget_is_unchanged() {
        let payload = json!({ "request": { "model": "m", "messages": [] } });
        let out = apply_event_size_budget(payload.clone());
        assert_eq!(out, payload);
    }
}

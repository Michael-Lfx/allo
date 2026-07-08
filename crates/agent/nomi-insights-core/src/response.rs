//! Parse ops-server batch upload responses (supports common API wrappers).

use crate::types::BatchUploadResponse;

/// Decode `POST .../batch` body into [`BatchUploadResponse`].
///
/// Supports:
/// - Direct `{ "accepted", "duplicates", "rejected" }`
/// - Wrapped `{ "data": { ... } }` / `{ "result": { ... } }`
/// - `{ "code": 0|200, "data": { ... } }` (Flowy-style)
/// - Empty body on HTTP 200 -> treat all contributions as accepted
pub fn parse_batch_upload_response(
    body: &str,
    contribution_count: usize,
) -> Result<BatchUploadResponse, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(fallback_success(contribution_count));
    }

    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid JSON: {e}"))?;

    if let Some(inner) = value.get("data") {
        if inner.is_null() {
            if is_success_code(&value) {
                return Ok(fallback_success(contribution_count));
            }
        } else if let Ok(r) = serde_json::from_value::<BatchUploadResponse>(inner.clone()) {
            return Ok(r);
        }
    }

    if let Some(inner) = value.get("result") {
        if let Ok(r) = serde_json::from_value::<BatchUploadResponse>(inner.clone()) {
            return Ok(r);
        }
    }

    if looks_like_batch_stats(&value) {
        return serde_json::from_value::<BatchUploadResponse>(value).map_err(|e| e.to_string());
    }

    if is_success_code(&value) {
        return Ok(fallback_success(contribution_count));
    }

    Err(format!(
        "unrecognized batch response shape (preview): {}",
        preview_body(trimmed, 400)
    ))
}

fn looks_like_batch_stats(value: &serde_json::Value) -> bool {
    let obj = match value.as_object() {
        Some(o) => o,
        None => return false,
    };
    obj.contains_key("accepted")
        || obj.contains_key("accepted_count")
        || obj.contains_key("acceptedCount")
        || obj.contains_key("duplicates")
        || obj.contains_key("duplicate_count")
        || obj.contains_key("duplicateCount")
        || obj.contains_key("rejected")
}

fn is_success_code(value: &serde_json::Value) -> bool {
    let Some(code) = value.get("code") else {
        return value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    };
    matches!(
        code,
        serde_json::Value::Number(n) if n.as_i64() == Some(0) || n.as_i64() == Some(200)
    ) || code == &serde_json::Value::String("0".into())
        || code == &serde_json::Value::String("200".into())
        || code == &serde_json::Value::String("success".into())
}

fn fallback_success(contribution_count: usize) -> BatchUploadResponse {
    BatchUploadResponse {
        accepted: contribution_count as u32,
        duplicates: 0,
        rejected: vec![],
    }
}

fn preview_body(body: &str, max_len: usize) -> String {
    if body.len() <= max_len {
        body.to_string()
    } else {
        format!("{}...", &body[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_shape() {
        let r = parse_batch_upload_response(r#"{"accepted":2,"duplicates":1,"rejected":[]}"#, 3)
            .unwrap();
        assert_eq!(r.accepted, 2);
        assert_eq!(r.duplicates, 1);
    }

    #[test]
    fn data_wrapper() {
        let r = parse_batch_upload_response(
            r#"{"code":0,"message":"ok","data":{"accepted":1,"duplicates":0,"rejected":[]}}"#,
            1,
        )
        .unwrap();
        assert_eq!(r.accepted, 1);
    }

    #[test]
    fn empty_body_fallback() {
        let r = parse_batch_upload_response("", 5).unwrap();
        assert_eq!(r.accepted, 5);
    }

    #[test]
    fn code_success_without_stats() {
        let r = parse_batch_upload_response(r#"{"code":200,"msg":"ok","data":null}"#, 3).unwrap();
        assert_eq!(r.accepted, 3);
    }
}

use nomi_mcp::protocol::McpToolResult;
use serde_json::Value;

use super::super::MAX_SEARCH_COUNT;
use super::shared::{build_hit, normalize_hits, parsed_text_value, string_field};
use super::{DecodeError, NormalizedSearchHit};

pub(crate) fn decode_parallel(
    result: &McpToolResult,
    count: usize,
) -> Result<Vec<crate::types::SearchHit>, DecodeError> {
    let value = result
        .structured_content
        .clone()
        .or_else(|| parsed_text_value(result));
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or(DecodeError::MalformedResponse)?;
    if results.is_empty() {
        return Ok(Vec::new());
    }

    let mut hits = Vec::<NormalizedSearchHit>::new();
    for (index, item) in results.iter().enumerate() {
        let object = item.as_object().ok_or(DecodeError::MalformedResponse)?;
        let title = string_field(object, &["title"]);
        let url = string_field(object, &["url"]);
        let excerpts = object
            .get("excerpts")
            .and_then(Value::as_array)
            .ok_or(DecodeError::MalformedResponse)?;
        let evidence = excerpts
            .iter()
            .map(|excerpt| {
                excerpt
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(DecodeError::MalformedResponse)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(hit) = build_hit(
            title,
            url,
            object.get("publish_date").and_then(Value::as_str),
            evidence,
            index,
        ) {
            hits.push(hit);
        }
    }
    if hits.is_empty() {
        return Err(DecodeError::MalformedResponse);
    }
    Ok(normalize_hits(
        hits,
        count.clamp(1, MAX_SEARCH_COUNT as usize),
    ))
}

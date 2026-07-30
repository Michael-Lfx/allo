use nomi_mcp::protocol::McpToolResult;
use serde_json::Value;

use super::super::MAX_SEARCH_COUNT;
use super::shared::{build_hit, normalize_hits, parsed_text_value, string_field};
use super::{DecodeError, DecodeOutcome, DecodeSource, NormalizedSearchHit};

pub(crate) fn decode_parallel(
    result: &McpToolResult,
    count: usize,
) -> Result<DecodeOutcome, DecodeError> {
    let (value, source) = match result.structured_content.clone() {
        Some(value) => (Some(value), DecodeSource::Structured),
        None => (parsed_text_value(result), DecodeSource::TextJsonFallback),
    };
    let Some(value) = value else {
        return Ok(DecodeOutcome {
            hits: Vec::new(),
            source,
            dropped_items: 0,
            contract_degraded: false,
            structured_fallback: false,
        });
    };
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or(DecodeError::MalformedResponse)?;
    if results.is_empty() {
        return Ok(DecodeOutcome {
            hits: Vec::new(),
            source,
            dropped_items: 0,
            contract_degraded: false,
            structured_fallback: false,
        });
    }

    let mut hits = Vec::<NormalizedSearchHit>::new();
    let mut dropped_items = 0;
    for (index, item) in results.iter().enumerate() {
        let Some(object) = item.as_object() else {
            dropped_items += 1;
            continue;
        };
        let title = string_field(object, &["title"]);
        let url = string_field(object, &["url"]);
        let Some(excerpts) = object.get("excerpts").and_then(Value::as_array) else {
            dropped_items += 1;
            continue;
        };
        let Some(evidence) = excerpts
            .iter()
            .map(Value::as_str)
            .map(|excerpt| excerpt.map(str::to_owned))
            .collect::<Option<Vec<_>>>()
        else {
            dropped_items += 1;
            continue;
        };
        if let Some(hit) = build_hit(
            title,
            url,
            object.get("publish_date").and_then(Value::as_str),
            evidence,
            index,
        ) {
            hits.push(hit);
        } else {
            dropped_items += 1;
        }
    }
    if hits.is_empty() {
        return Err(DecodeError::MalformedResponse);
    }
    Ok(DecodeOutcome {
        hits: normalize_hits(hits, count.clamp(1, MAX_SEARCH_COUNT as usize)),
        source,
        dropped_items,
        contract_degraded: dropped_items > 0,
        structured_fallback: false,
    })
}

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate};
use nomi_mcp::protocol::{McpContent, McpToolResult};
use serde_json::Value;
use url::Url;

use crate::types::{MAX_SEARCH_COUNT, SearchHit};

pub(crate) const MAX_EVIDENCE_FRAGMENTS: usize = 4;
pub(crate) const MAX_EVIDENCE_CHARS: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedSearchHit {
    pub(crate) title: String,
    pub(crate) url: Url,
    pub(crate) published_at: Option<String>,
    pub(crate) evidence_fragments: Vec<String>,
    pub(crate) original_rank: usize,
}

pub(crate) fn text_blocks(result: &McpToolResult) -> Vec<&str> {
    result
        .content
        .iter()
        .filter_map(|content| match content {
            McpContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

pub(crate) fn parsed_text_value(result: &McpToolResult) -> Option<Value> {
    text_blocks(result).into_iter().find_map(|text| {
        let text = text.trim();
        (!text.is_empty())
            .then(|| serde_json::from_str::<Value>(text).ok())
            .flatten()
    })
}

pub(crate) fn normalize_url(raw: &str) -> Option<Url> {
    let mut url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }

    let scheme = url.scheme().to_ascii_lowercase();
    url.set_scheme(&scheme).ok()?;

    let host = url.host_str()?.to_ascii_lowercase();
    url.set_host(Some(&host)).ok()?;

    let default_port = match scheme.as_str() {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    };
    if url.port().is_some() && url.port() == default_port {
        url.set_port(None).ok()?;
    }
    url.set_fragment(None);
    Some(url)
}

pub(crate) fn normalize_date(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if NaiveDate::parse_from_str(raw, "%Y-%m-%d").is_ok()
        || DateTime::parse_from_rfc3339(raw).is_ok()
    {
        Some(raw.to_owned())
    } else {
        None
    }
}

pub(crate) fn build_hit(
    title: Option<&str>,
    url: Option<&str>,
    published_at: Option<&str>,
    evidence: impl IntoIterator<Item = String>,
    original_rank: usize,
) -> Option<NormalizedSearchHit> {
    let title = title
        .map(normalize_text)
        .filter(|title| !title.is_empty())?;
    let url = normalize_url(url?)?;
    let evidence_fragments = evidence
        .into_iter()
        .filter_map(|fragment| {
            let fragment = normalize_text(&fragment);
            (!fragment.is_empty()).then(|| truncate_chars(fragment, MAX_EVIDENCE_CHARS))
        })
        .take(MAX_EVIDENCE_FRAGMENTS)
        .collect();
    Some(NormalizedSearchHit {
        title,
        url,
        published_at: normalize_date(published_at),
        evidence_fragments,
        original_rank,
    })
}

pub(crate) fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(mut value: String, limit: usize) -> String {
    if value.chars().count() > limit {
        value = value.chars().take(limit).collect();
    }
    value
}

pub(crate) fn string_field<'a>(object: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn evidence_values(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Vec<String> {
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        let values = match value {
            Value::String(text) => vec![text.clone()],
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        if !values.is_empty() {
            return values;
        }
    }
    Vec::new()
}

pub(crate) fn normalize_hits(
    hits: Vec<NormalizedSearchHit>,
    limit: usize,
) -> Vec<SearchHit> {
    let mut indexes = HashMap::<String, usize>::new();
    let mut merged = Vec::<NormalizedSearchHit>::new();

    for mut hit in hits {
        let key = hit.url.as_str().to_owned();
        if let Some(index) = indexes.get(&key).copied() {
            let existing = &mut merged[index];
            if existing.published_at.is_none() {
                existing.published_at = hit.published_at.take();
            }
            for evidence in hit.evidence_fragments {
                if existing.evidence_fragments.len() >= MAX_EVIDENCE_FRAGMENTS {
                    break;
                }
                if !existing.evidence_fragments.contains(&evidence) {
                    existing.evidence_fragments.push(evidence);
                }
            }
            continue;
        }
        indexes.insert(key, merged.len());
        merged.push(hit);
    }

    merged.sort_by_key(|hit| hit.original_rank);
    merged
        .into_iter()
        .take(limit.clamp(1, MAX_SEARCH_COUNT as usize))
        .enumerate()
        .map(|(index, hit)| SearchHit {
            title: hit.title,
            url: hit.url.to_string(),
            snippet: hit.evidence_fragments.join("\n"),
            published_at: hit.published_at,
            rank: index as u32 + 1,
        })
        .collect()
}

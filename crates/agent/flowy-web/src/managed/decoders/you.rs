use nomi_mcp::protocol::McpToolResult;
use serde_json::Value;

use super::super::MAX_SEARCH_COUNT;
use super::shared::{
    build_hit, evidence_values, normalize_hits, parsed_text_value, string_field, text_blocks,
};
use super::{DecodeError, NormalizedSearchHit};

#[derive(Clone, Copy)]
enum Section {
    Web,
    News,
}

pub(crate) fn decode_you(
    result: &McpToolResult,
    count: usize,
) -> Result<Vec<crate::types::SearchHit>, DecodeError> {
    let has_structured_content = result.structured_content.is_some();
    if let Some(value) = result.structured_content.as_ref()
        && let Ok(hits) = decode_value(value)
    {
        return Ok(normalize_hits(hits, count.clamp(1, MAX_SEARCH_COUNT as usize)));
    }

    if let Some(value) = parsed_text_value(result)
        && let Ok(hits) = decode_value(&value)
    {
        return Ok(normalize_hits(hits, count.clamp(1, MAX_SEARCH_COUNT as usize)));
    }

    let text = text_blocks(result).join("\n");
    if text.trim().is_empty() {
        return if has_structured_content {
            Err(DecodeError::MalformedResponse)
        } else {
            Ok(Vec::new())
        };
    }
    let hits = decode_labelled_text(&text)?;
    Ok(normalize_hits(hits, count.clamp(1, MAX_SEARCH_COUNT as usize)))
}

fn decode_value(value: &Value) -> Result<Vec<NormalizedSearchHit>, DecodeError> {
    if let Some(items) = value.as_array() {
        return decode_items(items, 0);
    }
    let results = value
        .get("results")
        .ok_or(DecodeError::MalformedResponse)?;
    if let Some(sections) = results.as_object() {
        let web = sections.get("web").and_then(Value::as_array);
        let news = sections.get("news").and_then(Value::as_array);
        if web.is_none() && news.is_none() {
            return Err(DecodeError::MalformedResponse);
        }
        return decode_interleaved(web, news);
    }
    let items = results.as_array().ok_or(DecodeError::MalformedResponse)?;
    decode_items(items, 0)
}

fn decode_interleaved(
    web: Option<&Vec<Value>>,
    news: Option<&Vec<Value>>,
) -> Result<Vec<NormalizedSearchHit>, DecodeError> {
    let web_len = web.map_or(0, Vec::len);
    let news_len = news.map_or(0, Vec::len);
    let mut hits = Vec::new();
    for index in 0..web_len.max(news_len) {
        if let Some(item) = web.and_then(|items| items.get(index)) {
            hits.push(decode_item(item, hits.len())?);
        }
        if let Some(item) = news.and_then(|items| items.get(index)) {
            hits.push(decode_item(item, hits.len())?);
        }
    }
    Ok(hits)
}

fn decode_items(items: &[Value], rank_offset: usize) -> Result<Vec<NormalizedSearchHit>, DecodeError> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| decode_item(item, rank_offset + index))
        .collect()
}

fn decode_item(value: &Value, rank: usize) -> Result<NormalizedSearchHit, DecodeError> {
    let object = value.as_object().ok_or(DecodeError::MalformedResponse)?;
    let title = string_field(object, &["title", "Title", "name", "Name"]);
    let url = string_field(object, &["url", "URL", "link", "Link"]);
    let evidence = evidence_values(
        object,
        &[
            "snippets",
            "Snippets",
            "excerpts",
            "Excerpts",
            "description",
            "Description",
            "snippet",
            "Snippet",
            "summary",
            "Summary",
        ],
    );
    build_hit(
        title,
        url,
        string_field(object, &["published", "Published", "published_at"]),
        evidence,
        rank,
    )
    .ok_or(DecodeError::MalformedResponse)
}

fn decode_labelled_text(text: &str) -> Result<Vec<NormalizedSearchHit>, DecodeError> {
    let mut section = Section::Web;
    let mut current: Option<LabelledRecord> = None;
    let mut web = Vec::new();
    let mut news = Vec::new();
    let mut recognized = false;
    let mut collecting_snippets = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let uppercase = trimmed.to_ascii_uppercase();
        if uppercase.contains("WEB RESULTS") {
            finish_record(&mut current, &mut web)?;
            section = Section::Web;
            collecting_snippets = false;
            recognized = true;
            continue;
        }
        if uppercase.contains("NEWS RESULTS") {
            finish_record(&mut current, &mut news)?;
            section = Section::News;
            collecting_snippets = false;
            recognized = true;
            continue;
        }
        if let Some(value) = labelled_value(trimmed, "Title") {
            finish_record_for_section(&mut current, section, &mut web, &mut news)?;
            current = Some(LabelledRecord {
                title: value.to_owned(),
                ..LabelledRecord::default()
            });
            collecting_snippets = false;
            recognized = true;
        } else if let Some(value) = labelled_value(trimmed, "URL") {
            if let Some(record) = current.as_mut() {
                record.url = Some(value.to_owned());
            }
            collecting_snippets = false;
            recognized = true;
        } else if let Some(value) = labelled_value(trimmed, "Description") {
            if let Some(record) = current.as_mut() {
                record.evidence.push(value.to_owned());
            }
            collecting_snippets = false;
            recognized = true;
        } else if let Some(value) = labelled_value(trimmed, "Published") {
            if let Some(record) = current.as_mut() {
                record.published = Some(value.to_owned());
            }
            collecting_snippets = false;
            recognized = true;
        } else if trimmed.eq_ignore_ascii_case("Snippets:") {
            collecting_snippets = true;
            recognized = true;
        } else if collecting_snippets {
            let value = trimmed
                .trim_start_matches(['-', '*', '•'])
                .trim();
            if let Some(record) = current.as_mut()
                && !value.is_empty()
            {
                record.evidence.push(value.to_owned());
            }
        }
    }
    finish_record_for_section(&mut current, section, &mut web, &mut news)?;
    if !recognized || web.is_empty() && news.is_empty() {
        return Err(DecodeError::MalformedResponse);
    }

    let mut interleaved = Vec::new();
    for index in 0..web.len().max(news.len()) {
        if let Some(hit) = web.get(index) {
            interleaved.push(hit.clone());
        }
        if let Some(hit) = news.get(index) {
            interleaved.push(hit.clone());
        }
    }
    Ok(interleaved)
}

#[derive(Default)]
struct LabelledRecord {
    title: String,
    url: Option<String>,
    published: Option<String>,
    evidence: Vec<String>,
}

fn finish_record(
    current: &mut Option<LabelledRecord>,
    target: &mut Vec<NormalizedSearchHit>,
) -> Result<(), DecodeError> {
    if let Some(record) = current.take() {
        if record.title.trim().is_empty() && record.url.is_none() {
            return Ok(());
        }
        let hit = build_hit(
            Some(&record.title),
            record.url.as_deref(),
            record.published.as_deref(),
            record.evidence,
            target.len(),
        )
        .ok_or(DecodeError::MalformedResponse)?;
        target.push(hit);
    }
    Ok(())
}

fn finish_record_for_section(
    current: &mut Option<LabelledRecord>,
    section: Section,
    web: &mut Vec<NormalizedSearchHit>,
    news: &mut Vec<NormalizedSearchHit>,
) -> Result<(), DecodeError> {
    match section {
        Section::Web => finish_record(current, web),
        Section::News => finish_record(current, news),
    }
}

fn labelled_value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let (prefix, value) = line.split_once(':')?;
    prefix.trim().eq_ignore_ascii_case(label).then_some(value.trim())
}

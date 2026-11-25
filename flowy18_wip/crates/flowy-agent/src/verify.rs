//! Deterministic delivery-time verification for `.pptx`, `.docx`, and `.xlsx`.
//!
//! All three formats share one [`VerifyReport`] contract, one four-code anchor
//! vocabulary (`ANCHOR_MISSING`, `ANCHOR_DUPLICATE`, `ANCHOR_INVALID`,
//! `VALUE_MISMATCH`, plus L1 `STRUCTURE`), host-owned [`BriefContract`] checks,
//! and one persistence/telemetry path into [`DeliveryStore`].

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{ToolCallArgs, ToolExecutionResult, ToolRunContext};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::delivery::{self, DeliveryStore};
use crate::pptx::resolve_turn_path;
use crate::semantic_verify::{
    SemanticContext, SemanticOutcome, probe_deck_semantics,
};

pub const ANCHOR_MISSING: &str = "ANCHOR_MISSING";
pub const ANCHOR_DUPLICATE: &str = "ANCHOR_DUPLICATE";
pub const ANCHOR_INVALID: &str = "ANCHOR_INVALID";
pub const VALUE_MISMATCH: &str = "VALUE_MISMATCH";
pub const STRUCTURE: &str = "STRUCTURE";

/// Safety bound per decompressed XML entry inside an OOXML archive (16 MiB).
const MAX_XML_ENTRY_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyIssue {
    pub code: &'static str,
    pub field: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub artifact: String,
    pub anchor: &'static str,
    pub ok: bool,
    pub issues: Vec<VerifyIssue>,
}

impl VerifyReport {
    #[must_use]
    pub fn to_value_with_meta(
        &self,
        summary: &str,
        report_id: &str,
        artifact_hash: &str,
        semantic_hint: Option<&str>,
    ) -> Value {
        json!({
            "summary": summary,
            "report_id": report_id,
            "artifact": self.artifact,
            "artifact_hash": artifact_hash,
            "anchor": self.anchor,
            "ok": self.ok,
            "next_action": next_action_for(self),
            "semantic_hint": semantic_hint,
            "issues": self.issues.iter().map(|issue| json!({
                "code": issue.code,
                "field": issue.field,
                "ui_key": ui_key(issue.code),
            })).collect::<Vec<_>>(),
        })
    }
}

#[must_use]
pub fn ui_key(code: &str) -> &'static str {
    match code {
        ANCHOR_MISSING => "verify.anchor_missing",
        ANCHOR_DUPLICATE => "verify.anchor_duplicate",
        ANCHOR_INVALID => "verify.anchor_invalid",
        VALUE_MISMATCH => "verify.value_mismatch",
        _ => "verify.structure",
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeckAnchor {
    pub anchor: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug)]
struct SlideL1Stats {
    index: usize,
    has_title: bool,
    has_body: bool,
    non_empty_shapes: usize,
}

/// Host-owned literal constraints extracted from the user brief (§6.6 & §6.7).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BriefContract {
    min_slides: Option<usize>,
    expected_filenames: Vec<String>,
}

impl BriefContract {
    #[must_use]
    pub fn from_brief(brief: &str) -> Self {
        Self {
            min_slides: extract_explicit_slide_count(brief),
            expected_filenames: extract_explicit_filenames(brief),
        }
    }

    fn check_filename(
        &self,
        artifact_path: &Path,
        ext: &str,
        issues: &mut Vec<VerifyIssue>,
    ) {
        let suffix = format!(".{}", ext.to_ascii_lowercase());
        let matches: Vec<&str> = self
            .expected_filenames
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(&suffix))
            .map(String::as_str)
            .collect();
        if matches.len() != 1 {
            return;
        }
        let actual = artifact_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !actual.eq_ignore_ascii_case(matches[0]) {
            push_unique(issues, STRUCTURE, "brief_filename");
        }
    }

    fn check_deck(
        &self,
        artifact_path: &Path,
        actual_slides: usize,
        declared_slides: usize,
        per_slide: &[SlideL1Stats],
        anchor_count: usize,
        issues: &mut Vec<VerifyIssue>,
    ) {
        if let Some(min) = self.min_slides
            && (actual_slides < min || declared_slides < min)
        {
            push_unique(issues, STRUCTURE, "brief_min_slides");
        }
        self.check_filename(artifact_path, "pptx", issues);
        if anchor_count == 0 {
            push_unique(issues, STRUCTURE, "anchors");
        }
        for slide in per_slide {
            if slide.non_empty_shapes == 0 {
                push_unique(
                    issues,
                    STRUCTURE,
                    &format!("slide_{}_empty", slide.index),
                );
            } else {
                if !slide.has_title {
                    push_unique(
                        issues,
                        STRUCTURE,
                        &format!("slide_{}_title", slide.index),
                    );
                }
                if !slide.has_body {
                    push_unique(
                        issues,
                        STRUCTURE,
                        &format!("slide_{}_body", slide.index),
                    );
                }
            }
        }
    }
}

fn next_action_for(report: &VerifyReport) -> &'static str {
    if report.ok {
        "publish"
    } else if !report.issues.is_empty()
        && report
            .issues
            .iter()
            .all(|issue| issue.code == VALUE_MISMATCH)
    {
        "rerender_once"
    } else {
        "clarify_or_rebuild"
    }
}

fn extract_explicit_slide_count(brief: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = brief.char_indices().collect();
    let mut best: Option<usize> = None;
    for idx in 0..chars.len() {
        // Skip ordinals ("第1页"/"第3页") so references to a specific slide are
        // never mistaken for a total slide count.
        if idx > 0 && chars[idx - 1].1 == '第' {
            continue;
        }
        let (byte_pos, ch) = chars[idx];
        let (num, end_idx, allow_en) = if ch.is_ascii_digit() {
            if idx > 0 && chars[idx - 1].1.is_ascii_digit() {
                continue;
            }
            let mut end = idx;
            while end < chars.len() && chars[end].1.is_ascii_digit() {
                end += 1;
            }
            let end_byte = chars.get(end).map_or(brief.len(), |(pos, _)| *pos);
            let Ok(num) = brief[byte_pos..end_byte].parse::<usize>() else {
                continue;
            };
            if !(1..=50).contains(&num) {
                continue;
            }
            (num, end, true)
        } else if let Some((num, len)) = parse_chinese_numeral(&chars[idx..]) {
            (num, idx + len, false)
        } else {
            continue;
        };
        let next_byte = chars.get(end_idx).map_or(brief.len(), |(pos, _)| *pos);
        let rest = brief[next_byte..].trim_start_matches([' ', '\t', '-']);
        let is_cn_unit = rest.starts_with('页')
            || rest.starts_with("张PPT")
            || rest.starts_with("张ppt")
            || rest.starts_with("张幻灯片");
        let is_en_unit = allow_en && {
            let lower = rest.to_ascii_lowercase();
            ["slides", "slide"].iter().any(|unit| {
                lower.strip_prefix(unit).is_some_and(|after| {
                    after
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_ascii_alphanumeric())
                })
            })
        };
        if is_cn_unit || is_en_unit {
            best = Some(best.map_or(num, |prev| prev.max(num)));
        }
    }
    best
}

fn parse_chinese_numeral(slice: &[(usize, char)]) -> Option<(usize, usize)> {
    match (slice.first()?.1, slice.get(1).map(|(_, ch)| *ch)) {
        ('十', Some('一')) => Some((11, 2)),
        ('十', Some('二')) => Some((12, 2)),
        ('十', _) => Some((10, 1)),
        ('一', _) => Some((1, 1)),
        ('二' | '两', _) => Some((2, 1)),
        ('三', _) => Some((3, 1)),
        ('四', _) => Some((4, 1)),
        ('五', _) => Some((5, 1)),
        ('六', _) => Some((6, 1)),
        ('七', _) => Some((7, 1)),
        ('八', _) => Some((8, 1)),
        ('九', _) => Some((9, 1)),
        _ => None,
    }
}

fn extract_explicit_filenames(brief: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in brief.split(|ch: char| {
        !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    }) {
        let candidate = token.trim_matches('.');
        let lower = candidate.to_ascii_lowercase();
        let is_target = [".pptx", ".docx", ".xlsx"]
            .iter()
            .any(|ext| lower.ends_with(ext) && lower.len() > ext.len());
        if is_target
            && !out
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(candidate))
        {
            out.push(candidate.to_owned());
        }
    }
    out
}

fn push_unique(issues: &mut Vec<VerifyIssue>, code: &'static str, field: &str) {
    if !issues
        .iter()
        .any(|issue| issue.code == code && issue.field == field)
    {
        issues.push(VerifyIssue {
            code,
            field: field.to_owned(),
        });
    }
}

fn evaluate_pairs(
    observed: &[(String, String)],
    expected: &[DeckAnchor],
    flag_extra: bool,
    issues: &mut Vec<VerifyIssue>,
) {
    let mut seen: Vec<&str> = Vec::with_capacity(expected.len());
    for anchor in expected {
        let matches: Vec<&str> = observed
            .iter()
            .filter(|(name, _)| name == &anchor.anchor)
            .map(|(_, value)| value.as_str())
            .collect();
        match matches.as_slice() {
            [] => push_unique(issues, ANCHOR_MISSING, &anchor.anchor),
            [actual] if *actual != anchor.value => {
                push_unique(issues, VALUE_MISMATCH, &anchor.anchor);
            }
            [_] => {}
            _ => push_unique(issues, ANCHOR_DUPLICATE, &anchor.anchor),
        }
        seen.push(&anchor.anchor);
    }
    if flag_extra {
        for (name, _) in observed {
            if !seen.contains(&name.as_str()) {
                push_unique(issues, ANCHOR_INVALID, name);
            }
        }
    }
}

fn evaluate(
    artifact: &str,
    slide_count: usize,
    expected_slides: usize,
    deck: &[(String, String)],
    expected: &[DeckAnchor],
) -> VerifyReport {
    let mut issues = Vec::new();
    if slide_count != expected_slides {
        push_unique(&mut issues, STRUCTURE, "slides");
    }
    evaluate_pairs(deck, expected, true, &mut issues);
    VerifyReport {
        artifact: artifact.to_owned(),
        anchor: "shape_name",
        ok: issues.is_empty(),
        issues,
    }
}

struct VerifiedDeck {
    report: VerifyReport,
    bytes: Vec<u8>,
    shapes: Vec<(String, String)>,
}

fn verify_deck_file_with_shapes(
    path: &Path,
    expected_slides: usize,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedDeck, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let presentation = rpptx::Presentation::from_bytes(&bytes)
        .map_err(|error| error.to_string())?;
    let slides: Vec<rpptx::SlideRef<'_>> = presentation.slides().collect();
    let mut deck = Vec::new();
    let mut per_slide = Vec::with_capacity(slides.len());
    for (index, slide) in slides.iter().enumerate() {
        let mut has_title = false;
        let mut has_body = false;
        let mut non_empty_shapes = 0_usize;
        for shape in slide.shapes() {
            if let Some(name) = shape.non_visual_name() {
                let text = shape.text().unwrap_or_default();
                if !text.trim().is_empty() {
                    non_empty_shapes += 1;
                    if name.to_ascii_lowercase().contains("title") {
                        has_title = true;
                    } else {
                        has_body = true;
                    }
                    deck.push((name, text));
                }
            }
        }
        per_slide.push(SlideL1Stats {
            index,
            has_title,
            has_body,
            non_empty_shapes,
        });
    }
    let mut report = evaluate(
        &path.display().to_string(),
        slides.len(),
        expected_slides,
        &deck,
        expected,
    );
    let default_contract = BriefContract::default();
    contract.unwrap_or(&default_contract).check_deck(
        path,
        slides.len(),
        expected_slides,
        &per_slide,
        expected.len(),
        &mut report.issues,
    );
    report.ok = report.issues.is_empty();
    Ok(VerifiedDeck {
        report,
        bytes,
        shapes: deck,
    })
}

fn read_zip_xml(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    entry_name: &str,
) -> Option<String> {
    let entry = archive.by_name(entry_name).ok()?;
    let mut buf = Vec::new();
    entry
        .take(MAX_XML_ENTRY_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() as u64 > MAX_XML_ENTRY_BYTES {
        return None;
    }
    String::from_utf8(buf).ok()
}

fn verify_docx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<(VerifyReport, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut issues = Vec::new();
    if let Some(contract) = contract {
        contract.check_filename(path, "docx", &mut issues);
    }
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
    else {
        push_unique(&mut issues, STRUCTURE, "docx_archive");
        return Ok((report_for(path, "bookmark", issues), bytes));
    };
    let Some(xml) = read_zip_xml(&mut archive, "word/document.xml") else {
        push_unique(&mut issues, STRUCTURE, "docx_document");
        return Ok((report_for(path, "bookmark", issues), bytes));
    };
    let Ok(doc) = roxmltree::Document::parse(&xml) else {
        push_unique(&mut issues, STRUCTURE, "docx_document");
        return Ok((report_for(path, "bookmark", issues), bytes));
    };
    let mut non_empty_paragraphs = 0_usize;
    let mut bookmarks: Vec<(String, String)> = Vec::new();
    for para in doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "p")
    {
        let mut text = String::new();
        for node in para.descendants().filter(roxmltree::Node::is_element) {
            match node.tag_name().name() {
                "t" => {
                    if let Some(chunk) = node.text() {
                        text.push_str(chunk);
                    }
                }
                "tab" => text.push('\t'),
                "br" => text.push('\n'),
                _ => {}
            }
        }
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            non_empty_paragraphs += 1;
        }
        for bm in para.descendants().filter(|node| {
            node.is_element() && node.tag_name().name() == "bookmarkStart"
        }) {
            if let Some(name) = bm
                .attributes()
                .find(|attr| attr.name() == "name")
                .map(|attr| attr.value())
                && !name.is_empty()
                && !name.starts_with('_')
            {
                bookmarks.push((name.to_owned(), trimmed.to_owned()));
            }
        }
    }
    if non_empty_paragraphs == 0 {
        push_unique(&mut issues, STRUCTURE, "docx_empty");
    }
    if !expected.is_empty() {
        evaluate_pairs(&bookmarks, expected, false, &mut issues);
    }
    Ok((report_for(path, "bookmark", issues), bytes))
}

fn verify_xlsx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<(VerifyReport, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut issues = Vec::new();
    if let Some(contract) = contract {
        contract.check_filename(path, "xlsx", &mut issues);
    }
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
    else {
        push_unique(&mut issues, STRUCTURE, "xlsx_archive");
        return Ok((report_for(path, "row_key", issues), bytes));
    };
    let Some(wb_xml) = read_zip_xml(&mut archive, "xl/workbook.xml") else {
        push_unique(&mut issues, STRUCTURE, "xlsx_workbook");
        return Ok((report_for(path, "row_key", issues), bytes));
    };
    if roxmltree::Document::parse(&wb_xml).is_err() {
        push_unique(&mut issues, STRUCTURE, "xlsx_workbook");
        return Ok((report_for(path, "row_key", issues), bytes));
    }
    let shared_strings: Vec<String> =
        read_zip_xml(&mut archive, "xl/sharedStrings.xml")
            .and_then(|xml| {
                let doc = roxmltree::Document::parse(&xml).ok()?;
                Some(
                    doc.descendants()
                        .filter(|node| {
                            node.is_element() && node.tag_name().name() == "si"
                        })
                        .map(|si| {
                            si.descendants()
                                .filter(|node| {
                                    node.is_element()
                                        && node.tag_name().name() == "t"
                                })
                                .filter_map(|node| node.text())
                                .collect::<String>()
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();

    let mut sheet_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_owned()))
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.starts_with("xl/worksheets/sheet")
                && Path::new(&lower)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        })
        .collect();
    sheet_names.sort();
    if sheet_names.is_empty() {
        push_unique(&mut issues, STRUCTURE, "xlsx_sheets");
        return Ok((report_for(path, "row_key", issues), bytes));
    }

    let mut non_empty_rows = 0_usize;
    let mut cells: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut row_summaries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut row_col_values: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for sheet_name in sheet_names {
        let Some(sheet_xml) = read_zip_xml(&mut archive, &sheet_name) else {
            push_unique(&mut issues, STRUCTURE, "xlsx_sheet");
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&sheet_xml) else {
            push_unique(&mut issues, STRUCTURE, "xlsx_sheet");
            continue;
        };
        let mut headers: BTreeMap<String, String> = BTreeMap::new();
        for (row_idx, row) in doc
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "row")
            .enumerate()
        {
            let mut row_cells: Vec<(String, String, String)> = Vec::new();
            for (col_idx, cell) in row
                .children()
                .filter(|node| {
                    node.is_element() && node.tag_name().name() == "c"
                })
                .enumerate()
            {
                let cell_ref = cell.attribute("r").map_or_else(
                    || format!("{}{}", column_letters(col_idx), row_idx + 1),
                    str::to_ascii_uppercase,
                );
                let col: String = cell_ref
                    .chars()
                    .take_while(char::is_ascii_alphabetic)
                    .collect();
                let raw = match cell.attribute("t") {
                    Some("s") => cell
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "v")
                        .and_then(|n| n.text())
                        .and_then(|s| s.trim().parse::<usize>().ok())
                        .and_then(|idx| shared_strings.get(idx))
                        .cloned()
                        .unwrap_or_default(),
                    Some("inlineStr") => cell
                        .descendants()
                        .filter(|n| {
                            n.is_element() && n.tag_name().name() == "t"
                        })
                        .filter_map(|n| n.text())
                        .collect::<String>(),
                    _ => cell
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "v")
                        .and_then(|n| n.text())
                        .unwrap_or_default()
                        .to_owned(),
                };
                let value = raw.trim().to_owned();
                if !value.is_empty() {
                    row_cells.push((col, cell_ref, value));
                }
            }
            if row_cells.is_empty() {
                continue;
            }
            non_empty_rows += 1;
            if headers.is_empty() {
                for (col, _, value) in &row_cells {
                    headers.insert(col.clone(), value.clone());
                }
            }
            let row_key = row_cells[0].2.clone();
            let summary = row_cells
                .iter()
                .map(|(_, _, val)| val.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            row_summaries
                .entry(row_key.clone())
                .or_default()
                .push(summary);
            for (col, cell_ref, value) in row_cells {
                cells.entry(cell_ref).or_default().push(value.clone());
                if let Some(header) = headers.get(&col) {
                    row_col_values
                        .entry(format!("{row_key}:{header}"))
                        .or_default()
                        .push(value);
                }
            }
        }
    }
    if non_empty_rows == 0 {
        push_unique(&mut issues, STRUCTURE, "xlsx_empty");
    }
    for anchor in expected {
        let lookup = if anchor.anchor.contains(':') {
            row_col_values.get(&anchor.anchor)
        } else if is_cell_ref(&anchor.anchor) {
            cells.get(&anchor.anchor.to_ascii_uppercase())
        } else {
            row_summaries.get(&anchor.anchor)
        };
        match lookup.map(Vec::as_slice) {
            None | Some([]) => {
                push_unique(&mut issues, ANCHOR_MISSING, &anchor.anchor);
            }
            Some([actual]) if actual != &anchor.value => {
                push_unique(&mut issues, VALUE_MISMATCH, &anchor.anchor);
            }
            Some([_]) => {}
            Some(_) => {
                push_unique(&mut issues, ANCHOR_DUPLICATE, &anchor.anchor);
            }
        }
    }
    Ok((report_for(path, "row_key", issues), bytes))
}

fn report_for(
    path: &Path,
    anchor: &'static str,
    issues: Vec<VerifyIssue>,
) -> VerifyReport {
    VerifyReport {
        artifact: path.display().to_string(),
        anchor,
        ok: issues.is_empty(),
        issues,
    }
}

fn is_cell_ref(token: &str) -> bool {
    let bytes = token.as_bytes();
    let letters = bytes.iter().take_while(|b| b.is_ascii_alphabetic()).count();
    (1..=3).contains(&letters)
        && letters < bytes.len()
        && bytes[letters..].iter().all(u8::is_ascii_digit)
}

fn column_letters(mut index: usize) -> String {
    let mut out = Vec::new();
    loop {
        let rem = u8::try_from(index % 26).unwrap_or(0);
        out.push(b'A' + rem);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "A".to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared verification finalizer keeps report, verified bytes, and telemetry in one path"
)]
async fn finalize_report(
    path: &Path,
    raw_path: &str,
    report: VerifyReport,
    verified_bytes: &[u8],
    shapes: Option<&[(String, String)]>,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
) -> ToolExecutionResult {
    let mut report_id = String::new();
    let mut artifact_hash = delivery::hash_bytes(verified_bytes);
    if let Some(store) = store {
        let issues = report
            .issues
            .iter()
            .map(|issue| delivery::Issue {
                code: issue.code.to_owned(),
                field: issue.field.clone(),
            })
            .collect();
        match store.record_report_bytes(path, verified_bytes, issues, now_ms())
        {
            Ok(persisted) => {
                report_id = persisted.id;
                artifact_hash = persisted.artifact_hash;
            }
            Err(error) => {
                tracing::warn!(
                    target: "flowy.delivery",
                    %error,
                    path = %path.display(),
                    "could not persist the verification report"
                );
            }
        }
    }
    let artifact_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(raw_path);
    let (l3_outcome, l3_duration_ms) = if report.ok
        && let (Some(semantic_ctx), Some(deck_shapes)) = (semantic, shapes)
    {
        probe_deck_semantics(
            semantic_ctx,
            artifact_name,
            deck_shapes,
            &context.signal,
        )
        .await
    } else {
        (SemanticOutcome::Skipped, 0)
    };
    let semantic_hint = l3_outcome.hint_text();
    let session_id = context.agent_id.as_deref().unwrap_or("");
    let submission_id = context.submission_id.as_deref().unwrap_or("");
    tracing::info!(
        target: "flowy.verify",
        session_id,
        root_session_id = session_id,
        submission_id,
        report_id = %report_id,
        artifact_hash = %artifact_hash,
        anchor = report.anchor,
        ok = report.ok,
        issue_count = report.issues.len(),
        l3_status = l3_outcome.status_code(),
        l3_reason = l3_outcome.reason(),
        l3_issue_count = l3_outcome.issue_count(),
        l3_duration_ms,
        "artifact verification completed"
    );
    let summary = if report.ok {
        "verification passed".to_owned()
    } else {
        format!(
            "verification failed: {}",
            report
                .issues
                .iter()
                .map(|issue| issue.code)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let value = report.to_value_with_meta(
        &summary,
        &report_id,
        &artifact_hash,
        semantic_hint.as_deref(),
    );
    let content = serde_json::to_string(&value).unwrap_or(summary);
    ToolExecutionResult::success_text(content, value)
}

async fn execute_deck(
    arguments: Value,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
    contract: Option<&BriefContract>,
) -> ToolExecutionResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        path: String,
        slides: usize,
        anchors: Vec<DeckAnchor>,
    }
    let Ok(args) = serde_json::from_value::<Args>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path, slides and anchors",
        );
    };
    let path = resolve_turn_path(
        context.cwd.as_deref(),
        context.output_root.as_deref(),
        &args.path,
    );
    match verify_deck_file_with_shapes(
        &path,
        args.slides,
        &args.anchors,
        contract,
    ) {
        Ok(verified) => {
            finalize_report(
                &path,
                &args.path,
                verified.report,
                &verified.bytes,
                Some(&verified.shapes),
                context,
                store,
                semantic,
            )
            .await
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

async fn execute_artifact(
    arguments: Value,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    contract: Option<&BriefContract>,
) -> ToolExecutionResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        path: String,
        #[serde(default)]
        slides: Option<usize>,
        #[serde(default)]
        anchors: Vec<DeckAnchor>,
    }
    let Ok(args) = serde_json::from_value::<Args>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path and optional slides/anchors",
        );
    };
    let path = resolve_turn_path(
        context.cwd.as_deref(),
        context.output_root.as_deref(),
        &args.path,
    );
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let result = match ext.as_deref() {
        Some("pptx") => match args.slides {
            Some(slides) => verify_deck_file_with_shapes(
                &path,
                slides,
                &args.anchors,
                contract,
            )
            .map(|verified| (verified.report, verified.bytes)),
            None => Err(
                "pptx verification requires expected slide count (`slides`)"
                    .to_owned(),
            ),
        },
        Some("docx") => verify_docx_file(&path, &args.anchors, contract),
        Some("xlsx") => verify_xlsx_file(&path, &args.anchors, contract),
        _ => {
            return ToolExecutionResult::error(
                "UNSUPPORTED_FORMAT",
                format!(
                    "no deterministic verifier for {}; supported: .pptx, .docx, .xlsx",
                    args.path
                ),
            );
        }
    };
    match result {
        Ok((report, bytes)) => {
            finalize_report(
                &path, &args.path, report, &bytes, None, context, store, None,
            )
            .await
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

pub fn definition(store: Option<DeliveryStore>) -> Arc<ToolDefinition> {
    definition_with_semantic_and_contract(store, None, None)
}

pub fn definition_with_semantic_and_contract(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "verify_deck",
        "Verify a generated deck before delivery. Checks host brief constraints, L1 per-slide title/body completeness, slide count, and each named shape against the expected text. Each anchor must be the slide shape name (for create_deck outputs: S0-title, S0-body, S1-title, S1-body, where slide 0 title is title and slide i>0 title is '{title} · {i+1}'). Returns ok, next_action ('publish' | 'rerender_once' | 'clarify_or_rebuild'), and ANCHOR_MISSING / ANCHOR_DUPLICATE / ANCHOR_INVALID / VALUE_MISMATCH / STRUCTURE issues; a failed report is not a tool error.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "slides": {"type": "integer", "minimum": 1},
                "anchors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "anchor": {"type": "string", "minLength": 1},
                            "value": {"type": "string"}
                        },
                        "required": ["anchor", "value"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "slides", "anchors"],
            "additionalProperties": false
        }),
        move |args: ToolCallArgs, context: ToolRunContext| {
            let store = store.clone();
            let semantic = semantic.clone();
            let contract = contract.clone();
            Box::pin(async move {
                execute_deck(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    semantic.as_ref(),
                    contract.as_ref(),
                )
                .await
            })
        },
    ))
}

pub fn artifact_definition(
    store: Option<DeliveryStore>,
    contract: Option<BriefContract>,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "verify_artifact",
        "Verify a generated .docx, .xlsx, or .pptx artifact before calling publish_file. Checks OOXML structure, non-empty content, host brief filename constraints, and optional anchors (Word bookmark names for .docx; row_key, 'row_key:ColumnHeader', or cell ref like 'B2' for .xlsx; shape names + slides for .pptx). Persists a delivery gate report; a failed report is not a tool error.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "slides": {"type": "integer", "minimum": 1},
                "anchors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "anchor": {"type": "string", "minLength": 1},
                            "value": {"type": "string"}
                        },
                        "required": ["anchor", "value"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        move |args: ToolCallArgs, context: ToolRunContext| {
            let store = store.clone();
            let contract = contract.clone();
            Box::pin(async move {
                execute_artifact(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    contract.as_ref(),
                )
                .await
            })
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn anchor(name: &str, value: &str) -> DeckAnchor {
        DeckAnchor {
            anchor: name.to_owned(),
            value: value.to_owned(),
        }
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, body) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    fn tool_context(root: &Path) -> ToolRunContext {
        ToolRunContext {
            signal: flowy_agent_core::CancelToken::new(),
            agent_id: Some("s-verify".to_owned()),
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: Some("sub-verify".to_owned()),
            session_origin: flowy_agent_core::SessionOrigin::Home,
            output_root: None,
            artifact_delivery_available: true,
        }
    }

    #[test]
    fn every_code_is_reported_distinctly() {
        let deck = vec![
            pair("S0-title", "T"),
            pair("S0-title", "T"),
            pair("S0-extra", "x"),
        ];
        let expected = vec![
            anchor("S0-title", "T"),
            anchor("S0-body", "one"),
            anchor("S0-value", "wrong"),
        ];
        let report = evaluate("deck.pptx", 2, 1, &deck, &expected);
        assert!(!report.ok);
        let codes: Vec<&str> =
            report.issues.iter().map(|issue| issue.code).collect();
        assert!(codes.contains(&STRUCTURE));
        assert!(codes.contains(&ANCHOR_DUPLICATE));
        assert!(codes.contains(&ANCHOR_MISSING));
        assert!(codes.contains(&ANCHOR_INVALID));
    }

    #[test]
    fn brief_contract_extracts_counts_and_skips_ordinals() {
        let east =
            BriefContract::from_brief("请做一份3页华东区汇报 q3-east.pptx");
        assert_eq!(east.min_slides, Some(3));
        assert_eq!(east.expected_filenames, vec!["q3-east.pptx"]);

        assert_eq!(
            BriefContract::from_brief("制作两页总结").min_slides,
            Some(2)
        );
        assert_eq!(
            BriefContract::from_brief("Create a 4-slide review").min_slides,
            Some(4)
        );
        assert_eq!(
            BriefContract::from_brief("把第3页标题改成Q4").min_slides,
            None
        );
    }

    #[tokio::test]
    async fn shrinking_deck_below_brief_min_slides_fails_even_when_worker_lowers_declared_slides()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-anticheat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let deck = root.join("q3-east.pptx");
        crate::pptx::write_deck(
            &deck,
            "华东区Q3汇报",
            &["上海6.1亿".to_owned(), "江苏4.2亿".to_owned()],
        )
        .unwrap();
        let store = DeliveryStore::new(root.join("delivery"));
        let contract = BriefContract::from_brief(
            "请生成一份3页华东区汇报 q3-east.pptx 并核验后交付",
        );
        let tool = definition_with_semantic_and_contract(
            Some(store.clone()),
            None,
            Some(contract),
        );
        let args = ToolCallArgs {
            call_id: "call-cheat".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "q3-east.pptx",
                "slides": 2,
                "anchors": [
                    {"anchor": "S0-title", "value": "华东区Q3汇报"},
                    {"anchor": "S0-body", "value": "上海6.1亿"},
                    {"anchor": "S1-title", "value": "华东区Q3汇报 · 2"},
                    {"anchor": "S1-body", "value": "江苏4.2亿"},
                ],
            }),
        };
        let ToolExecutionResult::Success { value, .. } =
            (tool.execute)(args, tool_context(&root)).await
        else {
            panic!("failed verification must return a structured report");
        };
        assert_eq!(value["ok"], false);
        assert_eq!(value["next_action"], "clarify_or_rebuild");
        let issues = value["issues"].as_array().unwrap();
        assert!(issues.iter().any(|item| {
            item["code"] == STRUCTURE && item["field"] == "brief_min_slides"
        }));
        assert_eq!(
            store.check(&deck).unwrap(),
            Some(delivery::BlockReason::VerificationFailed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn docx_and_xlsx_verifiers_pass_valid_ooxml_and_block_broken_archives()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-ooxml-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = DeliveryStore::new(root.join("delivery"));

        let docx = root.join("memo.docx");
        write_zip(
            &docx,
            &[(
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="0" w:name="summary"/><w:r><w:t>Q3 revenue &amp; growth</w:t></w:r></w:p></w:body></w:document>"#,
            )],
        );
        let xlsx = root.join("sales.xlsx");
        write_zip(
            &xlsx,
            &[
                ("xl/workbook.xml", "<workbook/>"),
                (
                    "xl/sharedStrings.xml",
                    "<sst><si><t>Region</t></si><si><t>Revenue</t></si><si><t>East</t></si></sst>",
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>6.1</v></c></row></sheetData></worksheet>"#,
                ),
            ],
        );
        let broken = root.join("broken.xlsx");
        std::fs::write(&broken, b"hello").unwrap();

        let tool = artifact_definition(Some(store.clone()), None);
        let docx_res = (tool.execute)(
            ToolCallArgs {
                call_id: "c-docx".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({
                    "path": "memo.docx",
                    "anchors": [{"anchor": "summary", "value": "Q3 revenue & growth"}],
                }),
            },
            tool_context(&root),
        )
        .await;
        let ToolExecutionResult::Success { value: dv, .. } = docx_res else {
            panic!("docx verification failed");
        };
        assert_eq!(dv["ok"], true);
        assert_eq!(dv["anchor"], "bookmark");
        assert_eq!(store.check(&docx).unwrap(), None);

        let xlsx_res = (tool.execute)(
            ToolCallArgs {
                call_id: "c-xlsx".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({
                    "path": "sales.xlsx",
                    "anchors": [
                        {"anchor": "East:Revenue", "value": "6.1"},
                        {"anchor": "B2", "value": "6.1"},
                        {"anchor": "East", "value": "East | 6.1"},
                    ],
                }),
            },
            tool_context(&root),
        )
        .await;
        let ToolExecutionResult::Success { value: xv, .. } = xlsx_res else {
            panic!("xlsx verification failed");
        };
        assert_eq!(xv["ok"], true);
        assert_eq!(xv["anchor"], "row_key");
        assert_eq!(store.check(&xlsx).unwrap(), None);

        let broken_res = (tool.execute)(
            ToolCallArgs {
                call_id: "c-broken".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({ "path": "broken.xlsx" }),
            },
            tool_context(&root),
        )
        .await;
        let ToolExecutionResult::Success { value: bv, .. } = broken_res else {
            panic!("broken archive must return structured failed report");
        };
        assert_eq!(bv["ok"], false);
        assert_eq!(
            store.check(&broken).unwrap(),
            Some(delivery::BlockReason::VerificationFailed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

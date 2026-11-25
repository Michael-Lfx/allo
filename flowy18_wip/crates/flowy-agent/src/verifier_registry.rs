//! Pluggable deterministic artifact verifier registry (`最终方案.md` §6).
//!
//! All built-in Office verifiers share one [`VerifyReport`] contract and one
//! four-code vocabulary (`ANCHOR_MISSING`, `ANCHOR_DUPLICATE`,
//! `ANCHOR_INVALID`, `VALUE_MISMATCH`, plus L1 `STRUCTURE`) and persist into
//! [`DeliveryStore`] so `publish_file` enforces the same hard gate across
//! `.pptx`, `.docx`, and `.xlsx` without format-specific bypasses.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{ToolCallArgs, ToolExecutionResult, ToolRunContext};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::delivery::{self, DeliveryStore};
use crate::pptx::resolve_turn_path;
use crate::verify::{
    ANCHOR_DUPLICATE, ANCHOR_INVALID, ANCHOR_MISSING, DeckAnchor, STRUCTURE,
    VALUE_MISMATCH, VerifyIssue, VerifyReport, verify_deck_file_with_shapes,
};
use crate::verify_contract::BriefContract;

/// Safety bound per decompressed XML entry inside an OOXML archive (16 MiB).
const MAX_XML_ENTRY_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactVerifySpec {
    #[serde(default)]
    pub slides: Option<usize>,
    #[serde(default)]
    pub anchors: Vec<DeckAnchor>,
}

/// Extensible deterministic verifier interface for one or more file extensions.
pub trait ArtifactVerifier: Send + Sync {
    fn extensions(&self) -> &'static [&'static str];
    fn anchor_kind(&self) -> &'static str;
    fn verify(
        &self,
        path: &Path,
        spec: &ArtifactVerifySpec,
        contract: Option<&BriefContract>,
    ) -> Result<VerifyReport, String>;
}

/// Extension-dispatched registry of deterministic artifact verifiers.
#[derive(Default)]
pub struct VerifierRegistry {
    verifiers: Vec<Box<dyn ArtifactVerifier>>,
}

impl std::fmt::Debug for VerifierRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kinds: Vec<&str> = self
            .verifiers
            .iter()
            .map(|verifier| verifier.anchor_kind())
            .collect();
        f.debug_struct("VerifierRegistry")
            .field("verifiers", &kinds)
            .finish()
    }
}

impl VerifierRegistry {
    /// Registry populated with built-in `.pptx`, `.docx`, and `.xlsx` verifiers.
    #[must_use]
    pub fn default_builtin() -> Self {
        let mut registry = Self::default();
        registry.register(Box::new(PptxVerifier));
        registry.register(Box::new(DocxVerifier));
        registry.register(Box::new(XlsxVerifier));
        registry
    }

    pub fn register(&mut self, verifier: Box<dyn ArtifactVerifier>) {
        self.verifiers.push(verifier);
    }

    #[must_use]
    pub fn for_path(&self, path: &Path) -> Option<&dyn ArtifactVerifier> {
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())?
            .to_ascii_lowercase();
        self.verifiers
            .iter()
            .find(|verifier| {
                verifier
                    .extensions()
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(&ext))
            })
            .map(Box::as_ref)
    }
}

/// Deterministic `.pptx` verifier (`anchor = "shape_name"`).
#[derive(Clone, Copy, Debug, Default)]
pub struct PptxVerifier;

impl ArtifactVerifier for PptxVerifier {
    fn extensions(&self) -> &'static [&'static str] {
        &["pptx"]
    }

    fn anchor_kind(&self) -> &'static str {
        "shape_name"
    }

    fn verify(
        &self,
        path: &Path,
        spec: &ArtifactVerifySpec,
        contract: Option<&BriefContract>,
    ) -> Result<VerifyReport, String> {
        let slides = spec.slides.ok_or_else(|| {
            "pptx verification requires expected slide count (`slides`)"
                .to_owned()
        })?;
        verify_deck_file_with_shapes(path, slides, &spec.anchors, contract)
            .map(|(report, _)| report)
    }
}

/// Deterministic `.docx` verifier (`anchor = "bookmark"`).
///
/// Validates the OOXML ZIP package and `word/document.xml`, ensures at least
/// one non-empty paragraph exists, checks host filename constraints, and when
/// anchors are provided checks `<w:bookmarkStart w:name="..."/>` entries with
/// the unified four-code model.
#[derive(Clone, Copy, Debug, Default)]
pub struct DocxVerifier;

impl ArtifactVerifier for DocxVerifier {
    fn extensions(&self) -> &'static [&'static str] {
        &["docx"]
    }

    fn anchor_kind(&self) -> &'static str {
        "bookmark"
    }

    fn verify(
        &self,
        path: &Path,
        spec: &ArtifactVerifySpec,
        contract: Option<&BriefContract>,
    ) -> Result<VerifyReport, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let mut issues = Vec::new();
        if let Some(contract) = contract {
            contract.check_filename(path, "docx", &mut issues);
        }
        let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(&bytes)) else {
            push_unique(&mut issues, STRUCTURE, "docx_archive");
            return Ok(VerifyReport {
                artifact: path.display().to_string(),
                anchor: self.anchor_kind(),
                ok: false,
                issues,
            });
        };
        let Ok(xml) =
            read_zip_text_bounded_ci(&mut archive, "word/document.xml")
        else {
            push_unique(&mut issues, STRUCTURE, "docx_document_xml");
            return Ok(VerifyReport {
                artifact: path.display().to_string(),
                anchor: self.anchor_kind(),
                ok: false,
                issues,
            });
        };

        let (paragraphs, bookmarks) = parse_docx_paragraphs_and_bookmarks(&xml);
        if paragraphs.is_empty() {
            push_unique(&mut issues, STRUCTURE, "docx_empty");
        }
        if !spec.anchors.is_empty() {
            evaluate_named_anchors(&bookmarks, &spec.anchors, &mut issues);
        }

        Ok(VerifyReport {
            artifact: path.display().to_string(),
            anchor: self.anchor_kind(),
            ok: issues.is_empty(),
            issues,
        })
    }
}

/// Deterministic `.xlsx` verifier (`anchor = "row_key"`).
///
/// Validates the OOXML ZIP package, `xl/workbook.xml`, shared strings, and
/// worksheet XML entries, ensures at least one non-empty cell exists, checks
/// host filename constraints, and when anchors are supplied verifies row keys
/// (`row_key`, `row_key:col_header`, or explicit cell reference `B2`).
#[derive(Clone, Copy, Debug, Default)]
pub struct XlsxVerifier;

impl ArtifactVerifier for XlsxVerifier {
    fn extensions(&self) -> &'static [&'static str] {
        &["xlsx"]
    }

    fn anchor_kind(&self) -> &'static str {
        "row_key"
    }

    fn verify(
        &self,
        path: &Path,
        spec: &ArtifactVerifySpec,
        contract: Option<&BriefContract>,
    ) -> Result<VerifyReport, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let mut issues = Vec::new();
        if let Some(contract) = contract {
            contract.check_filename(path, "xlsx", &mut issues);
        }
        let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(&bytes)) else {
            push_unique(&mut issues, STRUCTURE, "xlsx_archive");
            return Ok(VerifyReport {
                artifact: path.display().to_string(),
                anchor: self.anchor_kind(),
                ok: false,
                issues,
            });
        };
        if read_zip_text_bounded_ci(&mut archive, "xl/workbook.xml").is_err() {
            push_unique(&mut issues, STRUCTURE, "xlsx_workbook");
            return Ok(VerifyReport {
                artifact: path.display().to_string(),
                anchor: self.anchor_kind(),
                ok: false,
                issues,
            });
        }

        let shared_strings =
            read_zip_text_bounded_ci(&mut archive, "xl/sharedStrings.xml")
                .map_or_default(|xml| parse_shared_strings(&xml));

        let mut sheet_names: Vec<String> = (0..archive.len())
            .filter_map(|idx| {
                let name = archive.by_index(idx).ok()?.name().to_owned();
                let lower = name.to_ascii_lowercase();
                (lower.starts_with("xl/worksheets/sheet")
                    && Path::new(&lower)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("xml")))
                .then_some(name)
            })
            .collect();
        sheet_names.sort();
        if sheet_names.is_empty() {
            push_unique(&mut issues, STRUCTURE, "xlsx_worksheets");
            return Ok(VerifyReport {
                artifact: path.display().to_string(),
                anchor: self.anchor_kind(),
                ok: false,
                issues,
            });
        }

        let mut total_non_empty_cells = 0_usize;
        let mut headers_seen = BTreeSet::new();
        let mut row_entries: Vec<(String, String)> = Vec::new();
        let mut scoped_entries: Vec<(String, String)> = Vec::new();
        let mut cell_entries: Vec<(String, String)> = Vec::new();

        for sheet in &sheet_names {
            let Ok(xml) = read_zip_text_bounded(&mut archive, sheet) else {
                push_unique(&mut issues, STRUCTURE, "xlsx_sheet_xml");
                continue;
            };
            let rows = parse_xlsx_sheet_rows(&xml, &shared_strings);
            let mut col_headers: BTreeMap<String, String> = BTreeMap::new();
            for (row_idx, cells) in rows {
                for (col, val) in &cells {
                    if !val.trim().is_empty() {
                        total_non_empty_cells += 1;
                    }
                    let cell_ref = format!("{col}{row_idx}");
                    cell_entries.push((cell_ref, val.clone()));
                }
                if row_idx == 1 {
                    for (col, val) in &cells {
                        let header = val.trim();
                        if !header.is_empty() {
                            col_headers.insert(col.clone(), header.to_owned());
                            headers_seen.insert(header.to_owned());
                        }
                    }
                    continue;
                }
                let Some((first_col, first_val)) = cells.first() else {
                    continue;
                };
                let row_key = first_val.trim();
                if row_key.is_empty() {
                    continue;
                }
                let rest: Vec<&str> = cells
                    .iter()
                    .skip(1)
                    .map(|(_, value)| value.trim())
                    .filter(|value| !value.is_empty())
                    .collect();
                row_entries.push((row_key.to_owned(), rest.join(" | ")));
                for (col, val) in &cells {
                    if col == first_col {
                        continue;
                    }
                    if let Some(header) = col_headers.get(col) {
                        scoped_entries.push((
                            format!("{row_key}:{header}"),
                            val.trim().to_owned(),
                        ));
                    }
                }
            }
        }

        if total_non_empty_cells == 0 {
            push_unique(&mut issues, STRUCTURE, "xlsx_empty");
        }
        if !spec.anchors.is_empty() {
            evaluate_xlsx_anchors(
                &spec.anchors,
                &headers_seen,
                &row_entries,
                &scoped_entries,
                &cell_entries,
                &mut issues,
            );
        }

        Ok(VerifyReport {
            artifact: path.display().to_string(),
            anchor: self.anchor_kind(),
            ok: issues.is_empty(),
            issues,
        })
    }
}

fn evaluate_named_anchors(
    actual: &[(String, String)],
    expected: &[DeckAnchor],
    issues: &mut Vec<VerifyIssue>,
) {
    let mut seen = Vec::new();
    for anchor in expected {
        let matches: Vec<&(String, String)> = actual
            .iter()
            .filter(|(name, _)| name == &anchor.anchor)
            .collect();
        if matches.is_empty() {
            push_unique(issues, ANCHOR_MISSING, &anchor.anchor);
        } else if matches.len() > 1 {
            push_unique(issues, ANCHOR_DUPLICATE, &anchor.anchor);
        } else if matches[0].1.trim() != anchor.value.trim() {
            push_unique(issues, VALUE_MISMATCH, &anchor.anchor);
        }
        seen.push(anchor.anchor.as_str());
    }
    for (name, _) in actual {
        if !seen.contains(&name.as_str()) {
            push_unique(issues, ANCHOR_INVALID, name);
        }
    }
}

fn evaluate_xlsx_anchors(
    expected: &[DeckAnchor],
    headers_seen: &BTreeSet<String>,
    row_entries: &[(String, String)],
    scoped_entries: &[(String, String)],
    cell_entries: &[(String, String)],
    issues: &mut Vec<VerifyIssue>,
) {
    for anchor in expected {
        let key = anchor.anchor.trim();
        if let Some((_, header)) = key.split_once(':') {
            if !headers_seen.is_empty() && !headers_seen.contains(header) {
                push_unique(issues, ANCHOR_INVALID, key);
                continue;
            }
            check_anchor_matches(scoped_entries, key, &anchor.value, issues);
        } else if is_cell_ref(key) {
            let upper = key.to_ascii_uppercase();
            check_anchor_matches(cell_entries, &upper, &anchor.value, issues);
        } else {
            check_anchor_matches(row_entries, key, &anchor.value, issues);
        }
    }
}

fn check_anchor_matches(
    entries: &[(String, String)],
    key: &str,
    expected_value: &str,
    issues: &mut Vec<VerifyIssue>,
) {
    let matches: Vec<&(String, String)> =
        entries.iter().filter(|(name, _)| name == key).collect();
    if matches.is_empty() {
        push_unique(issues, ANCHOR_MISSING, key);
    } else if matches.len() > 1 {
        push_unique(issues, ANCHOR_DUPLICATE, key);
    } else if matches[0].1.trim() != expected_value.trim() {
        push_unique(issues, VALUE_MISMATCH, key);
    }
}

fn is_cell_ref(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_alphabetic() {
        idx += 1;
    }
    if idx == 0 || idx > 3 || idx == bytes.len() {
        return false;
    }
    bytes[idx..].iter().all(u8::is_ascii_digit)
}

fn read_zip_text_bounded_ci(
    archive: &mut zip::ZipArchive<Cursor<&Vec<u8>>>,
    expected_name: &str,
) -> Result<String, String> {
    let resolved = (0..archive.len()).find_map(|idx| {
        let name = archive.by_index(idx).ok()?.name().to_owned();
        name.eq_ignore_ascii_case(expected_name).then_some(name)
    });
    let Some(actual) = resolved else {
        return Err(format!("missing archive entry {expected_name}"));
    };
    read_zip_text_bounded(archive, &actual)
}

fn read_zip_text_bounded(
    archive: &mut zip::ZipArchive<Cursor<&Vec<u8>>>,
    entry_name: &str,
) -> Result<String, String> {
    let entry = archive
        .by_name(entry_name)
        .map_err(|error| error.to_string())?;
    let mut limited = entry.take(MAX_XML_ENTRY_BYTES + 1);
    let mut buf = Vec::new();
    limited
        .read_to_end(&mut buf)
        .map_err(|error| error.to_string())?;
    if buf.len() as u64 > MAX_XML_ENTRY_BYTES {
        return Err(format!("{entry_name} exceeds safety limit"));
    }
    String::from_utf8(buf).map_err(|error| error.to_string())
}

fn parse_docx_paragraphs_and_bookmarks(
    xml: &str,
) -> (Vec<String>, Vec<(String, String)>) {
    let mut paragraphs = Vec::new();
    let mut bookmarks = Vec::new();
    let mut cursor = 0;
    while let Some(open_rel) = xml[cursor..].find("<w:p") {
        let open_pos = cursor + open_rel;
        let after_tag = open_pos + 4;
        let boundary = xml[after_tag..].chars().next();
        if !matches!(boundary, Some('>' | ' ' | '\t' | '\n' | '\r' | '/')) {
            cursor = after_tag;
            continue;
        }
        let Some(tag_end_rel) = xml[open_pos..].find('>') else {
            break;
        };
        let tag_end = open_pos + tag_end_rel;
        // Microsoft Word often emits <w:bookmarkStart> at <w:body> level
        // immediately before the paragraph it anchors.
        let leading_bookmarks = extract_bookmark_names(&xml[cursor..open_pos]);
        if xml[open_pos..=tag_end].ends_with("/>") {
            cursor = tag_end + 1;
            continue;
        }
        let Some(close_rel) = xml[tag_end + 1..].find("</w:p>") else {
            break;
        };
        let inner = &xml[tag_end + 1..tag_end + 1 + close_rel];
        let text = extract_xml_tag_texts(inner, "w:t");
        if !text.trim().is_empty() {
            paragraphs.push(text.clone());
        }
        for name in leading_bookmarks
            .into_iter()
            .chain(extract_bookmark_names(inner))
        {
            if !name.starts_with('_') {
                bookmarks.push((name, text.clone()));
            }
        }
        cursor = tag_end + 1 + close_rel + "</w:p>".len();
    }
    (paragraphs, bookmarks)
}

fn extract_bookmark_names(fragment: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = fragment[cursor..].find("<w:bookmarkStart") {
        let pos = cursor + rel;
        let Some(end_rel) = fragment[pos..].find('>') else {
            break;
        };
        let tag = &fragment[pos..=pos + end_rel];
        if let Some(name) = extract_xml_attr(tag, "w:name")
            && !name.is_empty()
        {
            names.push(decode_xml_entities(&name));
        }
        cursor = pos + end_rel + 1;
    }
    names
}

fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = xml[cursor..].find("<si") {
        let pos = cursor + rel;
        let Some(tag_end_rel) = xml[pos..].find('>') else {
            break;
        };
        let tag_end = pos + tag_end_rel;
        if xml[pos..=tag_end].ends_with("/>") {
            out.push(String::new());
            cursor = tag_end + 1;
            continue;
        }
        let Some(close_rel) = xml[tag_end + 1..].find("</si>") else {
            break;
        };
        let inner = &xml[tag_end + 1..tag_end + 1 + close_rel];
        out.push(extract_xml_tag_texts(inner, "t"));
        cursor = tag_end + 1 + close_rel + "</si>".len();
    }
    out
}

fn parse_xlsx_sheet_rows(
    xml: &str,
    shared_strings: &[String],
) -> Vec<(usize, Vec<(String, String)>)> {
    let mut rows = Vec::new();
    let mut cursor = 0;
    let mut implicit_row = 0_usize;
    while let Some(rel) = xml[cursor..].find("<row") {
        let pos = cursor + rel;
        let boundary = xml[pos + 4..].chars().next();
        if !matches!(boundary, Some('>' | ' ' | '\t' | '\n' | '\r' | '/')) {
            cursor = pos + 4;
            continue;
        }
        let Some(tag_end_rel) = xml[pos..].find('>') else {
            break;
        };
        let tag_end = pos + tag_end_rel;
        let open_tag = &xml[pos..=tag_end];
        implicit_row += 1;
        let row_num = extract_xml_attr(open_tag, "r")
            .and_then(|val| val.parse::<usize>().ok())
            .unwrap_or(implicit_row);
        if open_tag.ends_with("/>") {
            cursor = tag_end + 1;
            continue;
        }
        let Some(close_rel) = xml[tag_end + 1..].find("</row>") else {
            break;
        };
        let inner = &xml[tag_end + 1..tag_end + 1 + close_rel];
        let cells = parse_xlsx_cells(inner, shared_strings);
        if !cells.is_empty() {
            rows.push((row_num, cells));
        }
        cursor = tag_end + 1 + close_rel + "</row>".len();
    }
    rows
}

fn parse_xlsx_cells(
    row_xml: &str,
    shared_strings: &[String],
) -> Vec<(String, String)> {
    let mut cells = Vec::new();
    let mut cursor = 0;
    let mut col_idx = 0_usize;
    while let Some(rel) = row_xml[cursor..].find("<c") {
        let pos = cursor + rel;
        let boundary = row_xml[pos + 2..].chars().next();
        if !matches!(boundary, Some('>' | ' ' | '\t' | '\n' | '\r' | '/')) {
            cursor = pos + 2;
            continue;
        }
        let Some(tag_end_rel) = row_xml[pos..].find('>') else {
            break;
        };
        let tag_end = pos + tag_end_rel;
        let open_tag = &row_xml[pos..=tag_end];
        col_idx += 1;
        if open_tag.ends_with("/>") {
            cursor = tag_end + 1;
            continue;
        }
        let Some(close_rel) = row_xml[tag_end + 1..].find("</c>") else {
            break;
        };
        let inner = &row_xml[tag_end + 1..tag_end + 1 + close_rel];
        let cell_ref = extract_xml_attr(open_tag, "r")
            .unwrap_or_else(|| column_letters(col_idx));
        let col: String = cell_ref
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect::<String>()
            .to_ascii_uppercase();
        let col = if col.is_empty() {
            column_letters(col_idx)
        } else {
            col
        };
        let cell_type = extract_xml_attr(open_tag, "t").unwrap_or_default();
        let value = match cell_type.as_str() {
            "s" => extract_xml_tag_texts(inner, "v")
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|idx| shared_strings.get(idx).cloned())
                .unwrap_or_default(),
            "inlineStr" => extract_xml_tag_texts(inner, "t"),
            _ => extract_xml_tag_texts(inner, "v"),
        };
        if !value.trim().is_empty() {
            cells.push((col, value));
        }
        cursor = tag_end + 1 + close_rel + "</c>".len();
    }
    cells
        .sort_by(|(a, _), (b, _)| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    cells
}

fn column_letters(mut index: usize) -> String {
    let mut chars = Vec::new();
    while index > 0 {
        let rem = (index - 1) % 26;
        chars.push((b'A' + rem as u8) as char);
        index = (index - 1) / 26;
    }
    chars.reverse();
    chars.into_iter().collect()
}

fn extract_xml_tag_texts(fragment: &str, tag_name: &str) -> String {
    let open_prefix = format!("<{tag_name}");
    let close_tag = format!("</{tag_name}>");
    let mut out = String::new();
    let mut cursor = 0;
    while let Some(rel) = fragment[cursor..].find(&open_prefix) {
        let pos = cursor + rel;
        let after = pos + open_prefix.len();
        let boundary = fragment[after..].chars().next();
        if !matches!(boundary, Some('>' | ' ' | '\t' | '\n' | '\r' | '/')) {
            cursor = after;
            continue;
        }
        let Some(tag_end_rel) = fragment[pos..].find('>') else {
            break;
        };
        let tag_end = pos + tag_end_rel;
        if fragment[pos..=tag_end].ends_with("/>") {
            cursor = tag_end + 1;
            continue;
        }
        let Some(close_rel) = fragment[tag_end + 1..].find(&close_tag) else {
            break;
        };
        let body = &fragment[tag_end + 1..tag_end + 1 + close_rel];
        out.push_str(&decode_xml_entities(body));
        cursor = tag_end + 1 + close_rel + close_tag.len();
    }
    out
}

fn extract_xml_attr(tag: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{attr_name}=");
    let mut cursor = 0;
    while let Some(rel) = tag[cursor..].find(&pattern) {
        let pos = cursor + rel;
        if pos > 0
            && !matches!(
                tag[..pos].chars().next_back(),
                Some(' ' | '\t' | '\n' | '\r')
            )
        {
            cursor = pos + pattern.len();
            continue;
        }
        let rest = &tag[pos + pattern.len()..];
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let after_quote = &rest[quote.len_utf8()..];
        let end = after_quote.find(quote)?;
        return Some(after_quote[..end].to_owned());
    }
    None
}

fn decode_xml_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let entity = &after[..semi];
            match entity {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ if entity.starts_with('#') => {
                    let parsed = entity
                        .strip_prefix("#x")
                        .or_else(|| entity.strip_prefix("#X"))
                        .map_or_else(
                            || entity[1..].parse::<u32>().ok(),
                            |hex| u32::from_str_radix(hex, 16).ok(),
                        )
                        .and_then(char::from_u32);
                    if let Some(ch) = parsed {
                        out.push(ch);
                    } else {
                        out.push('&');
                        out.push_str(entity);
                        out.push(';');
                    }
                }
                _ => {
                    out.push('&');
                    out.push_str(entity);
                    out.push(';');
                }
            }
            rest = &after[semi + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn push_unique(issues: &mut Vec<VerifyIssue>, code: &'static str, field: &str) {
    if issues
        .iter()
        .any(|issue| issue.code == code && issue.field == field)
    {
        return;
    }
    issues.push(VerifyIssue {
        code,
        field: field.to_owned(),
    });
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn execute(
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
    let registry = VerifierRegistry::default_builtin();
    let Some(verifier) = registry.for_path(&path) else {
        return ToolExecutionResult::error(
            "UNSUPPORTED_FORMAT",
            format!(
                "no deterministic verifier registered for {}; supported: .pptx, .docx, .xlsx",
                args.path
            ),
        );
    };
    let spec = ArtifactVerifySpec {
        slides: args.slides,
        anchors: args.anchors,
    };
    match verifier.verify(&path, &spec, contract) {
        Ok(report) => {
            let mut report_id = String::new();
            let mut artifact_hash = std::fs::read(&path).map_or_else(
                |_| String::new(),
                |bytes| {
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    format!("{:x}", hasher.finalize())
                },
            );
            if let Some(store) = store {
                let issues = report
                    .issues
                    .iter()
                    .map(|issue| delivery::Issue {
                        code: issue.code.to_owned(),
                        field: issue.field.clone(),
                    })
                    .collect();
                match store.record_report(&path, issues, now_ms()) {
                    Ok(persisted) => {
                        report_id = persisted.id;
                        artifact_hash = persisted.artifact_hash;
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "flowy.delivery",
                            %error,
                            path = %path.display(),
                            "could not persist artifact verification report"
                        );
                    }
                }
            }
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
                None,
            );
            let content = serde_json::to_string(&value).unwrap_or(summary);
            ToolExecutionResult::success_text(content, value)
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

pub fn definition(
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
                execute(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    contract.as_ref(),
                )
            })
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn write_zip(path: &Path, files: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, body) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir()
            .join(format!("flowy-verifier-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn docx_verifier_validates_structure_and_bookmarks() {
        let root = scratch("docx");
        let path = root.join("summary.docx");
        write_zip(
            &path,
            &[(
                "word/document.xml",
                r#"<w:document><w:body>
                  <w:p><w:bookmarkStart w:id="0" w:name="region"/><w:r><w:t>华东区 Q3 业绩</w:t></w:r></w:p>
                  <w:p><w:bookmarkStart w:id="1" w:name="revenue"/><w:r><w:t>12.4 亿元 &amp; +18%</w:t></w:r></w:p>
                </w:body></w:document>"#,
            )],
        );

        let verifier = DocxVerifier;
        // L1 structure pass without anchors.
        let basic = verifier
            .verify(&path, &ArtifactVerifySpec::default(), None)
            .unwrap();
        assert!(basic.ok, "{:?}", basic.issues);
        assert_eq!(basic.anchor, "bookmark");

        // L2 bookmark anchor pass.
        let ok_spec = ArtifactVerifySpec {
            slides: None,
            anchors: vec![
                DeckAnchor {
                    anchor: "region".into(),
                    value: "华东区 Q3 业绩".into(),
                },
                DeckAnchor {
                    anchor: "revenue".into(),
                    value: "12.4 亿元 & +18%".into(),
                },
            ],
        };
        let passed = verifier.verify(&path, &ok_spec, None).unwrap();
        assert!(passed.ok, "{:?}", passed.issues);

        // L2 value mismatch + missing bookmark.
        let bad_spec = ArtifactVerifySpec {
            slides: None,
            anchors: vec![
                DeckAnchor {
                    anchor: "region".into(),
                    value: "华南区".into(),
                },
                DeckAnchor {
                    anchor: "missing_bm".into(),
                    value: "x".into(),
                },
            ],
        };
        let failed = verifier.verify(&path, &bad_spec, None).unwrap();
        assert!(!failed.ok);
        let codes: Vec<&str> = failed.issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&VALUE_MISMATCH));
        assert!(codes.contains(&ANCHOR_MISSING));
        assert!(codes.contains(&ANCHOR_INVALID));

        // Corrupt non-zip file fails cleanly as STRUCTURE.
        let corrupt = root.join("broken.docx");
        std::fs::write(&corrupt, b"not a zip").unwrap();
        let bad = verifier
            .verify(&corrupt, &ArtifactVerifySpec::default(), None)
            .unwrap();
        assert!(!bad.ok);
        assert_eq!(bad.issues[0].code, STRUCTURE);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn xlsx_verifier_validates_shared_strings_headers_and_row_keys() {
        let root = scratch("xlsx");
        let path = root.join("sales.xlsx");
        write_zip(
            &path,
            &[
                (
                    "xl/workbook.xml",
                    r#"<workbook><sheets><sheet name="Sheet1" sheetId="1"/></sheets></workbook>"#,
                ),
                (
                    "xl/sharedStrings.xml",
                    r"<sst><si><t>区域</t></si><si><t>销售额</t></si><si><t>华东区</t></si></sst>",
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData>
                      <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
                      <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>12.4</v></c></row>
                    </sheetData></worksheet>"#,
                ),
            ],
        );

        let verifier = XlsxVerifier;
        let basic = verifier
            .verify(&path, &ArtifactVerifySpec::default(), None)
            .unwrap();
        assert!(basic.ok, "{:?}", basic.issues);
        assert_eq!(basic.anchor, "row_key");

        let spec = ArtifactVerifySpec {
            slides: None,
            anchors: vec![
                DeckAnchor {
                    anchor: "华东区:销售额".into(),
                    value: "12.4".into(),
                },
                DeckAnchor {
                    anchor: "B2".into(),
                    value: "12.4".into(),
                },
            ],
        };
        let passed = verifier.verify(&path, &spec, None).unwrap();
        assert!(passed.ok, "{:?}", passed.issues);

        let bad_spec = ArtifactVerifySpec {
            slides: None,
            anchors: vec![
                DeckAnchor {
                    anchor: "华东区:不存在列".into(),
                    value: "1".into(),
                },
                DeckAnchor {
                    anchor: "华南区".into(),
                    value: "9.0".into(),
                },
                DeckAnchor {
                    anchor: "华东区:销售额".into(),
                    value: "99.9".into(),
                },
            ],
        };
        let failed = verifier.verify(&path, &bad_spec, None).unwrap();
        assert!(!failed.ok);
        let codes: Vec<&str> = failed.issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&ANCHOR_INVALID));
        assert!(codes.contains(&ANCHOR_MISSING));
        assert!(codes.contains(&VALUE_MISMATCH));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn verify_artifact_unlocks_publish_file_hard_gate_for_xlsx() {
        let root = scratch("xlsx-publish");
        let store =
            crate::artifact_store::ArtifactStore::open(root.join("store"))
                .unwrap();
        let gate = DeliveryStore::new(root.join("delivery"));
        let submission_id = format!("submission-{:032x}", 7);
        let output_root = root.join("workspace/outputs").join(&submission_id);
        std::fs::create_dir_all(&output_root).unwrap();
        let sheet = output_root.join("q3.xlsx");
        write_zip(
            &sheet,
            &[
                ("xl/workbook.xml", r"<workbook/>"),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Q3</t></is></c><c r="B1"><v>12.4</v></c></row></sheetData></worksheet>"#,
                ),
            ],
        );

        let publish =
            crate::publish_tool::definition(store, Some(gate.clone()));
        let make_ctx = || ToolRunContext {
            signal: flowy_agent_core::CancelToken::new(),
            agent_id: Some("s-xlsx".to_owned()),
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: Some(submission_id.clone()),
            session_origin: flowy_agent_core::SessionOrigin::Home,
            output_root: Some(output_root.clone()),
            artifact_delivery_available: true,
        };

        // Before verification, .xlsx is blocked (not silently exempted).
        let blocked = (publish.execute)(
            ToolCallArgs {
                call_id: "p1".into(),
                name: "publish_file".into(),
                arguments: json!({"path": "q3.xlsx", "delivery_key": "q3"}),
            },
            make_ctx(),
        )
        .await;
        assert!(matches!(
            blocked,
            ToolExecutionResult::Error { ref code, .. }
                if code == "DELIVERY_NOT_VERIFIED"
        ));

        // After verify_artifact passes, publish_file succeeds cleanly.
        let verify_tool = definition(
            Some(gate),
            Some(BriefContract::from_brief("生成表格 q3.xlsx")),
        );
        let verified = (verify_tool.execute)(
            ToolCallArgs {
                call_id: "v1".into(),
                name: "verify_artifact".into(),
                arguments: json!({"path": "q3.xlsx"}),
            },
            make_ctx(),
        )
        .await;
        assert!(matches!(verified, ToolExecutionResult::Success { .. }));

        let published = (publish.execute)(
            ToolCallArgs {
                call_id: "p2".into(),
                name: "publish_file".into(),
                arguments: json!({"path": "q3.xlsx", "delivery_key": "q3"}),
            },
            make_ctx(),
        )
        .await;
        assert!(matches!(
            published,
            ToolExecutionResult::Success {
                delivery: Some(_),
                ..
            }
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}

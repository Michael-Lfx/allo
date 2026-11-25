//! Deterministic OOXML verifiers for `.pptx`, `.docx`, and `.xlsx`.

use super::{
    ANCHOR_DUPLICATE, ANCHOR_INVALID, ANCHOR_MISSING, BTreeMap, BriefContract,
    Cursor, DeckAnchor, MAX_XML_ENTRY_BYTES, Path, Read, STRUCTURE,
    VALUE_MISMATCH, VerifyIssue, VerifyReport,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct SlideL1Stats {
    pub(super) index: usize,
    pub(super) has_title: bool,
    pub(super) has_body: bool,
    pub(super) non_empty_shapes: usize,
}

pub(super) fn push_unique(
    issues: &mut Vec<VerifyIssue>,
    code: &'static str,
    field: &str,
) {
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

pub(super) fn evaluate_pairs(
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

pub(super) fn evaluate(
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

pub(super) struct VerifiedDeck {
    pub(super) report: VerifyReport,
    pub(super) bytes: Vec<u8>,
    pub(super) shapes: Vec<(String, String)>,
}

/// Maximum ZIP entries accepted in a `.pptx` archive before full parse.
const MAX_PPTX_ENTRIES: usize = 4_096;
/// Maximum cumulative declared uncompressed size for `.pptx` preflight (64 MiB).
const MAX_PPTX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
/// Cumulative decompressed XML budget per OOXML archive (32 MiB).
const MAX_ARCHIVE_XML_BYTES: u64 = 32 * 1024 * 1024;
/// Maximum total populated cells processed across all sheets of one `.xlsx`.
const MAX_XLSX_CELLS: usize = 200_000;

/// Bounded ZIP-bomb preflight run BEFORE `rpptx::Presentation::from_bytes`.
///
/// Enforces entry count, declared uncompressed total, AND actually streams
/// `.xml`/`.rels` entries against `MAX_ARCHIVE_XML_BYTES` so forged ZIP headers
/// cannot OOM the desktop process.
pub(super) fn preflight_pptx_archive(bytes: &[u8]) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("invalid pptx archive: {error}"))?;
    if archive.len() > MAX_PPTX_ENTRIES {
        return Err(format!(
            "pptx archive exceeds maximum entry count ({MAX_PPTX_ENTRIES})"
        ));
    }
    let mut total_declared = 0_u64;
    let mut xml_budget = MAX_ARCHIVE_XML_BYTES;
    let mut discard = Vec::new();
    for idx in 0..archive.len() {
        let entry = archive
            .by_index(idx)
            .map_err(|error| format!("unreadable pptx entry: {error}"))?;
        let declared = entry.size();
        total_declared = total_declared.saturating_add(declared);
        if total_declared > MAX_PPTX_TOTAL_UNCOMPRESSED_BYTES {
            return Err(format!(
                "pptx archive exceeds {MAX_PPTX_TOTAL_UNCOMPRESSED_BYTES}-byte uncompressed limit"
            ));
        }
        let is_xml = Path::new(entry.name()).extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("rels")
        });
        let entry_limit = if is_xml {
            MAX_XML_ENTRY_BYTES.min(xml_budget)
        } else {
            MAX_PPTX_TOTAL_UNCOMPRESSED_BYTES
        };
        if is_xml {
            if entry_limit == 0 {
                return Err(format!(
                    "pptx XML exceeds {MAX_ARCHIVE_XML_BYTES}-byte archive budget"
                ));
            }
            discard.clear();
            entry
                .take(entry_limit.saturating_add(1))
                .read_to_end(&mut discard)
                .map_err(|error| format!("unreadable pptx XML: {error}"))?;
            xml_budget = xml_budget.saturating_sub(discard.len() as u64);
            if discard.len() as u64 > entry_limit {
                return Err(format!(
                    "pptx XML entry exceeds {MAX_XML_ENTRY_BYTES}-byte / {MAX_ARCHIVE_XML_BYTES}-byte budget"
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn verify_deck_file_with_shapes(
    path: &Path,
    expected_slides: usize,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedDeck, String> {
    let bytes = crate::delivery::read_regular_file_bounded(
        path,
        crate::MAX_ARTIFACT_BYTES,
    )
    .map_err(|error| error.to_string())?;
    preflight_pptx_archive(&bytes)?;
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
    let active_contract = contract.unwrap_or(&default_contract);
    active_contract.check_deck(
        path,
        slides.len(),
        expected_slides,
        &per_slide,
        expected.len(),
        &mut report.issues,
    );
    let combined_text: String = deck
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    active_contract.check_pinned_facts(&combined_text, &mut report.issues);
    report.ok = report.issues.is_empty();
    Ok(VerifiedDeck {
        report,
        bytes,
        shapes: deck,
    })
}

pub(super) struct VerifiedArtifact {
    pub(super) report: VerifyReport,
    pub(super) bytes: Vec<u8>,
    pub(super) snippets: Vec<(String, String)>,
}

pub(super) fn read_zip_xml_budgeted(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    entry_name: &str,
    remaining_budget: &mut u64,
) -> Option<String> {
    let limit = MAX_XML_ENTRY_BYTES.min(*remaining_budget);
    if limit == 0 {
        return None;
    }
    let entry = archive.by_name(entry_name).ok()?;
    let mut buf = Vec::new();
    entry.take(limit + 1).read_to_end(&mut buf).ok()?;
    // Always charge decompressed bytes against the cumulative archive budget
    // BEFORE rejecting an oversized entry so many oversized sheets cannot
    // bypass `MAX_ARCHIVE_XML_BYTES`.
    *remaining_budget = remaining_budget.saturating_sub(buf.len() as u64);
    if buf.len() as u64 > limit {
        return None;
    }
    String::from_utf8(buf).ok()
}

fn collect_direct_para_text_and_bookmarks(
    para: roxmltree::Node<'_, '_>,
    text: &mut String,
    bookmark_names: &mut Vec<String>,
) {
    for child in para.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            // Stop at nested paragraphs (e.g. textboxes or table cells) so
            // inner paragraphs are scored once in their own pass instead of
            // causing false ANCHOR_DUPLICATE hits.
            "p" => {}
            "t" => {
                if let Some(chunk) = child.text() {
                    text.push_str(chunk);
                }
            }
            "tab" => text.push('\t'),
            "br" => text.push('\n'),
            "bookmarkStart" => {
                if let Some(name) = child
                    .attributes()
                    .find(|attr| attr.name() == "name")
                    .map(|attr| attr.value())
                    && !name.is_empty()
                    && !name.starts_with('_')
                {
                    bookmark_names.push(name.to_owned());
                }
            }
            _ => collect_direct_para_text_and_bookmarks(
                child,
                text,
                bookmark_names,
            ),
        }
    }
}

pub(super) fn verify_docx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedArtifact, String> {
    let bytes = crate::delivery::read_regular_file_bounded(
        path,
        crate::MAX_ARTIFACT_BYTES,
    )
    .map_err(|error| error.to_string())?;
    let mut issues = Vec::new();
    if let Some(contract) = contract {
        contract.check_filename(path, "docx", &mut issues);
    }
    if expected.is_empty() {
        push_unique(&mut issues, STRUCTURE, "anchors");
    }
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
    else {
        push_unique(&mut issues, STRUCTURE, "docx_archive");
        return Ok(VerifiedArtifact {
            report: report_for(path, "bookmark", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let mut xml_budget = MAX_ARCHIVE_XML_BYTES;
    let Some(xml) = read_zip_xml_budgeted(
        &mut archive,
        "word/document.xml",
        &mut xml_budget,
    ) else {
        push_unique(&mut issues, STRUCTURE, "docx_document");
        return Ok(VerifiedArtifact {
            report: report_for(path, "bookmark", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let Ok(doc) = roxmltree::Document::parse(&xml) else {
        push_unique(&mut issues, STRUCTURE, "docx_document");
        return Ok(VerifiedArtifact {
            report: report_for(path, "bookmark", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let mut non_empty_paragraphs = 0_usize;
    let mut bookmarks: Vec<(String, String)> = Vec::new();
    let mut snippets: Vec<(String, String)> = Vec::new();
    let mut all_text = String::new();
    // Track body-level (sibling) `<w:bookmarkStart>` nodes outside `<w:p>` and
    // attach them to the next paragraph's trimmed text.
    let mut pending_sibling_bookmarks: Vec<String> = Vec::new();
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        if node.tag_name().name() == "bookmarkStart" {
            let inside_para = node
                .ancestors()
                .skip(1)
                .any(|anc| anc.is_element() && anc.tag_name().name() == "p");
            if !inside_para
                && let Some(name) = node
                    .attributes()
                    .find(|attr| attr.name() == "name")
                    .map(|attr| attr.value())
                && !name.is_empty()
                && !name.starts_with('_')
            {
                pending_sibling_bookmarks.push(name.to_owned());
            }
            continue;
        }
        if node.tag_name().name() != "p" {
            continue;
        }
        let mut text = String::new();
        let mut para_bookmarks = Vec::new();
        collect_direct_para_text_and_bookmarks(
            node,
            &mut text,
            &mut para_bookmarks,
        );
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            non_empty_paragraphs += 1;
            all_text.push_str(trimmed);
            all_text.push('\n');
            snippets
                .push((format!("P{non_empty_paragraphs}"), trimmed.to_owned()));
            for name in std::mem::take(&mut pending_sibling_bookmarks) {
                bookmarks.push((name, trimmed.to_owned()));
            }
        }
        for name in para_bookmarks {
            bookmarks.push((name, trimmed.to_owned()));
        }
    }
    if non_empty_paragraphs == 0 {
        push_unique(&mut issues, STRUCTURE, "docx_empty");
    }
    if !expected.is_empty() {
        evaluate_pairs(&bookmarks, expected, false, &mut issues);
    }
    if let Some(contract) = contract {
        contract.check_pinned_facts(&all_text, &mut issues);
    }
    Ok(VerifiedArtifact {
        report: report_for(path, "bookmark", issues),
        bytes,
        snippets,
    })
}

fn numeric_sheet_order_key(name: &str) -> (u32, String) {
    let lower = name.to_ascii_lowercase();
    let num = lower
        .rsplit('/')
        .next()
        .and_then(|file| file.strip_prefix("sheet"))
        .and_then(|rest| rest.strip_suffix(".xml"))
        .and_then(|digits| digits.parse::<u32>().ok())
        .unwrap_or(u32::MAX);
    (num, lower)
}

fn resolve_xlsx_sheets(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    wb_doc: &roxmltree::Document<'_>,
    xml_budget: &mut u64,
) -> Vec<(String, String)> {
    // Prefer exact workbook relationships (`xl/_rels/workbook.xml.rels`) so
    // custom sheet filenames and >= 10 sheets map 1:1 with `workbook.xml`.
    let mut rel_targets: BTreeMap<String, String> = BTreeMap::new();
    if let Some(rels_xml) =
        read_zip_xml_budgeted(archive, "xl/_rels/workbook.xml.rels", xml_budget)
        && let Ok(rels_doc) = roxmltree::Document::parse(&rels_xml)
    {
        for rel in rels_doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        {
            if let (Some(id), Some(target)) =
                (rel.attribute("Id"), rel.attribute("Target"))
            {
                let trimmed = target.trim_start_matches('/');
                let full = if trimmed.starts_with("xl/") {
                    trimmed.to_owned()
                } else {
                    format!("xl/{trimmed}")
                };
                rel_targets.insert(id.to_owned(), full);
            }
        }
    }
    let mut mapped: Vec<(String, String)> = Vec::new();
    for sheet in wb_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "sheet")
    {
        let title = sheet.attribute("name").unwrap_or_default().to_owned();
        let rid = sheet
            .attributes()
            .find(|attr| attr.name() == "id")
            .map(|attr| attr.value());
        if let Some(rid) = rid
            && let Some(target) = rel_targets.get(rid)
            && archive.by_name(target).is_ok()
        {
            mapped.push((
                if title.is_empty() {
                    format!("Sheet{}", mapped.len() + 1)
                } else {
                    title
                },
                target.clone(),
            ));
        }
    }
    if !mapped.is_empty() {
        return mapped;
    }
    let sheet_titles: Vec<String> = wb_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "sheet")
        .filter_map(|node| node.attribute("name").map(str::to_owned))
        .collect();
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
    sheet_names.sort_by_key(|name| numeric_sheet_order_key(name));
    sheet_names
        .into_iter()
        .enumerate()
        .map(|(idx, path)| {
            let title = sheet_titles
                .get(idx)
                .cloned()
                .unwrap_or_else(|| format!("Sheet{}", idx + 1));
            (title, path)
        })
        .collect()
}

pub(super) fn verify_xlsx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedArtifact, String> {
    let bytes = crate::delivery::read_regular_file_bounded(
        path,
        crate::MAX_ARTIFACT_BYTES,
    )
    .map_err(|error| error.to_string())?;
    let mut issues = Vec::new();
    if let Some(contract) = contract {
        contract.check_filename(path, "xlsx", &mut issues);
    }
    if expected.is_empty() {
        push_unique(&mut issues, STRUCTURE, "anchors");
    }
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
    else {
        push_unique(&mut issues, STRUCTURE, "xlsx_archive");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let mut xml_budget = MAX_ARCHIVE_XML_BYTES;
    let Some(wb_xml) =
        read_zip_xml_budgeted(&mut archive, "xl/workbook.xml", &mut xml_budget)
    else {
        push_unique(&mut issues, STRUCTURE, "xlsx_workbook");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else {
        push_unique(&mut issues, STRUCTURE, "xlsx_workbook");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let shared_strings: Vec<String> = read_zip_xml_budgeted(
        &mut archive,
        "xl/sharedStrings.xml",
        &mut xml_budget,
    )
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
                            node.is_element() && node.tag_name().name() == "t"
                        })
                        .filter_map(|node| node.text())
                        .collect::<String>()
                })
                .collect(),
        )
    })
    .unwrap_or_default();

    let resolved_sheets =
        resolve_xlsx_sheets(&mut archive, &wb_doc, &mut xml_budget);
    if resolved_sheets.is_empty() {
        push_unique(&mut issues, STRUCTURE, "xlsx_sheets");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    }

    let mut non_empty_rows = 0_usize;
    let mut total_cells = 0_usize;
    let mut cells: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut qualified_cells: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut row_summaries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut qualified_row_summaries: BTreeMap<String, Vec<String>> =
        BTreeMap::new();
    let mut row_col_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut qualified_row_col: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut snippets: Vec<(String, String)> = Vec::new();
    let mut all_text = String::new();

    'sheets: for (sheet_title, sheet_name) in &resolved_sheets {
        let sheet_key = sheet_title.to_ascii_lowercase();
        let Some(sheet_xml) =
            read_zip_xml_budgeted(&mut archive, sheet_name, &mut xml_budget)
        else {
            push_unique(&mut issues, STRUCTURE, "xlsx_sheet");
            if xml_budget == 0 {
                break 'sheets;
            }
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&sheet_xml) else {
            push_unique(&mut issues, STRUCTURE, "xlsx_sheet");
            continue;
        };
        // Isolate column headers per sheet so Sheet 2 never inherits Sheet 1 headers.
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
                total_cells = total_cells.saturating_add(1);
                if total_cells > MAX_XLSX_CELLS {
                    push_unique(&mut issues, STRUCTURE, "xlsx_cell_limit");
                    break 'sheets;
                }
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
                    _ => {
                        let cached = cell
                            .children()
                            .find(|n| {
                                n.is_element() && n.tag_name().name() == "v"
                            })
                            .and_then(|n| n.text())
                            .map_or_default(str::trim);
                        if !cached.is_empty() {
                            cached.to_owned()
                        } else if let Some(formula) = cell
                            .children()
                            .find(|n| {
                                n.is_element() && n.tag_name().name() == "f"
                            })
                            .and_then(|n| n.text())
                            .map(str::trim)
                            .filter(|f| !f.is_empty())
                        {
                            format!("={formula}")
                        } else {
                            String::new()
                        }
                    }
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
            all_text.push_str(&summary);
            all_text.push('\n');
            snippets
                .push((format!("{sheet_title}!{row_key}"), summary.clone()));
            qualified_row_summaries
                .entry(format!("{sheet_key}!{row_key}"))
                .or_default()
                .push(summary.clone());
            row_summaries
                .entry(row_key.clone())
                .or_default()
                .push(summary);
            for (col, cell_ref, value) in row_cells {
                qualified_cells
                    .entry(format!("{sheet_key}!{cell_ref}"))
                    .or_default()
                    .push(value.clone());
                cells.entry(cell_ref).or_default().push(value.clone());
                if let Some(header) = headers.get(&col) {
                    qualified_row_col
                        .entry(format!("{sheet_key}!{row_key}:{header}"))
                        .or_default()
                        .push(value.clone());
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
        let lookup = if let Some((sheet_part, target)) =
            anchor.anchor.split_once('!')
        {
            let sheet_norm = sheet_part.trim().to_ascii_lowercase();
            let target_trimmed = target.trim();
            if target_trimmed.contains(':') {
                qualified_row_col.get(&format!("{sheet_norm}!{target_trimmed}"))
            } else if is_cell_ref(target_trimmed) {
                qualified_cells.get(&format!(
                    "{sheet_norm}!{}",
                    target_trimmed.to_ascii_uppercase()
                ))
            } else {
                qualified_row_summaries
                    .get(&format!("{sheet_norm}!{target_trimmed}"))
            }
        } else if anchor.anchor.contains(':') {
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
    if let Some(contract) = contract {
        contract.check_pinned_facts(&all_text, &mut issues);
    }
    Ok(VerifiedArtifact {
        report: report_for(path, "row_key", issues),
        bytes,
        snippets,
    })
}

pub(super) fn report_for(
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

pub(super) fn is_cell_ref(token: &str) -> bool {
    let bytes = token.as_bytes();
    let letters = bytes.iter().take_while(|b| b.is_ascii_alphabetic()).count();
    (1..=3).contains(&letters)
        && letters < bytes.len()
        && bytes[letters..].iter().all(u8::is_ascii_digit)
}

pub(super) fn column_letters(mut index: usize) -> String {
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

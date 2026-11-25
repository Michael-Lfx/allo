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

pub(super) fn verify_deck_file_with_shapes(
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

pub(super) fn read_zip_xml(
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

pub(super) fn verify_docx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedArtifact, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
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
    let Some(xml) = read_zip_xml(&mut archive, "word/document.xml") else {
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
            all_text.push_str(trimmed);
            all_text.push('\n');
            snippets
                .push((format!("P{non_empty_paragraphs}"), trimmed.to_owned()));
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
    if let Some(contract) = contract {
        contract.check_pinned_facts(&all_text, &mut issues);
    }
    Ok(VerifiedArtifact {
        report: report_for(path, "bookmark", issues),
        bytes,
        snippets,
    })
}

pub(super) fn verify_xlsx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedArtifact, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
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
    let Some(wb_xml) = read_zip_xml(&mut archive, "xl/workbook.xml") else {
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
    let sheet_titles: Vec<String> = wb_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "sheet")
        .filter_map(|node| node.attribute("name").map(str::to_owned))
        .collect();
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
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    }

    let mut non_empty_rows = 0_usize;
    let mut cells: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut qualified_cells: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut row_summaries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut row_col_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut qualified_row_col: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut snippets: Vec<(String, String)> = Vec::new();
    let mut all_text = String::new();

    for (sheet_idx, sheet_name) in sheet_names.iter().enumerate() {
        let sheet_title = sheet_titles
            .get(sheet_idx)
            .cloned()
            .unwrap_or_else(|| format!("Sheet{}", sheet_idx + 1));
        let sheet_key = sheet_title.to_ascii_lowercase();
        let Some(sheet_xml) = read_zip_xml(&mut archive, sheet_name) else {
            push_unique(&mut issues, STRUCTURE, "xlsx_sheet");
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
                row_summaries.get(target_trimmed)
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

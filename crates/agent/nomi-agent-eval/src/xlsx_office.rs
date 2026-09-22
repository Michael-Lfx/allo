//! Lightweight OOXML `.xlsx` inspection for eval scorers (sheet names + header row).

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

pub fn xlsx_sheet_names(bytes: &[u8]) -> Result<Vec<String>, String> {
    let xml = zip_entry_string(bytes, "xl/workbook.xml")?;
    Ok(extract_quoted_attrs(&xml, "name"))
}

pub fn xlsx_header_row(bytes: &[u8], sheet: Option<&str>) -> Result<Vec<String>, String> {
    let names = xlsx_sheet_names(bytes)?;
    if names.is_empty() {
        return Err("xlsx has no sheets".into());
    }
    let target = match sheet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(want) => names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(want))
            .cloned()
            .ok_or_else(|| format!("sheet `{want}` not found"))?,
        None => names[0].clone(),
    };
    let rels = zip_entry_string(bytes, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let workbook = zip_entry_string(bytes, "xl/workbook.xml")?;
    let sheet_path = sheet_target_path(&workbook, &rels, &target)?;
    let sheet_xml = zip_entry_string(bytes, &sheet_path)?;
    let shared = zip_entry_string(bytes, "xl/sharedStrings.xml")
        .ok()
        .map(|xml| parse_shared_strings(&xml))
        .unwrap_or_default();
    Ok(first_row_values(&sheet_xml, &shared))
}

pub fn xlsx_sheet_tables(bytes: &[u8]) -> Result<Vec<(String, Vec<Vec<String>>)>, String> {
    let names = xlsx_sheet_names(bytes)?;
    if names.is_empty() {
        return Err("xlsx has no sheets".into());
    }
    let rels = zip_entry_string(bytes, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let workbook = zip_entry_string(bytes, "xl/workbook.xml")?;
    let shared = zip_entry_string(bytes, "xl/sharedStrings.xml")
        .ok()
        .map(|xml| parse_shared_strings(&xml))
        .unwrap_or_default();
    let mut tables = Vec::new();
    for name in names {
        let sheet_path = sheet_target_path(&workbook, &rels, &name)?;
        let sheet_xml = zip_entry_string(bytes, &sheet_path)?;
        tables.push((name, all_row_values(&sheet_xml, &shared)));
    }
    Ok(tables)
}

pub fn parse_cell_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed.replace(',', "").replace('%', "");
    if stripped.chars().any(|c| c.is_ascii_alphabetic() || c == '/') {
        return None;
    }
    stripped.parse::<f64>().ok().filter(|n| n.is_finite())
}

pub fn xlsx_numeric_values(bytes: &[u8]) -> Result<Vec<f64>, String> {
    let mut out = Vec::new();
    for (_name, rows) in xlsx_sheet_tables(bytes)? {
        for row in rows {
            for cell in row {
                if let Some(n) = parse_cell_number(&cell) {
                    out.push(n);
                }
            }
        }
    }
    Ok(out)
}

/// Latest-month revenue total plus per-region sums, used by the T03 advisory oracle.
pub fn xlsx_source_totals(bytes: &[u8]) -> Result<Vec<f64>, String> {
    let tables = xlsx_sheet_tables(bytes)?;
    let mut totals = Vec::new();
    for (_name, rows) in &tables {
        totals.extend(table_expected_totals(rows));
    }
    if totals.is_empty() {
        let numbers = xlsx_numeric_values(bytes)?;
        if numbers.is_empty() {
            return Err("source workbook has no numeric cells".into());
        }
        totals.push(numbers.iter().copied().sum());
    }
    Ok(totals)
}

pub fn xlsx_totals_within(
    source: &[u8],
    output: &[u8],
    tolerance: f64,
) -> Result<(bool, String), String> {
    let expected = xlsx_source_totals(source)?;
    let actual = xlsx_numeric_values(output)?;
    if actual.is_empty() {
        return Ok((false, "output has no numeric cells".into()));
    }
    let hits = expected
        .iter()
        .filter(|want| actual.iter().any(|have| numbers_close(**want, *have, tolerance)))
        .count();
    Ok((
        hits > 0,
        format!(
            "matched={hits}/{} expected={} tolerance={tolerance}",
            expected.len(),
            expected
                .iter()
                .map(|n| format!("{n:.2}"))
                .collect::<Vec<_>>()
                .join("|")
        ),
    ))
}

fn numbers_close(a: f64, b: f64, tolerance: f64) -> bool {
    let scale = a.abs().max(1.0);
    (a - b).abs() / scale <= tolerance
}

fn table_expected_totals(rows: &[Vec<String>]) -> Vec<f64> {
    let Some(header) = rows.first() else {
        return Vec::new();
    };
    let Some(value_idx) = header.iter().position(|h| looks_like_revenue(h)) else {
        return Vec::new();
    };
    let month_idx = header.iter().position(|h| looks_like_month(h));
    let region_idx = header.iter().position(|h| looks_like_region(h));
    let data = &rows[1..];
    let latest = month_idx.and_then(|idx| {
        data.iter()
            .filter_map(|row| row.get(idx).map(|s| s.as_str()))
            .max_by_key(|s| month_sort_key(s))
            .map(str::to_owned)
    });
    let selected: Vec<&Vec<String>> = if let (Some(month_idx), Some(latest)) = (month_idx, latest) {
        data.iter()
            .filter(|row| row.get(month_idx).map(|s| s.as_str()) == Some(latest.as_str()))
            .collect()
    } else {
        data.iter().collect()
    };
    let mut totals = Vec::new();
    let grand: f64 = selected
        .iter()
        .filter_map(|row| row.get(value_idx).and_then(|s| parse_cell_number(s)))
        .sum();
    if grand != 0.0 {
        totals.push(grand);
    }
    if let Some(region_idx) = region_idx {
        let mut by_region: BTreeMap<String, f64> = BTreeMap::new();
        for row in &selected {
            let Some(key) = row
                .get(region_idx)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            if let Some(v) = row.get(value_idx).and_then(|s| parse_cell_number(s)) {
                *by_region.entry(key).or_insert(0.0) += v;
            }
        }
        totals.extend(by_region.into_values().filter(|v| *v != 0.0));
    }
    totals
}

fn looks_like_revenue(header: &str) -> bool {
    let n = header.to_ascii_lowercase();
    n.contains("revenue")
        || header.contains("营收")
        || header.contains("營收")
        || n.contains("sales")
        || n == "amount"
}

fn looks_like_month(header: &str) -> bool {
    let n = header.to_ascii_lowercase();
    n.contains("month") || n.contains("date") || n.contains("period") || header.contains("月")
}

fn looks_like_region(header: &str) -> bool {
    let n = header.to_ascii_lowercase();
    n.contains("region") || header.contains("区域") || header.contains("區域")
}

fn month_sort_key(value: &str) -> String {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 4 {
        format!("{digits:0>8}")
    } else {
        value.to_owned()
    }
}

pub fn read_xlsx_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn zip_entry_string(bytes: &[u8], name: &str) -> Result<String, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("invalid xlsx package: {e}"))?;
    let mut entry = archive
        .by_name(name)
        .map_err(|e| format!("missing {name}: {e}"))?;
    let mut xml = String::new();
    entry
        .read_to_string(&mut xml)
        .map_err(|e| format!("read {name}: {e}"))?;
    Ok(xml)
}

fn sheet_target_path(workbook: &str, rels: &str, sheet_name: &str) -> Result<String, String> {
    let rid = sheet_rid_for_name(workbook, sheet_name)
        .ok_or_else(|| format!("sheet `{sheet_name}` has no r:id"))?;
    let target = rel_target(rels, &rid).unwrap_or_else(|| "worksheets/sheet1.xml".to_owned());
    let trimmed = target.trim_start_matches('/');
    if trimmed.starts_with("xl/") {
        Ok(trimmed.to_owned())
    } else {
        Ok(format!("xl/{trimmed}"))
    }
}

fn sheet_rid_for_name(workbook: &str, sheet_name: &str) -> Option<String> {
    let mut search = workbook;
    while let Some(idx) = search.find("<sheet") {
        let rest = &search[idx..];
        let end = rest.find("/>").or_else(|| rest.find('>'))?;
        let tag = &rest[..=end];
        let name = attr(tag, "name")?;
        if name.eq_ignore_ascii_case(sheet_name) {
            return attr(tag, "id").or_else(|| attr(tag, "r:id"));
        }
        search = &rest[end + 1..];
    }
    None
}

fn rel_target(rels: &str, rid: &str) -> Option<String> {
    let mut search = rels;
    while let Some(idx) = search.find("<Relationship") {
        let rest = &search[idx..];
        let end = rest.find("/>").or_else(|| rest.find('>'))?;
        let tag = &rest[..=end];
        if attr(tag, "Id").as_deref() == Some(rid) {
            return attr(tag, "Target");
        }
        search = &rest[end + 1..];
    }
    None
}

fn attr(tag: &str, key: &str) -> Option<String> {
    for prefix in [format!("{key}=\""), format!("r:{key}=\"")] {
        if let Some(start) = tag.find(&prefix) {
            let rest = &tag[start + prefix.len()..];
            if let Some(end) = rest.find('"') {
                return Some(decode_xml_entities(&rest[..end]));
            }
        }
    }
    None
}

fn extract_quoted_attrs(xml: &str, key: &str) -> Vec<String> {
    let needle = format!("{key}=\"");
    let mut out = Vec::new();
    let mut search = xml;
    while let Some(idx) = search.find(&needle) {
        let rest = &search[idx + needle.len()..];
        if let Some(end) = rest.find('"') {
            out.push(decode_xml_entities(&rest[..end]));
            search = &rest[end + 1..];
        } else {
            break;
        }
    }
    out
}

fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search = xml;
    while let Some(si) = search.find("<si") {
        let rest = &search[si..];
        let Some(end) = rest.find("</si>") else { break };
        let block = &rest[..end];
        out.push(concat_t_nodes(block));
        search = &rest[end + 5..];
    }
    out
}

fn concat_t_nodes(xml: &str) -> String {
    let mut out = String::new();
    let mut search = xml;
    while let Some(start) = search.find("<t") {
        let after = &search[start + 2..];
        let Some(tag_end) = after.find('>') else { break };
        if after[..tag_end].ends_with('/') {
            search = &after[tag_end + 1..];
            continue;
        }
        let body = &after[tag_end + 1..];
        if let Some(close) = body.find("</t>") {
            out.push_str(&decode_xml_entities(&body[..close]));
            search = &body[close + 4..];
        } else {
            break;
        }
    }
    out
}

fn first_row_values(sheet_xml: &str, shared: &[String]) -> Vec<String> {
    all_row_values(sheet_xml, shared)
        .into_iter()
        .next()
        .unwrap_or_default()
}

fn all_row_values(sheet_xml: &str, shared: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut search = sheet_xml;
    while let Some(start) = search.find("<row") {
        let rest = &search[start..];
        let Some(end) = rest.find("</row>") else { break };
        let row_xml = &rest[..end + 6];
        out.push(cells_in_row(row_xml, shared));
        search = &rest[end + 6..];
    }
    out
}

fn cells_in_row(row_xml: &str, shared: &[String]) -> Vec<String> {
    let mut cells: BTreeMap<u32, String> = BTreeMap::new();
    let mut search = row_xml;
    while let Some(idx) = search.find("<c") {
        let rest = &search[idx..];
        let Some(tag_end) = rest.find('>') else { break };
        let tag = &rest[..=tag_end];
        if !local_starts_with_c(tag) {
            search = &rest[1..];
            continue;
        }
        let col = attr(tag, "r")
            .and_then(|r| column_index(&r))
            .unwrap_or(cells.len() as u32);
        let cell_ty = attr(tag, "t").unwrap_or_default();
        let (inner, consumed) = if tag.ends_with("/>") {
            ("", tag.len())
        } else if let Some(close) = rest.find("</c>") {
            (&rest[tag_end + 1..close], close + 4)
        } else {
            break;
        };
        let value = cell_value(inner, &cell_ty, shared);
        cells.insert(col, value);
        search = &rest[consumed.max(1)..];
    }
    cells.into_values().collect()
}

fn local_starts_with_c(tag: &str) -> bool {
    let trimmed = tag.trim_start_matches('<');
    trimmed.starts_with("c ") || trimmed.starts_with("c>") || trimmed.starts_with("c/")
}

fn cell_value(inner: &str, ty: &str, shared: &[String]) -> String {
    if ty == "inlineStr" {
        return concat_t_nodes(inner);
    }
    let v = between(inner, "<v>", "</v>").unwrap_or_default();
    if ty == "s" {
        if let Ok(idx) = v.trim().parse::<usize>() {
            return shared.get(idx).cloned().unwrap_or_default();
        }
    }
    v.trim().to_owned()
}

fn between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = haystack.find(start)?;
    let rest = &haystack[i + start.len()..];
    let j = rest.find(end)?;
    Some(&rest[..j])
}

fn column_index(cell_ref: &str) -> Option<u32> {
    let mut n = 0u32;
    for c in cell_ref.chars() {
        if c.is_ascii_alphabetic() {
            n = n * 26 + u32::from(c.to_ascii_uppercase()) - u32::from(b'A') + 1;
        } else {
            break;
        }
    }
    (n > 0).then_some(n - 1)
}

fn decode_xml_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
pub(crate) fn pack_test_xlsx(sheet_name: &str, headers: &[&str]) -> Vec<u8> {
    pack_test_xlsx_rows(sheet_name, headers, &[])
}

#[cfg(test)]
pub(crate) fn pack_test_xlsx_rows(
    sheet_name: &str,
    headers: &[&str],
    rows: &[&[&str]],
) -> Vec<u8> {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let opt = SimpleFileOptions::default();
        zip.start_file("[Content_Types].xml", opt).unwrap();
        zip.write_all(b"<?xml version=\"1.0\"?><Types></Types>").unwrap();
        zip.start_file("xl/workbook.xml", opt).unwrap();
        let workbook = format!(
            r#"<?xml version="1.0"?><workbook xmlns:r="http://r"><sheets><sheet name="{sheet_name}" r:id="rId1"/></sheets></workbook>"#
        );
        zip.write_all(workbook.as_bytes()).unwrap();
        zip.start_file("xl/_rels/workbook.xml.rels", opt).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?><Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#,
        )
        .unwrap();
        let mut shared_items: Vec<String> = headers.iter().map(|h| (*h).to_owned()).collect();
        let mut sheet_data = String::new();
        sheet_data.push_str(r#"<row r="1">"#);
        for (i, _header) in headers.iter().enumerate() {
            let col = (b'A' + i as u8) as char;
            sheet_data.push_str(&format!(r#"<c r="{col}1" t="s"><v>{i}</v></c>"#));
        }
        sheet_data.push_str("</row>");
        for (row_idx, row) in rows.iter().enumerate() {
            let excel_row = row_idx + 2;
            sheet_data.push_str(&format!(r#"<row r="{excel_row}">"#));
            for (col_idx, cell) in row.iter().enumerate() {
                let col = (b'A' + col_idx as u8) as char;
                if parse_cell_number(cell).is_some()
                    && !cell.chars().any(|c| c.is_ascii_alphabetic() || c == '-')
                {
                    sheet_data.push_str(&format!(r#"<c r="{col}{excel_row}"><v>{cell}</v></c>"#));
                } else {
                    let si = shared_items.len();
                    shared_items.push((*cell).to_owned());
                    sheet_data.push_str(&format!(
                        r#"<c r="{col}{excel_row}" t="s"><v>{si}</v></c>"#
                    ));
                }
            }
            sheet_data.push_str("</row>");
        }
        let mut shared = String::from(r#"<?xml version="1.0"?><sst>"#);
        for item in &shared_items {
            shared.push_str(&format!("<si><t>{item}</t></si>"));
        }
        shared.push_str("</sst>");
        zip.start_file("xl/sharedStrings.xml", opt).unwrap();
        zip.write_all(shared.as_bytes()).unwrap();
        zip.start_file("xl/worksheets/sheet1.xml", opt).unwrap();
        let sheet = format!(
            r#"<?xml version="1.0"?><worksheet><sheetData>{sheet_data}</sheetData></worksheet>"#
        );
        zip.write_all(sheet.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sheet_names_and_headers() {
        let bytes = pack_test_xlsx("Action Items", &["Priority", "Owner", "Status"]);
        assert_eq!(xlsx_sheet_names(&bytes).unwrap(), vec!["Action Items"]);
        let headers = xlsx_header_row(&bytes, Some("Action Items")).unwrap();
        assert_eq!(headers, vec!["Priority", "Owner", "Status"]);
    }

    #[test]
    fn sums_latest_month_revenue() {
        let source = pack_test_xlsx_rows(
            "Sales",
            &["Region", "Month", "Revenue"],
            &[
                &["East", "2024-01", "10"],
                &["West", "2024-01", "20"],
                &["East", "2024-02", "30"],
                &["West", "2024-02", "40"],
            ],
        );
        let ok = pack_test_xlsx_rows(
            "Region Summary",
            &["Region", "Revenue"],
            &[&["East", "30"], &["West", "40"], &["Total", "70"]],
        );
        let miss = pack_test_xlsx_rows(
            "Region Summary",
            &["Region", "Revenue"],
            &[&["East", "1"], &["West", "2"]],
        );
        let (passed, detail) = xlsx_totals_within(&source, &ok, 0.01).unwrap();
        assert!(passed, "{detail}");
        let (passed, _) = xlsx_totals_within(&source, &miss, 0.01).unwrap();
        assert!(!passed);
    }
}

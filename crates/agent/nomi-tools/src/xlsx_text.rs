//! Plain-text extraction from OOXML `.xlsx` workbooks for the Read tool.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

const MAX_SHEETS: usize = 16;
const MAX_ROWS: usize = 80;
const MAX_CHARS: usize = 80_000;

pub(crate) fn is_xlsx_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx"))
}

pub(crate) fn extract_xlsx_text(bytes: &[u8]) -> Result<String, String> {
    let names = sheet_names(bytes)?;
    if names.is_empty() {
        return Err("xlsx has no sheets".into());
    }
    let rels = zip_entry_string(bytes, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let workbook = zip_entry_string(bytes, "xl/workbook.xml")?;
    let shared = zip_entry_string(bytes, "xl/sharedStrings.xml")
        .ok()
        .map(|xml| parse_shared_strings(&xml))
        .unwrap_or_default();
    let mut out = String::new();
    for (index, name) in names.iter().take(MAX_SHEETS).enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str("# Sheet: ");
        out.push_str(name);
        out.push('\n');
        let path = sheet_target_path(&workbook, &rels, name)?;
        let sheet_xml = zip_entry_string(bytes, &path)?;
        out.push_str(&sheet_rows_tsv(&sheet_xml, &shared));
        out.push('\n');
        if out.len() >= MAX_CHARS {
            out.truncate(MAX_CHARS);
            out.push_str("\n…");
            break;
        }
    }
    if names.len() > MAX_SHEETS {
        out.push_str(&format!(
            "\n({} additional sheets omitted)\n",
            names.len() - MAX_SHEETS
        ));
    }
    Ok(out)
}

fn sheet_names(bytes: &[u8]) -> Result<Vec<String>, String> {
    let xml = zip_entry_string(bytes, "xl/workbook.xml")?;
    Ok(extract_quoted_attrs(&xml, "name"))
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

fn sheet_rows_tsv(sheet_xml: &str, shared: &[String]) -> String {
    let mut out = String::new();
    let mut search = sheet_xml;
    let mut rows = 0usize;
    while let Some(start) = search.find("<row") {
        let rest = &search[start..];
        let Some(end) = rest.find("</row>") else { break };
        let row_xml = &rest[..end + 6];
        let line = row_values(row_xml, shared).join("\t");
        if rows > 0 {
            out.push('\n');
        }
        out.push_str(&line);
        rows += 1;
        if rows >= MAX_ROWS {
            out.push_str("\n…");
            break;
        }
        search = &rest[end + 6..];
    }
    out
}

fn row_values(row_xml: &str, shared: &[String]) -> Vec<String> {
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
        cells.insert(col, cell_value(inner, &cell_ty, shared));
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
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn pack_xlsx(sheet_name: &str, headers: &[&str]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let opt = SimpleFileOptions::default();
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
            let mut shared = String::from(r#"<?xml version="1.0"?><sst>"#);
            let mut row = String::from(r#"<row r="1">"#);
            for (i, header) in headers.iter().enumerate() {
                shared.push_str(&format!("<si><t>{header}</t></si>"));
                let col = (b'A' + i as u8) as char;
                row.push_str(&format!(r#"<c r="{col}1" t="s"><v>{i}</v></c>"#));
            }
            shared.push_str("</sst>");
            row.push_str("</row>");
            zip.start_file("xl/sharedStrings.xml", opt).unwrap();
            zip.write_all(shared.as_bytes()).unwrap();
            zip.start_file("xl/worksheets/sheet1.xml", opt).unwrap();
            let sheet = format!(
                r#"<?xml version="1.0"?><worksheet><sheetData>{row}</sheetData></worksheet>"#
            );
            zip.write_all(sheet.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn extracts_sheet_tsv() {
        let bytes = pack_xlsx("Region Summary", &["Region", "Revenue"]);
        let text = extract_xlsx_text(&bytes).unwrap();
        assert!(text.contains("# Sheet: Region Summary"));
        assert!(text.contains("Region\tRevenue"));
        assert!(is_xlsx_path("Sales_Result.XLSX"));
        assert!(!is_xlsx_path("notes.docx"));
    }
}

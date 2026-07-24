//! Plain-text extraction from OOXML `.docx` packages for the Read tool.
//!
//! A `.docx` is a ZIP of XML parts. Body copy lives in `word/document.xml`.
//! This module pulls paragraph text from that part so agents can summarize
//! documents without an external CLI (officecli / pandoc / python-docx).

use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

/// True when `path` looks like a Word OOXML document by extension.
pub(crate) fn is_docx_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
}

/// Extract readable body text from a `.docx` byte buffer.
pub(crate) fn extract_docx_text(bytes: &[u8]) -> Result<String, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("invalid docx package: {error}"))?;
    let mut entry = archive
        .by_name("word/document.xml")
        .map_err(|error| format!("missing word/document.xml: {error}"))?;
    let mut xml = String::new();
    entry
        .read_to_string(&mut xml)
        .map_err(|error| format!("failed to read word/document.xml: {error}"))?;
    Ok(document_xml_to_text(&xml))
}

fn document_xml_to_text(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() / 8);
    let bytes = xml.as_bytes();
    let mut i = 0;
    let mut in_text = false;
    let mut text_buf = String::new();

    while i < bytes.len() {
        if !in_text && bytes[i] == b'<' {
            if let Some(end) = find_tag_end(bytes, i) {
                let tag = &xml[i..=end];
                let name = local_tag_name(tag);
                if name == "t" {
                    if !tag.as_bytes().get(1).is_some_and(|b| *b == b'/') && !tag.ends_with("/>") {
                        in_text = true;
                        text_buf.clear();
                    }
                } else if name == "tab" {
                    out.push('\t');
                } else if name == "br" || name == "cr" {
                    out.push('\n');
                } else if name == "p" && tag.as_bytes().get(1).is_some_and(|b| *b == b'/') {
                    push_paragraph_break(&mut out);
                }
                i = end + 1;
                continue;
            }
        }

        if in_text {
            if bytes[i] == b'<' {
                // End of text run — expect </w:t>
                if let Some(end) = find_tag_end(bytes, i) {
                    let tag = &xml[i..=end];
                    if local_tag_name(tag) == "t"
                        && tag.as_bytes().get(1).is_some_and(|b| *b == b'/')
                    {
                        out.push_str(&decode_xml_entities(&text_buf));
                        in_text = false;
                        text_buf.clear();
                    }
                    i = end + 1;
                    continue;
                }
            }
            text_buf.push(xml[i..].chars().next().unwrap_or('\u{fffd}'));
            i += xml[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            continue;
        }

        i += 1;
    }

    trim_trailing_newlines(&mut out);
    out
}

fn push_paragraph_break(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

fn trim_trailing_newlines(out: &mut String) {
    while out.ends_with('\n') || out.ends_with('\r') {
        out.pop();
    }
}

fn find_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start + 1..]
        .iter()
        .position(|&b| b == b'>')
        .map(|offset| start + 1 + offset)
}

fn local_tag_name(tag: &str) -> &str {
    let body = tag
        .trim_start_matches('<')
        .trim_start_matches('/')
        .trim_end_matches('>')
        .trim_end_matches('/');
    let qname = body.split_whitespace().next().unwrap_or("");
    qname.rsplit_once(':').map(|(_, local)| local).unwrap_or(qname)
}

fn decode_xml_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        if let Some(semi) = rest.find(';') {
            let entity = &rest[..=semi];
            out.push_str(match entity {
                "&amp;" => "&",
                "&lt;" => "<",
                "&gt;" => ">",
                "&quot;" => "\"",
                "&apos;" => "'",
                _ => {
                    if let Some(decoded) = decode_numeric_entity(entity) {
                        out.push(decoded);
                        rest = &rest[semi + 1..];
                        continue;
                    }
                    entity
                }
            });
            rest = &rest[semi + 1..];
        } else {
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn decode_numeric_entity(entity: &str) -> Option<char> {
    let inner = entity.strip_prefix("&#")?.strip_suffix(';')?;
    let code = if let Some(hex) = inner.strip_prefix('x').or_else(|| inner.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        inner.parse::<u32>().ok()?
    };
    char::from_u32(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn pack_docx(document_xml: &str) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn extracts_paragraph_text_and_entities() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Hello &amp; welcome</w:t></w:r></w:p>
    <w:p><w:r><w:t xml:space="preserve">Line 2</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        let bytes = pack_docx(xml);
        let text = extract_docx_text(&bytes).unwrap();
        assert_eq!(text, "Hello & welcome\nLine 2");
    }

    #[test]
    fn is_docx_path_is_case_insensitive() {
        assert!(is_docx_path(r"C:\docs\Report.DOCX"));
        assert!(!is_docx_path(r"C:\docs\Report.doc"));
        assert!(!is_docx_path(r"C:\docs\notes.txt"));
    }
}

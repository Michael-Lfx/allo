//! Lightweight PDF text extraction for the Read tool (Tj / TJ operators).

use std::io::Read;
use std::path::Path;

use flate2::read::{DeflateDecoder, ZlibDecoder};

const MAX_CHARS: usize = 80_000;

pub(crate) fn is_pdf_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

pub(crate) fn extract_pdf_text(bytes: &[u8]) -> Result<String, String> {
    if !bytes.starts_with(b"%PDF") {
        return Err("not a PDF".into());
    }
    let mut out = String::new();
    let mut search = 0usize;
    while let Some(rel) = find_subslice(&bytes[search..], b"stream") {
        let keyword = search + rel;
        if !is_stream_keyword(bytes, keyword) {
            search = keyword + 6;
            continue;
        }
        let mut payload_start = keyword + 6;
        if bytes.get(payload_start) == Some(&b'\r') {
            payload_start += 1;
        }
        if bytes.get(payload_start) == Some(&b'\n') {
            payload_start += 1;
        }
        let Some(end_rel) = find_subslice(&bytes[payload_start..], b"endstream") else {
            break;
        };
        let payload_end = payload_start + end_rel;
        let header_from = search.max(keyword.saturating_sub(512));
        let header = &bytes[header_from..keyword];
        let payload = trim_eol(&bytes[payload_start..payload_end]);
        let decoded = if contains_slice(header, b"/FlateDecode") {
            inflate(payload).unwrap_or_else(|| payload.to_vec())
        } else {
            payload.to_vec()
        };
        extract_pdf_strings(&decoded, &mut out);
        if out.len() >= MAX_CHARS {
            out.truncate(MAX_CHARS);
            out.push('…');
            break;
        }
        search = payload_end + 9;
    }
    let text = collapse_blank_lines(out.trim());
    if text.is_empty() {
        Err("no extractable PDF text".into())
    } else {
        Ok(text)
    }
}

fn is_stream_keyword(bytes: &[u8], at: usize) -> bool {
    let before_ok = at == 0
        || bytes[at - 1].is_ascii_whitespace()
        || bytes[at - 1] == b'>';
    let after = at + 6;
    let after_ok = after >= bytes.len()
        || bytes[after].is_ascii_whitespace();
    before_ok && after_ok
}

fn inflate(payload: &[u8]) -> Option<Vec<u8>> {
    let mut zlib = ZlibDecoder::new(payload);
    let mut out = Vec::new();
    if zlib.read_to_end(&mut out).is_ok() && !out.is_empty() {
        return Some(out);
    }
    let mut deflate = DeflateDecoder::new(payload);
    out.clear();
    if deflate.read_to_end(&mut out).is_ok() && !out.is_empty() {
        return Some(out);
    }
    None
}

fn extract_pdf_strings(content: &[u8], out: &mut String) {
    let mut i = 0;
    while i < content.len() {
        match content[i] {
            b'(' => {
                let (text, next) = read_literal_string(content, i + 1);
                if !text.trim().is_empty() {
                    push_fragment(out, &text);
                }
                i = next;
            }
            b'<' if content.get(i + 1) == Some(&b'<') => i += 2,
            _ => i += 1,
        }
    }
}

fn read_literal_string(bytes: &[u8], mut i: usize) -> (String, usize) {
    let mut out = String::new();
    let mut depth = 1u32;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'(' | b')' | b'\\' => out.push(bytes[i] as char),
                    b'\n' => {}
                    b'\r' => {
                        if bytes.get(i + 1) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    other if other.is_ascii_digit() => {
                        let mut oct = other - b'0';
                        let mut consumed = 1;
                        while consumed < 3 {
                            if let Some(d) = bytes.get(i + consumed).copied() {
                                if d.is_ascii_digit() {
                                    oct = oct * 8 + (d - b'0');
                                    consumed += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                        out.push(char::from(oct));
                        i += consumed - 1;
                    }
                    other => out.push(other as char),
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                out.push('(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
                out.push(')');
            }
            b => {
                if b.is_ascii() {
                    out.push(b as char);
                }
                i += 1;
            }
        }
    }
    (out, i)
}

fn push_fragment(out: &mut String, fragment: &str) {
    if !out.is_empty()
        && !out.ends_with([' ', '\n', '\t'])
        && !fragment.starts_with([' ', '\n', '\t', ',', '.', ';', ':'])
    {
        out.push(' ');
    }
    out.push_str(fragment);
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !blank && !out.is_empty() {
                out.push('\n');
                blank = true;
            }
            continue;
        }
        if !out.is_empty() && !blank {
            out.push('\n');
        }
        out.push_str(trimmed);
        blank = false;
    }
    out
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn contains_slice(haystack: &[u8], needle: &[u8]) -> bool {
    find_subslice(haystack, needle).is_some()
}

fn trim_eol(payload: &[u8]) -> &[u8] {
    let mut end = payload.len();
    if end > 0 && payload[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && payload[end - 1] == b'\r' {
        end -= 1;
    }
    &payload[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pdf() -> Vec<u8> {
        let body = b"BT /F1 12 Tf 72 720 Td (Hello PDF) Tj ET";
        let stream = format!(
            "<< /Length {} >>\nstream\n{}\nendstream\n",
            body.len(),
            std::str::from_utf8(body).unwrap()
        );
        format!("%PDF-1.4\n{stream}%%EOF\n").into_bytes()
    }

    #[test]
    fn extracts_literal_strings() {
        let text = extract_pdf_text(&sample_pdf()).unwrap();
        assert!(text.contains("Hello PDF"));
        assert!(is_pdf_path("Customer_Request.PDF"));
        assert!(!is_pdf_path("notes.docx"));
    }
}

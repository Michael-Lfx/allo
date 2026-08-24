use super::*;


/// Cut a single leading/trailing code fence pair around a model reply (the
/// repair prompt forbids fences, but models still add them).
pub(super) fn strip_code_fences(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.lines().collect();
    if lines.first().is_some_and(|line| line.trim().starts_with("```")) {
        lines.remove(0);
    }
    if lines.last().is_some_and(|line| line.trim().starts_with("```")) {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}


/// Normalize a question prompt for duplicate comparison: lowercase and
/// collapse all whitespace runs into single spaces.
pub(super) fn normalize_prompt(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}


/// Strip Markdown code fences and prose around a lesson document. The
/// document stage must output the document itself, but models still wrap it
/// in ```markdown fences or a one-line preface; cut both, keeping every
/// document line intact. Document-internal fences (```svg / ```jsxgraph
/// figures, code samples) are tracked as pairs so only a fence that would
/// OPEN a new block can be treated as the wrapper's leftover half.
pub(super) fn strip_markdown_fences(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.lines().collect();
    // Drop the preface: keep from the first heading line onward.
    if let Some(at) = lines
        .iter()
        .position(|line| line.trim_start().starts_with("## "))
    {
        lines.drain(0..at);
    }
    // Remove a leftover leading fence line (``` or ```markdown).
    if lines.first().is_some_and(|line| line.trim().starts_with("```")) {
        lines.remove(0);
    }
    // A trailing wrapper fence (with optional commentary after it) marks the
    // end of the document. Walk the fence pairs: an in-block fence line
    // closes its block and is document content, so only the last fence found
    // while outside any block is a truncation candidate.
    let mut inside = false;
    let mut candidate = None;
    for (index, line) in lines.iter().enumerate() {
        if line.trim().starts_with("```") {
            if inside {
                candidate = None;
            } else {
                candidate = Some(index);
            }
            inside = !inside;
        }
    }
    // Content following the candidate fence may still be real document text
    // (an opener for a section that lost its closer); keep it whenever a
    // heading follows.
    if let Some(fence) = candidate {
        let trailing_has_heading = lines[fence + 1..]
            .iter()
            .any(|line| line.trim_start().starts_with('#'));
        if !trailing_has_heading {
            lines.truncate(fence);
        }
    }
    // Remove trailing empty lines.
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}


/// Parse the first JSON object in the raw model output (fences and prose
/// around it are tolerated). Extraction is string-aware so braces inside
/// string values — LaTeX formulas like `\frac{a}{b}`, code samples — never
/// terminate the object early. Candidate objects are tried in order, and a
/// failed parse is retried after repairing the common mistakes models make:
/// escaping errors (raw newlines, LaTeX backslashes) and trailing commas.
/// Shared with the reflection-grading parser in `service.rs`, which faces the
/// same fence/prose habits from the same models.
pub(crate) fn parse_json_object<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    let mut last_error = "no complete JSON object found".to_owned();
    let mut scan_from = 0usize;
    while let Some((start, end)) = find_json_object_bounds(raw, scan_from) {
        let slice = &raw[start..=end];
        let parsed = serde_json::from_str(slice)
            .or_else(|_| serde_json::from_str(&repair_json_escapes(slice)))
            .or_else(|_| serde_json::from_str(&repair_json_trailing_commas(slice)))
            .or_else(|_| {
                serde_json::from_str(&repair_json_trailing_commas(&repair_json_escapes(slice)))
            });
        match parsed {
            Ok(value) => return Ok(value),
            Err(error) => last_error = format!("invalid JSON: {error}"),
        }
        scan_from = end + 1;
    }
    Err(last_error)
}


/// Locate the next top-level `{...}` object starting at or after `from`.
/// Braces inside string values (and their escapes) are skipped, so a string
/// ending in `}` or a stray `{`/`}` in surrounding prose never truncates or
/// poisons the candidate object.
fn find_json_object_bounds(raw: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = raw.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut start: Option<usize> = None;
    for (index, &byte) in bytes.iter().enumerate().skip(from) {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            b'}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        return start.map(|start| (start, index));
                    }
                }
            }
            _ => {}
        }
    }
    None
}


/// Repair escaping mistakes models make with special characters inside JSON
/// string values. Only invoked when the standard parse of a candidate object
/// fails, so valid JSON is never touched:
/// - raw control characters (real newlines/tabs) become JSON escapes;
/// - `\` before an invalid escape character (LaTeX commands like `\alpha`,
///   `\{`) is doubled so the text stays literal;
/// - `\b`/`\f` are valid JSON escapes but course text virtually never means
///   backspace/form-feed; followed by letters they are LaTeX commands
///   (`\begin`, `\frac`) and the backslash is kept literal.
fn repair_json_escapes(slice: &str) -> String {
    let mut out = String::with_capacity(slice.len() + 16);
    let mut chars = slice.chars().peekable();
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if !in_string {
            out.push(ch);
            if ch == '"' {
                in_string = true;
            }
            continue;
        }
        match ch {
            '"' => {
                out.push('"');
                in_string = false;
            }
            '\\' => match chars.next() {
                // Valid JSON escapes pass through untouched.
                Some(next @ ('"' | '\\' | '/' | 'n' | 'r' | 't' | 'u')) => {
                    out.push('\\');
                    out.push(next);
                }
                // `\b`/`\f` followed by letters are LaTeX commands, not
                // control-character escapes.
                Some(next @ ('b' | 'f')) => {
                    let latex = matches!(chars.peek(), Some(c) if c.is_ascii_alphabetic());
                    if latex {
                        out.push('\\');
                    }
                    out.push('\\');
                    out.push(next);
                }
                // Unknown escape: double the backslash to keep it literal.
                Some(next) => {
                    out.push('\\');
                    out.push('\\');
                    out.push(next);
                }
                None => out.push('\\'),
            },
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}


/// Remove trailing commas before `}`/`]` — one of the most habitual JSON
/// mistakes models make. String-aware so a comma inside a string value is
/// kept; only invoked after the standard parse of a candidate object fails.
fn repair_json_trailing_commas(slice: &str) -> String {
    let mut out = String::with_capacity(slice.len());
    let mut chars = slice.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == ',' {
            // Look past whitespace: a comma directly before a closing
            // brace/bracket is a trailing comma and is dropped.
            let mut ahead = chars.clone();
            let mut trailing = false;
            for next in ahead.by_ref() {
                if next.is_whitespace() {
                    continue;
                }
                trailing = next == '}' || next == ']';
                break;
            }
            if trailing {
                continue;
            }
        }
        out.push(ch);
    }
    out
}


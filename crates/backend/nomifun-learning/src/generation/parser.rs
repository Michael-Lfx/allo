use super::*;


/// Cut a single leading/trailing code fence pair around a model reply (the
/// repair prompt forbids fences, but models still add them).
pub(crate) fn strip_code_fences(raw: &str) -> String {
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
        match parse_candidate::<T>(slice) {
            Ok(value) => return Ok(value),
            Err(error) => last_error = format!("invalid JSON: {error}"),
        }
        scan_from = end + 1;
    }
    // No complete object anywhere: the reply was probably cut mid-JSON by a
    // provider output cap. Salvage what the model finished by closing the
    // dangling array/object — a partial graph that passes the audit gate is
    // still a usable graph, and far better than a hard failure.
    if let Some(candidate) = recover_truncated_object(raw) {
        if let Ok(value) = parse_candidate::<T>(&candidate) {
            return Ok(value);
        }
    }
    Err(format!("{last_error}; {}", reply_diagnostic(raw)))
}


/// Parse one candidate slice with the standard repair chain (escape repair,
/// then trailing commas). Returns the deserialization error when every
/// repair fails.
fn parse_candidate<T: DeserializeOwned>(slice: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str::<T>(slice)
        .or_else(|_| serde_json::from_str::<T>(&repair_json_escapes(slice)))
        .or_else(|_| serde_json::from_str::<T>(&repair_json_trailing_commas(slice)))
        .or_else(|_| {
            serde_json::from_str::<T>(&repair_json_trailing_commas(&repair_json_escapes(slice)))
        })
}


/// Try to salvage a reply truncated mid-JSON: cut the prose before the first
/// `{`, then try the common closing sequences in turn (the array+object
/// closer first — most truncations die inside the concepts array; a trailing
/// comma is then dropped by the repair chain). When the cut is mid-object
/// (a dangling `"min"` with no value) no closer can fix it, but the units
/// before the cut are intact — retry from the last completed object. A reply
/// cut mid-string stays unparseable and yields None.
fn recover_truncated_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let tail = raw[start..].trim_end();
    for closer in ["]}]}", "]}", "}", "}}"] {
        let candidate = format!("{tail}{closer}");
        if parse_candidate::<serde_json::Value>(&candidate).is_ok() {
            return Some(candidate);
        }
    }
    if let Some(last_brace) = tail.rfind('}') {
        let trimmed = tail[..=last_brace].trim_end();
        for closer in ["]}", "}"] {
            let candidate = format!("{trimmed}{closer}");
            if parse_candidate::<serde_json::Value>(&candidate).is_ok() {
                return Some(candidate);
            }
        }
    }
    None
}


/// Compact facts about a model reply for error messages: byte length, brace
/// and bracket balance, and the opening characters — enough to tell a
/// truncated JSON (more opens than closes) from a non-JSON reply (no braces)
/// without dumping the whole output into the UI.
fn reply_diagnostic(raw: &str) -> String {
    let opens = raw.matches('{').count();
    let closes = raw.matches('}').count();
    let opens_bracket = raw.matches('[').count();
    let closes_bracket = raw.matches(']').count();
    let head: String = raw.chars().take(120).collect();
    format!(
        "reply is {} chars, {opens} '{{' / {closes} '}}', {opens_bracket} '[' / {closes_bracket} ']'; head: {head:?}",
        raw.len()
    )
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_object_handles_fences_and_prose_wrapped_objects() {
        let raw = "Here is the graph:\n```json\n{\"concepts\": [{\"name\": \"A\", \"min\": 10}]}\n```\n\nEnjoy!";
        let value: serde_json::Value = parse_json_object(raw).unwrap();
        assert_eq!(value["concepts"][0]["name"], "A");
    }

    #[test]
    fn parse_json_object_recovers_a_reply_cut_inside_the_concepts_array() {
        // A provider output cap cut the reply between two units; the cut is
        // right after a finished unit object, so appending the closers fixes it.
        let raw = r#"{"concepts": [
            {"name": "A", "pre": [], "min": 10},
            {"name": "B", "pre": ["A"], "min": 15},
            {"name": "C", "pre": ["B"], "min": 20}"#;
        let value: serde_json::Value = parse_json_object(raw).unwrap();
        let concepts = value["concepts"].as_array().unwrap();
        assert_eq!(concepts.len(), 3, "finished units survive the cut");
        assert_eq!(concepts[2]["min"], 20);
    }

    #[test]
    fn parse_json_object_recovers_a_reply_cut_mid_unit_object() {
        // The cut landed inside the last unit (a dangling "min" key without a
        // value) — no closer can finish that object, but the units before it
        // are intact and are kept by retrying from the last completed object.
        let raw = r#"{"concepts": [
            {"name": "A", "pre": [], "min": 10},
            {"name": "B", "pre": ["A"], "min": 15},
            {"name": "C", "pre": ["B"], "min""#;
        let value: serde_json::Value = parse_json_object(raw).unwrap();
        let concepts = value["concepts"].as_array().unwrap();
        assert_eq!(concepts.len(), 2, "the intact units before the cut survive");
        assert_eq!(concepts[1]["name"], "B");
    }

    #[test]
    fn parse_json_object_recovers_a_reply_cut_inside_a_pre_list() {
        let raw = r#"{"concepts": [{"name": "A", "pre": ["B", "C""#;
        let value: serde_json::Value = parse_json_object(raw).unwrap();
        assert_eq!(value["concepts"][0]["pre"][1], "C");
    }

    #[test]
    fn parse_json_object_gives_up_on_a_reply_cut_mid_string() {
        let raw = r#"{"concepts": [{"name": "用配方"#;
        let err = parse_json_object::<serde_json::Value>(raw).unwrap_err();
        assert!(err.contains("no complete JSON object found"), "{err}");
        assert!(err.contains("head:"), "{err}");
    }

    #[test]
    fn parse_json_object_reports_structural_diagnostics_on_non_json() {
        let err = parse_json_object::<serde_json::Value>(
            "The plan: 1. 学配方法 2. 学求根公式 3. 学判别式",
        )
        .unwrap_err();
        assert!(err.contains("no complete JSON object found"), "{err}");
        assert!(err.contains("reply is"), "{err}");
        assert!(err.contains("0 '{' / 0 '}'"), "{err}");
        assert!(err.contains("head:"), "{err}");
    }

    #[test]
    fn parse_json_object_keeps_the_invalid_json_error_for_unparseable_objects() {
        // An object EXISTS but is broken beyond the repair chain; the error
        // must say so instead of falling back to the truncation message.
        let raw = r#"{"concepts": [{"name": "A", "pre": ["B", , ]}]}"#;
        let err = parse_json_object::<serde_json::Value>(raw).unwrap_err();
        assert!(err.starts_with("invalid JSON"), "{err}");
        assert!(err.contains("head:"), "{err}");
    }

    #[test]
    fn repair_json_escapes_keeps_latex_backslashes_literal() {
        let fixed = repair_json_escapes(r#"{"n": "用 \alpha 推导"}"#);
        assert_eq!(fixed, r#"{"n": "用 \\alpha 推导"}"#);
        assert!(serde_json::from_str::<serde_json::Value>(&fixed).is_ok());
    }
}


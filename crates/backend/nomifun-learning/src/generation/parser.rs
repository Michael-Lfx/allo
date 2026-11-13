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
pub(crate) fn normalize_prompt(text: &str) -> String {
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
/// mermaid 节点/边文本含 | { } 等特殊字符且未整体双引号包裹时自动补引号
/// ——learnhub 实测的「渲染降级为源码」高频根因,确定性后处理零风险
/// (只动含高信号字符且未带引号的方括号标签)。
pub(crate) fn fix_mermaid_quotes(body: &str) -> String {
    let mut fixed = Vec::with_capacity(body.lines().count());
    let mut in_mermaid = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_mermaid {
                in_mermaid = false;
            } else if trimmed[3..].trim() == "mermaid" {
                in_mermaid = true;
            }
            fixed.push(line.to_owned());
            continue;
        }
        if !in_mermaid {
            fixed.push(line.to_owned());
            continue;
        }
        fixed.push(fix_mermaid_line(line));
    }
    fixed.join("\n")
}

fn fix_mermaid_line(line: &str) -> String {
    let open = match line.find('[') {
        Some(at) => at,
        None => return line.into(),
    };
    let close = match line.rfind(']') {
        Some(at) => at,
        None => return line.into(),
    };
    if close <= open + 1 {
        return line.into();
    }
    let label = &line[open + 1..close];
    let high_signal = label.contains(['|', '{', '"', '#']);
    let already_quoted = label.trim_start().starts_with('"');
    if !high_signal || already_quoted {
        return line.into();
    }
    format!("{}[\"{}\"]{}", &line[..open + 1], label, &line[close..])
}

pub(crate) fn parse_json_object<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    let mut last_error = "no complete JSON object found".to_owned();
    let mut scan_from = 0usize;
    while let Some((start, end)) = find_json_object_bounds(raw, scan_from) {
        let slice = &raw[start..=end];
        match parse_candidate::<T>(slice) {
            (Ok(value), _) => return Ok(value),
            (Err(last), first) => {
                // 窗口定位用**首次**未修复解析的 line/column：修复链会重排
                // 字节（转义/尾逗号），后续错误的位置不再对应原文。
                let first = first.unwrap_or(last);
                last_error = format!(
                    "invalid JSON: {first}; window: {}",
                    parse_failure_window(raw, start, first.line(), first.column())
                );
            }
        }
        scan_from = end + 1;
    }
    // No complete object anywhere: the reply was probably cut mid-JSON by a
    // provider output cap. Salvage what the model finished by closing the
    // dangling array/object — a partial graph that passes the audit gate is
    // still a usable graph, and far better than a hard failure.
    if let Some(candidate) = recover_truncated_object(raw) {
        if let (Ok(value), _) = parse_candidate::<T>(&candidate) {
            return Ok(value);
        }
    }
    Err(format!("{last_error}; {}", reply_diagnostic(raw)))
}


/// 解析失败窗口半径（字符；±120，learnhub `FEEDBACK_WINDOW_RADIUS` 同值）：
/// 中文正文一字符三字节，窗口必须按**字符**收口，截口永不落在字符中间。
const PARSE_FAILURE_WINDOW_RADIUS: usize = 120;


/// 解析失败的外围窗口：末次候选对象在原文中的绝对偏移 ±120 **字符**，
/// 带 `line/column` 标注。`line`/`column` 是 serde 相对候选切片的 1 基
/// 行号与**字节**列；换算顺序 = 切片内行首字节 + 列减一 → 退到 UTF-8 字符
/// 边界 → 加切片起点得到绝对偏移。越界一律钳到文末，绝不 panic。
pub(crate) fn parse_failure_window(
    raw: &str,
    slice_start: usize,
    line: usize,
    column: usize,
) -> String {
    let slice_start = slice_start.min(raw.len());
    let offset = (slice_start + offset_of_line_col(&raw[slice_start..], line, column))
        .min(raw.len());
    let start = step_back_chars(raw, offset, PARSE_FAILURE_WINDOW_RADIUS);
    let end = step_forward_chars(raw, offset, PARSE_FAILURE_WINDOW_RADIUS);
    format!("line {line} column {column}: …{}…", &raw[start..end])
}


/// 1 基 `line/column`（serde 语义：列按**行内字节**）→ 文本内字节偏移；
/// 越界取所在行末/文末，偏移退到 UTF-8 字符边界。
fn offset_of_line_col(text: &str, line: usize, column: usize) -> usize {
    let mut current_line = 1usize;
    let mut line_start = 0usize;
    for (index, ch) in text.char_indices() {
        if current_line == line {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            line_start = index + 1;
        }
    }
    if current_line != line {
        return text.len();
    }
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |position| line_start + position);
    let mut offset = (line_start + column.saturating_sub(1)).min(line_end);
    while offset > line_start && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}


/// 从字节偏移向前退 `count` 个字符（不越文首，永不破字符）。
fn step_back_chars(text: &str, from: usize, count: usize) -> usize {
    let mut at = from.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    for _ in 0..count {
        match text[..at].char_indices().next_back() {
            Some((index, _)) => at = index,
            None => break,
        }
    }
    at
}


/// 从字节偏移向后进 `count` 个字符（不越文末，永不破字符）。
fn step_forward_chars(text: &str, from: usize, count: usize) -> usize {
    let mut at = from.min(text.len());
    while at < text.len() && !text.is_char_boundary(at) {
        at += 1;
    }
    for _ in 0..count {
        match text[at..].chars().next() {
            Some(ch) => at += ch.len_utf8(),
            None => break,
        }
    }
    at
}


/// 解析一份候选切片：返回 `(最终结果, 首次 serde 错误)`。首次错误（未修复
/// 原文）是窗口定位的唯一可信位置源——修复链的转义/尾逗号重写会移动字节。
fn parse_candidate<T: DeserializeOwned>(
    slice: &str,
) -> (Result<T, serde_json::Error>, Option<serde_json::Error>) {
    let first_error = match serde_json::from_str::<T>(slice) {
        Ok(value) => return (Ok(value), None),
        Err(error) => error,
    };
    let outcome = serde_json::from_str::<T>(&repair_json_escapes(slice))
        .or_else(|_| serde_json::from_str::<T>(&repair_json_trailing_commas(slice)))
        .or_else(|_| {
            serde_json::from_str::<T>(&repair_json_trailing_commas(&repair_json_escapes(slice)))
        });
    (outcome, Some(first_error))
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
        if parse_candidate::<serde_json::Value>(&candidate).0.is_ok() {
            return Some(candidate);
        }
    }
    if let Some(last_brace) = tail.rfind('}') {
        let trimmed = tail[..=last_brace].trim_end();
        for closer in ["]}", "}"] {
            let candidate = format!("{trimmed}{closer}");
            if parse_candidate::<serde_json::Value>(&candidate).0.is_ok() {
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
        assert!(
            !err.contains("window:"),
            "截断无可定位对象时走通用兜底，不误报窗口：{err}"
        );
    }

    /// parse 失败窗口（y02 红绿样）：中文字段之间缺逗号——serde 的
    /// `column` 是行内**字节**偏移，按字符解释会落到 1/3 处；窗口必须覆盖
    /// 真正的出错点（紧跟值之后的 `"next"`），且截口不破 UTF-8（切片越界
    /// 或落在字符中间会直接 panic）。
    #[test]
    fn parse_failure_window_locates_the_error_inside_bytes_heavy_chinese() {
        let long_chinese = "先把常数项移到等号右边，再把两边同时加上一次项系数一半的平方，".repeat(100);
        let raw = format!(
            r#"{{"name": "{long_chinese}", "min": 10 "next": "尾"}}"#
        );
        let err = parse_json_object::<serde_json::Value>(&raw).unwrap_err();
        assert!(err.starts_with("invalid JSON"), "{err}");
        assert!(err.contains("line 1 column"), "{err}");
        assert!(err.contains("window:"), "错误串必须带出错窗口：{err}");
        assert!(
            err.contains("\"next\""),
            "窗口必须覆盖真正的出错点（缺逗号后的 next 键）：{err}"
        );
        assert!(
            err.contains("一次项系数一半的平方"),
            "窗口必须带出错点前后的中文正文：{err}"
        );
        let window = err.split("window: ").nth(1).expect("window present");
        let snippet = window.split('…').nth(1).expect("window is delimited");
        assert!(
            snippet.chars().count() <= 2 * 120,
            "窗口是 ±120 字符，不得退化成全文：{} chars",
            snippet.chars().count()
        );
    }

    /// 围栏/散文包裹的坏对象：窗口按候选切片的**绝对偏移**定位——散文前缀
    /// 超过窗口半径时，忽略 `slice_start` 会把窗口钉在散文开头而不是出错点。
    #[test]
    fn parse_failure_window_uses_the_candidate_slice_offset() {
        let prose = format!(
            "起始标记X{}",
            "这是一段很长的解释性散文，本不该出现在回包里。".repeat(20)
        );
        let raw = format!(
            "{prose}\n```json\n{{\"concepts\": [{{\"name\": \"甲\", \"min\": 10 \"next\": 1}}]}}\n```\n"
        );
        let err = parse_json_object::<serde_json::Value>(&raw).unwrap_err();
        assert!(err.contains("window:"), "{err}");
        let window = err.split("window: ").nth(1).expect("window present");
        let window = window.split("; reply is").next().expect("window is bounded");
        assert!(
            window.contains("\"next\""),
            "窗口必须落在对象内的出错点：{err}"
        );
        assert!(
            !window.contains("起始标记X"),
            "窗口不得被散文前缀拉走（slice_start 必须计入绝对偏移）：{err}"
        );
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
        assert!(
            !err.contains("window:"),
            "没有对象就没有定位窗口（通用兜底）：{err}"
        );
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

    /// f3 邻域补测（fix_mermaid_quotes 原为零单测）：mermaid 围栏内高信号
    /// 字符且未引号包裹的方括号标签自动补引号；已引号标签与非 mermaid 围栏
    /// 原样保留（引号卫生先于 lint，行为不因 f3 改变）。
    #[test]
    fn fix_mermaid_quotes_wraps_high_signal_labels_only_inside_mermaid() {
        let raw = "前文\n```mermaid\nflowchart TD\nA[本金|利息] --> B{开关}\n```\n```svg\n<text>|</text>\n```\n后文\n";
        let fixed = fix_mermaid_quotes(raw);
        assert!(fixed.contains("A[[\"本金|利息\"]]"), "{fixed}");
        // B{开关} 不在 [ ] 区间内，原样保留。
        assert!(fixed.contains(" --> B{开关}"), "{fixed}");
        // svg 围栏不受引号卫生影响。
        assert!(fixed.contains("<text>|</text>"), "{fixed}");
    }

    #[test]
    fn fix_mermaid_quotes_leaves_quoted_and_plain_lines_untouched() {
        let raw = "```mermaid\nflowchart LR\nA[\"已引号|文本\"] --> B[普通]\n```\n正文段落 | 不是围栏\n";
        let fixed = fix_mermaid_quotes(raw);
        assert!(fixed.contains("A[\"已引号|文本\"]"), "{fixed}");
        assert!(fixed.contains("B[普通]"), "{fixed}");
        assert!(fixed.contains("正文段落 | 不是围栏"), "{fixed}");
    }
}


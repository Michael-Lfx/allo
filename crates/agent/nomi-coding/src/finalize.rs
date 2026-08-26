//! User-facing finalize text for coding hard-stops / forced EndTurn.
//!
//! When the harness clears tools for a closing provider pass, models often
//! still emit tool-call markup (`<tool_call>…`) or compact-style `<summary>`
//! tags as plain text. That markup is machine protocol, not a reply — strip
//! it before the host shows the turn to the user.

/// Turn-tail instruction for a forced finalize provider pass (no tools).
pub fn forced_finalize_instruction(reason: &str) -> String {
    format!(
        "{reason}\n\n\
         Write a concise final reply for the user now in plain prose (markdown \
         is fine). Do not call tools. Do not emit XML/HTML tags, `<tool_call>` \
         markup, `<summary>` blocks, JSON tool envelopes, or internal policy \
         jargon — only what the user should read."
    )
}

/// Friendly fallback when the finalize pass produces no usable prose.
pub const FRIENDLY_FINALIZE_FALLBACK: &str = "本轮已停止继续调用工具。请根据上方工具结果确认进度；如需继续请再发一条消息。";

/// Strip machine-protocol markup from a closing assistant reply.
///
/// Returns the cleaned text (may be empty when the model only emitted markup).
pub fn sanitize_user_facing_reply(raw: &str) -> String {
    let without_tool_calls = strip_tag_blocks(raw, "tool_call");
    let without_function = strip_tag_blocks(&without_tool_calls, "function_calls");
    let without_invoke = strip_tag_blocks(&without_function, "invoke");
    // Compact history teaches `<summary>`; unwrap inner text if present, else drop tags.
    let unwrapped = unwrap_or_strip_tag(&without_invoke, "summary");
    let without_analysis = strip_tag_blocks(&unwrapped, "analysis");
    collapse_blank_lines(&without_analysis).trim().to_string()
}

/// Prefer sanitized model prose; otherwise a short user-readable fallback.
pub fn finalize_reply_or_fallback(raw: &str) -> String {
    let cleaned = sanitize_user_facing_reply(raw);
    if cleaned.is_empty() {
        FRIENDLY_FINALIZE_FALLBACK.to_string()
    } else {
        cleaned
    }
}

fn strip_tag_blocks(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let open_lower = open.to_ascii_lowercase();
    let close_lower = close.to_ascii_lowercase();

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(start) = find_ascii_ci(rest, &open_lower) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        if let Some(end) = find_ascii_ci(after_open, &close_lower) {
            rest = &after_open[end + close.len()..];
        } else {
            // Unclosed markup: drop the opener and the remainder (protocol junk).
            break;
        }
    }
    out
}

fn unwrap_or_strip_tag(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let open_lower = open.to_ascii_lowercase();
    let close_lower = close.to_ascii_lowercase();

    let Some(start) = find_ascii_ci(text, &open_lower) else {
        return text.to_string();
    };
    let after_open = &text[start + open.len()..];
    let Some(end) = find_ascii_ci(after_open, &close_lower) else {
        return strip_tag_blocks(text, tag);
    };
    let inner = after_open[..end].trim();
    let before = text[..start].trim();
    let after = after_open[end + close.len()..].trim();
    let mut parts = Vec::new();
    if !before.is_empty() {
        parts.push(before);
    }
    if !inner.is_empty() {
        parts.push(inner);
    }
    if !after.is_empty() {
        parts.push(after);
    }
    parts.join("\n\n")
}

fn find_ascii_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle_lower.len())
        .position(|window| {
            window.len() == needle_lower.len()
                && window
                    .iter()
                    .zip(needle_lower.bytes())
                    .all(|(a, b)| a.to_ascii_lowercase() == b)
        })
}

fn collapse_blank_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_blank = false;
    for line in text.lines() {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        prev_blank = blank;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tool_call_markup() {
        let raw = "Done.\n<tool_call>{\"name\":\"Bash\",\"arguments\":{}}</tool_call>\n";
        assert_eq!(sanitize_user_facing_reply(raw), "Done.");
    }

    #[test]
    fn unwraps_summary_tags() {
        let raw = "<summary>\nFixed the bug in foo.rs.\n</summary>";
        assert_eq!(sanitize_user_facing_reply(raw), "Fixed the bug in foo.rs.");
    }

    #[test]
    fn drops_unclosed_tool_markup() {
        let raw = "prefix <tool_call>{\"name\":\"Read\"";
        assert_eq!(sanitize_user_facing_reply(raw), "prefix");
    }

    #[test]
    fn fallback_when_only_markup() {
        let raw = "<tool_call>{\"name\":\"Bash\"}</tool_call>";
        assert_eq!(finalize_reply_or_fallback(raw), FRIENDLY_FINALIZE_FALLBACK);
    }

    #[test]
    fn instruction_forbids_markup() {
        let text = forced_finalize_instruction("Coding explore hard-stop");
        assert!(text.contains("plain prose"));
        assert!(text.contains("<tool_call>"));
        assert!(text.contains("Coding explore hard-stop"));
    }
}

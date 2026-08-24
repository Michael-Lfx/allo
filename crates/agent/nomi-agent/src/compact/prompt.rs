//! Compact prompt templates for LLM-based conversation summarization.
//!
//! Provides the 7-section next-turn briefing prompt, response parsing, and
//! post-compact message construction.

/// System prompt used for the compact LLM call.
///
/// Tells the model that user turns are kept verbatim alongside the summary,
/// so its job is to fold the assistant/tool work — and any prior compact
/// briefing — into a briefing the agent can resume from.
pub const COMPACT_SYSTEM_PROMPT: &str = "You are compacting the earlier part of a coding agent's \
    conversation to save context. The agent keeps your summary alongside the user's own turns \
    (kept verbatim) and the recent tail; your job is to fold the assistant/tool work into a \
    briefing it can resume from. If the conversation already contains a prior compact briefing, \
    merge it into this one; treat that briefing as prior agent state, not as a user request.";

// ── Prompt construction ─────────────────────────────────────────────────────

/// Build the 7-section compact prompt that asks the LLM for a next-turn briefing.
pub fn build_compact_prompt() -> String {
    format!("{PREAMBLE}\n\n{BODY}\n\n{FORMAT_INSTRUCTIONS}\n\n{REMINDER}")
}

const PREAMBLE: &str = "\
CRITICAL: Respond with TEXT ONLY. Do NOT call any tools.
- Do NOT use Read, Bash, Grep, Glob, Edit, Write, or ANY other tool.
- You already have all the context you need in the conversation above.
- Tool calls will be REJECTED and will waste your only turn — you will fail the task.
- Your entire response must be plain text inside a <summary> block.";

const BODY: &str = "\
Your task is to write a next-turn briefing: keep only what the agent still needs to continue \
the work. Do not try to capture everything. Drop resolved detours, duplicated file dumps, and details the \
recent tail already shows. The user's own messages are kept verbatim alongside your summary, \
so do NOT reproduce user messages.

The conversation may already contain a prior compact briefing (a message that starts with \
\"This session is being continued\" or similar). That briefing is prior agent state. Merge it \
into this one; do not ignore it as a user message to avoid restating.

Your summary should include the following sections:

1. **Standing Facts & Constraints**: Names, paths, IDs, versions, preferences, and hard \
\"never do X\" rules that still govern the work.
2. **Goal**: The user's request and intent.
3. **Decisions & Rationale**: Key choices made so far and why — so they are not re-litigated \
or reversed.
4. **Files & Code**: Files read or modified, with the specific facts that still matter: \
signatures, line locations, data shapes, and exact edits applied.
5. **Commands & Outcomes**: Commands run (builds, tests, git) and the results that still \
matter — what passed, what failed, and the error text that is still relevant.
6. **Errors & Fixes**: Problems hit and how they were resolved (or not), so the same dead ends \
are not repeated.
7. **Pending & Next Step**: What is still in progress or unstarted, and the single most concrete \
next action to take.";

const FORMAT_INSTRUCTIONS: &str = "\
Format your response exactly as follows:

<summary>
Your structured next-turn briefing following the 7 sections above
</summary>";

const REMINDER: &str = "\
REMINDER: Do NOT call any tools. Respond with plain text only — a <summary> block. \
If a prior compact briefing is present, merge it. Tool calls will be rejected and you will fail the task.";

// ── Response parsing ────────────────────────────────────────────────────────

/// Parse the raw LLM response: strip `<analysis>`, extract `<summary>` content.
///
/// If no `<summary>` tags are found, returns the raw text as-is (graceful degradation).
pub fn format_compact_summary(raw: &str) -> String {
    // Step 1: remove <analysis>...</analysis>
    let without_analysis = strip_tag(raw, "analysis");

    // Step 2: extract <summary>...</summary> content
    if let Some(summary_content) = extract_tag_content(&without_analysis, "summary") {
        let trimmed = summary_content.trim();
        if trimmed.is_empty() {
            return collapse_blank_lines(&without_analysis).trim().to_string();
        }
        format!("Summary:\n{trimmed}")
    } else {
        // Graceful degradation: use the text with analysis stripped
        collapse_blank_lines(&without_analysis).trim().to_string()
    }
}

// ── Post-compact message content ────────────────────────────────────────────

/// Build the user message content for the post-compact summary.
///
/// For autocompact (`is_auto = true`), appends an instruction telling the
/// model to continue seamlessly without acknowledging the compaction.
pub fn build_summary_content(formatted_summary: &str, is_auto: bool) -> String {
    let mut content = String::from(
        "This session is being continued from a previous conversation that ran out of context. \
         The summary below covers the earlier portion of the conversation.\n\n",
    );
    content.push_str(formatted_summary);

    if is_auto {
        content.push_str(
            "\n\nContinue the conversation from where it left off without asking the user \
             any further questions. Resume directly — do not acknowledge the summary, \
             do not recap what was happening, do not preface with \"I'll continue\" or similar. \
             Pick up the last task as if the break never happened.",
        );
    }

    content
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Remove `<tag>...</tag>` (first occurrence) from text.
///
/// If the closing tag appears before the opening tag (reversed order),
/// the text is returned unchanged to avoid producing duplicate content.
fn strip_tag(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let Some(start) = text.find(&open) else {
        return text.to_string();
    };
    let Some(end) = text.find(&close) else {
        return text.to_string();
    };

    // Guard: closing tag before opening tag → no-op
    if end < start {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    result.push_str(&text[..start]);
    result.push_str(&text[end + close.len()..]);
    collapse_blank_lines(&result)
}

/// Extract the content between `<tag>` and `</tag>` (first occurrence).
fn extract_tag_content<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let start = text.find(&open)? + open.len();
    let end = text.find(&close)?;

    if start <= end {
        Some(&text[start..end])
    } else {
        None
    }
}

/// Collapse consecutive blank lines into a single blank line.
fn collapse_blank_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_blank = false;

    for line in text.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && prev_was_blank {
            continue;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        prev_was_blank = is_blank;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_compact_prompt ────────────────────────────────────────────

    #[test]
    fn prompt_contains_all_seven_sections() {
        let prompt = build_compact_prompt();
        for i in 1..=7 {
            assert!(prompt.contains(&format!("{i}.")), "Missing section {i}");
        }
        assert!(!prompt.contains("8."), "prompt should describe 7 sections, not 9");
        assert!(!prompt.contains("9 sections"));
    }

    #[test]
    fn prompt_forbids_tool_calls() {
        let prompt = build_compact_prompt();
        assert!(prompt.contains("Do NOT call any tools"));
        assert!(prompt.contains("CRITICAL"));
    }

    #[test]
    fn prompt_is_next_turn_briefing_without_analysis() {
        let prompt = build_compact_prompt();
        assert!(prompt.contains("<summary>"));
        assert!(!prompt.contains("<analysis>"));
        assert!(prompt.contains("next-turn briefing"));
        assert!(prompt.contains("Merge it"));
        assert!(!prompt.contains("thorough"));
        assert!(!prompt.contains("exhaustive"));
    }

    // ── format_compact_summary ──────────────────────────────────────────

    #[test]
    fn strips_analysis_extracts_summary() {
        let raw =
            "<analysis>thinking about things</analysis>\n<summary>the actual result</summary>";
        assert_eq!(format_compact_summary(raw), "Summary:\nthe actual result");
    }

    #[test]
    fn extracts_summary_without_analysis() {
        let raw = "<summary>result only</summary>";
        assert_eq!(format_compact_summary(raw), "Summary:\nresult only");
    }

    #[test]
    fn graceful_degradation_without_tags() {
        let raw = "plain text without any tags";
        assert_eq!(format_compact_summary(raw), "plain text without any tags");
    }

    #[test]
    fn handles_multiline_summary() {
        let raw =
            "<analysis>analysis\nwith lines</analysis>\n<summary>\nLine 1\nLine 2\n</summary>";
        let result = format_compact_summary(raw);
        assert!(result.starts_with("Summary:\n"));
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
    }

    #[test]
    fn empty_summary_tags_falls_back() {
        let raw = "<analysis>thinking</analysis>\n<summary></summary>";
        let result = format_compact_summary(raw);
        // Falls back since summary content is empty
        assert!(!result.is_empty());
    }

    // ── build_summary_content ───────────────────────────────────────────

    #[test]
    fn auto_summary_includes_continuation_instruction() {
        let content = build_summary_content("Summary:\ntest", true);
        assert!(content.contains("Continue the conversation"));
        assert!(content.contains("as if the break never happened"));
    }

    #[test]
    fn manual_summary_no_continuation_instruction() {
        let content = build_summary_content("Summary:\ntest", false);
        assert!(!content.contains("Continue the conversation"));
    }

    #[test]
    fn summary_content_includes_session_header() {
        let content = build_summary_content("Summary:\ntest", false);
        assert!(content.contains("This session is being continued"));
    }

    // ── strip_tag ───────────────────────────────────────────────────────

    #[test]
    fn strip_tag_removes_complete_tag() {
        let text = "before<foo>inside</foo>after";
        assert_eq!(strip_tag(text, "foo"), "beforeafter");
    }

    #[test]
    fn strip_tag_noop_when_tag_missing() {
        let text = "no tags here";
        assert_eq!(strip_tag(text, "foo"), "no tags here");
    }

    #[test]
    fn strip_tag_noop_when_reversed_order() {
        // Closing tag before opening tag should be treated as no-op
        let text = "before</foo>middle<foo>inside</foo>after";
        // The first </foo> is at position 6, first <foo> is at position 17
        // Since end < start, the text should be returned unchanged
        assert_eq!(strip_tag(text, "foo"), text);
    }

    // ── extract_tag_content ─────────────────────────────────────────────

    #[test]
    fn extract_existing_tag() {
        let text = "<summary>hello world</summary>";
        assert_eq!(extract_tag_content(text, "summary"), Some("hello world"));
    }

    #[test]
    fn extract_missing_tag() {
        let text = "no summary here";
        assert_eq!(extract_tag_content(text, "summary"), None);
    }

    // ── collapse_blank_lines ────────────────────────────────────────────

    #[test]
    fn collapses_multiple_blank_lines() {
        let text = "a\n\n\n\nb";
        let result = collapse_blank_lines(text);
        assert_eq!(result, "a\n\nb");
    }

    #[test]
    fn preserves_single_blank_line() {
        let text = "a\n\nb";
        assert_eq!(collapse_blank_lines(text), "a\n\nb");
    }
}

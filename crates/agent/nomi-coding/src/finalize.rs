//! User-facing finalize text for coding hard-stops / forced EndTurn.
//!
//! When the harness clears tools for a closing provider pass, models often
//! still emit tool-call markup (`<tool_call>…`) or compact-style `<summary>`
//! tags as plain text. That markup is machine protocol, not a reply — strip
//! it before the host shows the turn to the user.

use crate::todo_continuation::PlanSnapshot;

/// Turn-tail instruction for a forced finalize provider pass (no tools).
pub fn forced_finalize_instruction(reason: &str) -> String {
    forced_finalize_instruction_for_plan(reason, None)
}

/// Same as [`forced_finalize_instruction`], plus remaining plan steps when the
/// last accepted `update_plan` snapshot is still incomplete.
///
/// Used on the **reply** finalize pass (no tools). Remaining steps are facts
/// the model already declared — omitting them is how a hard-stop reply claimed
/// "done" while the UI still showed open todos.
pub fn forced_finalize_instruction_for_plan(
    reason: &str,
    remaining: Option<&PlanSnapshot>,
) -> String {
    let mut text = format!(
        "{reason}\n\n\
         Write a concise final reply for the user now in plain prose (markdown \
         is fine). Do not call tools. Do not emit XML/HTML tags, `<tool_call>` \
         markup, `<summary>` blocks, JSON tool envelopes, or internal policy \
         jargon — only what the user should read."
    );
    append_remaining_plan(
        &mut text,
        remaining,
        "Do not tell the user the task is finished. Report what actually completed, \
         what is blocked, and the remaining steps. Do not invent completions.",
    );
    text
}

/// First forced-finalize pass: keep `update_plan` advertised so the model can
/// close a checklist it no longer has other tools to work on.
///
/// The host must not invent completions. This pass is the last chance for the
/// model to declare what actually happened (session
/// `01a0d19b-c5e8-7381-a811-a7b715bf058a` finished the work, then lost the
/// tool, and the UI kept four pending todos).
pub fn forced_finalize_plan_sync_instruction(
    reason: &str,
    remaining: Option<&PlanSnapshot>,
) -> String {
    let mut text = format!(
        "{reason}\n\n\
         Call `update_plan` once with an honest full snapshot of what actually \
         happened this turn. Mark a step completed only if you did that work; \
         leave blocked work pending. Do not invent completions. Do not call any \
         other tool. Do not write the user-facing reply yet."
    );
    append_remaining_plan(
        &mut text,
        remaining,
        "Sync the checklist to this remaining work, then stop calling tools.",
    );
    text
}

fn append_remaining_plan(text: &mut String, remaining: Option<&PlanSnapshot>, closer: &str) {
    let Some(plan) = remaining.filter(|plan| !plan.is_empty() && !plan.all_completed()) else {
        return;
    };
    let pending = plan.pending();
    let done = plan.steps.len() - pending.len();
    let total = plan.steps.len();
    let list = pending
        .iter()
        .enumerate()
        .map(|(i, step)| format!("  {}. [{}] {}", i + 1, step.status, step.content))
        .collect::<Vec<_>>()
        .join("\n");
    text.push_str(&format!(
        "\n\nThe declared plan still has {} uncompleted step(s) ({done}/{total} done):\n{list}\n\n\
         {closer}",
        pending.len()
    ));
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
        assert!(!text.contains("uncompleted step"));
    }

    #[test]
    fn instruction_lists_remaining_plan_and_forbids_claiming_done() {
        let remaining = PlanSnapshot {
            steps: vec![
                crate::todo_continuation::PlanStepView {
                    content: "Inspect renderer".into(),
                    status: "completed".into(),
                },
                crate::todo_continuation::PlanStepView {
                    content: "Write the patch".into(),
                    status: "in_progress".into(),
                },
                crate::todo_continuation::PlanStepView {
                    content: "Verify build".into(),
                    status: "pending".into(),
                },
            ],
        };
        let text = forced_finalize_instruction_for_plan(
            "Coding lifetime recon hard-stop",
            Some(&remaining),
        );
        assert!(text.contains("Write the patch"));
        assert!(text.contains("Verify build"));
        assert!(text.contains("Do not tell the user the task is finished"));
        assert!(!text.contains("Inspect renderer"));
    }

    #[test]
    fn plan_sync_instruction_asks_for_update_plan_not_a_reply() {
        let remaining = PlanSnapshot {
            steps: vec![crate::todo_continuation::PlanStepView {
                content: "Start the backend".into(),
                status: "in_progress".into(),
            }],
        };
        let text = forced_finalize_plan_sync_instruction(
            "Coding explore hard-stop",
            Some(&remaining),
        );
        assert!(text.contains("update_plan"));
        assert!(text.contains("Do not write the user-facing reply yet"));
        assert!(text.contains("Start the backend"));
        assert!(!text.contains("Write a concise final reply"));
    }
}

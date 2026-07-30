//! Advisor view of the conversation.
//!
//! Reference models receive a text-only projection of the live history:
//! system prompt stripped (they get their own advisory framing), assistant
//! tool calls inlined as text, tool results folded and truncated, and the
//! whole view trimmed to the advisor's context window. The projection keeps
//! user-first ordering and always ends on a user message carrying the
//! advisory instruction, so strict-alternation providers accept it.

use nomi_types::message::{ContentBlock, Message, Role};

use crate::compact::estimate::estimate_tokens_from_messages;

/// Synthetic final user message: tells the advisor what to do with the
/// projected state. Not part of the real history — the runner's turn
/// signature is computed on the original messages, never on this view.
pub const ADVISORY_INSTRUCTION: &str = "[The conversation above is the current state of the task. Give your most \
intelligent judgement: what is going on, what should happen next, what \
risks or mistakes you see, and how the acting agent should proceed.]";

/// Character budget for one folded tool result (head + tail halves).
const TOOL_RESULT_BUDGET: usize = 4000;
/// Ceiling on the inlined argument preview of one tool call.
const TOOL_CALL_ARGS_BUDGET: usize = 500;
/// Conservative window assumed when the host does not know the advisor's
/// real context size.
const DEFAULT_CONTEXT_WINDOW_TOKENS: u64 = 32_768;
/// Tokens reserved for the advisor's own output when trimming the view.
const OUTPUT_RESERVE_TOKENS: u64 = 8_192;
/// Never trim the input budget below this floor.
const MIN_INPUT_BUDGET_TOKENS: u64 = 2_048;

/// Keep the head and tail of an oversized text, eliding the middle.
fn truncate_middle(text: &str, budget: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= budget {
        return text.to_string();
    }
    let half = budget / 2;
    let head: String = chars[..half].iter().collect();
    let tail: String = chars[chars.len() - half..].iter().collect();
    let omitted = chars.len() - half * 2;
    format!("{head}\n[... {omitted} chars omitted ...]\n{tail}")
}

/// Render one source message into plain advisor text. Empty string → the
/// frame carries nothing an advisor can use and is dropped.
fn render_message_text(msg: &Message) -> String {
    let mut parts: Vec<String> = Vec::new();
    for block in &msg.content {
        match block {
            ContentBlock::Text { text } => {
                if !text.trim().is_empty() {
                    parts.push(text.clone());
                }
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let args = truncate_middle(&input.to_string(), TOOL_CALL_ARGS_BUDGET);
                parts.push(format!("[called tool: {name}({args})]"));
            }
            ContentBlock::ToolResult { content, is_error, .. } => {
                let body = truncate_middle(content, TOOL_RESULT_BUDGET);
                if *is_error {
                    parts.push(format!("[tool result (error): {body}]"));
                } else {
                    parts.push(format!("[tool result: {body}]"));
                }
            }
            // Advisors get conclusions, not the acting model's private
            // reasoning stream.
            ContentBlock::Thinking { .. } => {}
            ContentBlock::Image { .. } => parts.push("[image attachment]".to_string()),
        }
    }
    parts.join("\n")
}

/// Build the untrimmed advisor view from live history. See module docs for
/// the projection rules.
pub fn build_advisory_view(messages: &[Message]) -> Vec<Message> {
    let mut view: Vec<Message> = Vec::new();
    for msg in messages {
        // Advisors receive their own system framing; the acting agent's
        // system prompt (tool docs, persona) would only mislead them.
        let role = match msg.role {
            Role::System => continue,
            Role::Assistant => Role::Assistant,
            // Tool results ride user-role messages in this codebase; a
            // dedicated Tool role folds the same way.
            Role::User | Role::Tool => Role::User,
        };
        let text = render_message_text(msg);
        if text.is_empty() {
            continue;
        }
        // Coalesce consecutive same-role frames so strict-alternation
        // providers (Anthropic) accept the projection.
        if let Some(last) = view.last_mut() {
            if last.role == role {
                if let Some(ContentBlock::Text { text: existing }) = last.content.first_mut() {
                    existing.push_str("\n\n");
                    existing.push_str(&text);
                    continue;
                }
            }
        }
        view.push(Message::new(role, vec![ContentBlock::Text { text }]));
    }

    // User-first: drop leading assistant frames (an advisor view opening on
    // an assistant frame reads as the advisor's own words).
    while view.first().map(|m| m.role) == Some(Role::Assistant) {
        view.remove(0);
    }

    // Always end on a user message carrying the advisory instruction.
    match view.last_mut() {
        Some(last) if last.role == Role::User => {
            if let Some(ContentBlock::Text { text }) = last.content.first_mut() {
                text.push_str("\n\n");
                text.push_str(ADVISORY_INSTRUCTION);
            }
        }
        _ => view.push(Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: ADVISORY_INSTRUCTION.to_string(),
            }],
        )),
    }
    view
}

/// Trim a built view to an advisor's context window: drop the oldest frames
/// first, then restore user-first ordering. The final (instruction-bearing)
/// frame is always kept.
pub fn trim_view_to_window(mut view: Vec<Message>, window_tokens: Option<u64>) -> Vec<Message> {
    let window = window_tokens.unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS);
    let budget = window
        .saturating_sub(OUTPUT_RESERVE_TOKENS)
        .max(MIN_INPUT_BUDGET_TOKENS);
    while view.len() > 1 && estimate_tokens_from_messages(&view) > budget {
        view.remove(0);
    }
    while view.len() > 1 && view.first().map(|m| m.role) == Some(Role::Assistant) {
        view.remove(0);
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_of(msg: &Message) -> &str {
        match msg.content.first() {
            Some(ContentBlock::Text { text }) => text,
            _ => panic!("advisory view must contain only text blocks"),
        }
    }

    fn user_text(text: &str) -> Message {
        Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        )
    }

    #[test]
    fn strips_system_and_inlines_tool_use() {
        let messages = vec![
            Message::new(
                Role::System,
                vec![ContentBlock::Text {
                    text: "act as the agent".into(),
                }],
            ),
            user_text("please list files"),
            Message::new(
                Role::Assistant,
                vec![
                    ContentBlock::Text {
                        text: "Listing now.".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "Bash".into(),
                        input: json!({"cmd": "ls"}),
                        extra: None,
                    },
                ],
            ),
        ];
        let view = build_advisory_view(&messages);
        assert!(view.iter().all(|m| m.role != Role::System));
        assert!(!view.iter().any(|m| text_of(m).contains("act as the agent")));
        let assistant = view.iter().find(|m| m.role == Role::Assistant).unwrap();
        assert!(text_of(assistant).contains("Listing now."));
        assert!(text_of(assistant).contains("[called tool: Bash("));
        assert!(text_of(assistant).contains("\"cmd\":\"ls\""));
    }

    #[test]
    fn folds_and_truncates_tool_results() {
        let long = "x".repeat(10_000);
        let messages = vec![
            user_text("run it"),
            Message::new(
                Role::Assistant,
                vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "Bash".into(),
                    input: json!({}),
                    extra: None,
                }],
            ),
            Message::new(
                Role::User,
                vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: long,
                    is_error: false,
                    images: Vec::new(),
                }],
            ),
        ];
        let view = build_advisory_view(&messages);
        let folded = text_of(view.last().unwrap());
        assert!(folded.contains("[tool result: "));
        assert!(folded.contains("chars omitted"));
        // Head + tail + marker stays well under the raw 10k characters.
        assert!(folded.len() < 6_000);
    }

    #[test]
    fn user_first_and_ends_on_user_with_instruction() {
        // Leading assistant frame (e.g. everything earlier was compacted away).
        let messages = vec![
            Message::new(
                Role::Assistant,
                vec![ContentBlock::Text {
                    text: "stale opener".into(),
                }],
            ),
            user_text("question"),
            Message::new(
                Role::Assistant,
                vec![ContentBlock::Text {
                    text: "partial answer".into(),
                }],
            ),
        ];
        let view = build_advisory_view(&messages);
        assert_eq!(view.first().unwrap().role, Role::User);
        assert!(!text_of(view.first().unwrap()).contains("stale opener"));
        let last = view.last().unwrap();
        assert_eq!(last.role, Role::User);
        assert!(text_of(last).contains("Give your most"));
    }

    #[test]
    fn instruction_merges_into_trailing_user_message() {
        let view = build_advisory_view(&[user_text("only question")]);
        assert_eq!(view.len(), 1);
        assert!(text_of(&view[0]).starts_with("only question"));
        assert!(text_of(&view[0]).contains(ADVISORY_INSTRUCTION));
    }

    #[test]
    fn empty_history_yields_single_instruction_frame() {
        let view = build_advisory_view(&[]);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].role, Role::User);
        assert_eq!(text_of(&view[0]), ADVISORY_INSTRUCTION);
    }

    #[test]
    fn trim_drops_oldest_frames_and_keeps_user_first() {
        // Each frame ≈ 2.5k tokens (10k chars / 4); a 12k-token window with
        // an 8k output reserve leaves ~4k input, so only the newest frames fit.
        let bulk = "y".repeat(10_000);
        let mut messages = Vec::new();
        for i in 0..6 {
            messages.push(user_text(&format!("q{i} {bulk}")));
            messages.push(Message::new(
                Role::Assistant,
                vec![ContentBlock::Text {
                    text: format!("a{i} {bulk}"),
                }],
            ));
        }
        messages.push(user_text("final question"));
        let view = build_advisory_view(&messages);
        let trimmed = trim_view_to_window(view.clone(), Some(12_000));
        assert!(trimmed.len() < view.len(), "expected oldest frames dropped");
        assert_eq!(trimmed.first().unwrap().role, Role::User);
        // The instruction-bearing tail frame always survives.
        let last = trimmed.last().unwrap();
        assert!(text_of(last).contains("final question"));
        assert!(text_of(last).contains("Give your most"));
    }

    #[test]
    fn trim_keeps_everything_when_window_unknown_but_small_history() {
        let view = build_advisory_view(&[user_text("short")]);
        let trimmed = trim_view_to_window(view.clone(), None);
        assert_eq!(trimmed.len(), view.len());
    }
}

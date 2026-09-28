use nomi_types::message::{ContentBlock, Message};
use nomi_types::tool::ToolDef;

const CHARS_PER_TOKEN_TEXT: usize = 4;

const CHARS_PER_TOKEN_JSON: usize = 3;

/// Flat per-image token estimate. A 1568px-edge screenshot costs roughly
/// 1100-1600 tokens on Anthropic's vision pricing; over-estimating keeps
/// compaction triggering early rather than late.
const TOKENS_PER_IMAGE: usize = 1600;

/// Counts the bytes `serde` writes, discarding the document itself.
///
/// `serde_json::to_string(value).len()` allocates the entire serialized document
/// only to read its length, and this estimator runs over every tool input and
/// every advertised tool schema on every request. Counting during serialization
/// yields the same number with no allocation.
struct JsonByteCounter(usize);

impl std::io::Write for JsonByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Byte length of `value` exactly as [`serde_json::to_string`] would render it.
///
/// A write into [`JsonByteCounter`] cannot fail, and a `serde_json::Value` is by
/// construction serializable, so a serialization error can only mean the writer
/// errored.
fn json_byte_len(value: &serde_json::Value) -> usize {
    let mut counter = JsonByteCounter(0);
    let _ = serde_json::to_writer(&mut counter, value);
    counter.0
}

/// Estimate tokens for plain text using the same ratio as message estimation.
pub fn estimate_tokens_from_text(text: &str) -> u64 {
    (text.len() / CHARS_PER_TOKEN_TEXT) as u64
}

/// Estimate tokens for JSON / schema payloads.
pub fn estimate_tokens_from_json_text(text: &str) -> u64 {
    (text.len() / CHARS_PER_TOKEN_JSON) as u64
}

/// Estimate tokens for one tool definition as sent to the provider.
///
/// Deferred tools only expose a stub description and empty parameters, matching
/// the OpenAI adapter's request shaping.
pub fn estimate_tokens_from_tool_def(tool: &ToolDef) -> u64 {
    // Deferred tools advertise a stub schema, so they contribute no schema tokens.
    // The schema is counted rather than rendered: materializing it to read its
    // length allocated the whole document on every request.
    let schema_tokens = if tool.deferred {
        0
    } else {
        (json_byte_len(&tool.input_schema) / CHARS_PER_TOKEN_JSON) as u64
    };
    estimate_tokens_from_text(&tool.name)
        .saturating_add(estimate_tokens_from_text(&tool.description))
        .saturating_add(schema_tokens)
}

/// Estimate the total token count for a slice of messages.
///
/// Intentionally conservative (slightly over-estimates) to ensure
/// compaction triggers rather than being skipped.
pub fn estimate_tokens_from_messages(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_tokens_from_message).sum()
}

/// Estimate the tokens a provider request will actually consume: system
/// prompt, tool definitions, transcript, and optional turn-tail extras.
///
/// Message-only estimates miss static overhead and can skip compaction
/// until the real request overflows.
pub fn estimate_tokens_from_request(
    system_prompt: &str,
    tools: &[ToolDef],
    messages: &[Message],
    turn_tail: Option<&str>,
) -> u64 {
    let mut total = estimate_tokens_from_text(system_prompt);
    for tool in tools {
        total = total.saturating_add(estimate_tokens_from_tool_def(tool));
    }
    total = total.saturating_add(estimate_tokens_from_messages(messages));
    if let Some(tail) = turn_tail.filter(|text| !text.is_empty()) {
        total = total.saturating_add(estimate_tokens_from_text(tail));
    }
    total
}

/// Estimate tokens for a single message.
pub fn estimate_tokens_from_message(message: &Message) -> u64 {
    let mut total_chars: usize = 0;
    let mut json_chars: usize = 0;
    let mut image_tokens: usize = 0;

    for block in &message.content {
        match block {
            ContentBlock::Text { text } => {
                total_chars += text.len();
            }
            ContentBlock::Thinking { thinking, .. } => {
                total_chars += thinking.len();
            }
            ContentBlock::ToolUse { name, input, .. } => {
                json_chars += name.len() + json_byte_len(input);
            }
            ContentBlock::ToolResult { content, images, .. } => {
                total_chars += content.len();
                image_tokens += images.len() * TOKENS_PER_IMAGE;
            }
            ContentBlock::Image { .. } => {
                image_tokens += TOKENS_PER_IMAGE;
            }
        }
    }

    let text_tokens = total_chars / CHARS_PER_TOKEN_TEXT;
    let json_tokens = json_chars / CHARS_PER_TOKEN_JSON;

    (text_tokens + json_tokens + image_tokens) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_types::message::{Message, Role};
    use serde_json::json;

    #[test]
    fn empty_messages_returns_zero() {
        assert_eq!(estimate_tokens_from_messages(&[]), 0);
    }

    #[test]
    fn text_only_message() {
        let text = "a".repeat(400);
        let msg = Message::new(Role::User, vec![ContentBlock::Text { text }]);
        assert_eq!(estimate_tokens_from_messages(&[msg]), 100);
    }

    #[test]
    fn tool_use_message_uses_json_ratio() {
        let input = json!({"cmd": "ls -la"});
        let input_len = "Bash".len() + input.to_string().len();
        let msg = Message::new(
            Role::Assistant,
            vec![ContentBlock::ToolUse {
                id: "call_1".into(),
                name: "Bash".into(),
                input,
                extra: None,
            }],
        );
        let result = estimate_tokens_from_messages(&[msg]);
        assert_eq!(result, (input_len / CHARS_PER_TOKEN_JSON) as u64);
    }

    #[test]
    fn tool_result_uses_text_ratio() {
        let content = "x".repeat(800);
        let msg = Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content,
                is_error: false,
                images: Vec::new(),
            }],
        );
        assert_eq!(estimate_tokens_from_messages(&[msg]), 200);
    }

    #[test]
    fn mixed_conversation_accumulates() {
        let messages = vec![
            Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: "a".repeat(400),
                }],
            ),
            Message::new(
                Role::Assistant,
                vec![
                    ContentBlock::Text {
                        text: "b".repeat(200),
                    },
                    ContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "Read".into(),
                        input: json!({"path": "/foo/bar.rs"}),
                        extra: None,
                    },
                ],
            ),
            Message::new(
                Role::User,
                vec![ContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: "c".repeat(1200),
                    is_error: false,
                    images: Vec::new(),
                }],
            ),
        ];
        let estimate = estimate_tokens_from_messages(&messages);
        // text_tokens = (400 + 200 + 1200) / 4 = 450
        // json_tokens = ("Read".len() + json_string.len()) / 3
        assert!(estimate > 450);
        assert!(estimate < 600);
    }

    #[test]
    fn thinking_block_counted() {
        let thinking = "t".repeat(4000);
        let msg = Message::new(
            Role::Assistant,
            vec![ContentBlock::Thinking {
                thinking,
                signature: None,
            }],
        );
        assert_eq!(estimate_tokens_from_messages(&[msg]), 1000);
    }

    #[test]
    fn large_conversation_realistic_estimate() {
        let big_result = "x".repeat(400_000);
        let messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: big_result,
                is_error: false,
                images: Vec::new(),
            }],
        )];
        let estimate = estimate_tokens_from_messages(&messages);
        assert_eq!(estimate, 100_000);
    }

    #[test]
    fn effective_watermark_uses_max() {
        let provider_reported: u64 = 500;
        let messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: "x".repeat(400_000),
                is_error: false,
                images: Vec::new(),
            }],
        )];
        let local_estimate = estimate_tokens_from_messages(&messages);
        let effective = provider_reported.max(local_estimate);

        assert_eq!(effective, 100_000);
        assert!(effective > provider_reported);
    }

    #[test]
    fn request_estimate_includes_system_tools_and_tail() {
        let system = "s".repeat(400);
        let tools = vec![ToolDef {
            name: "Read".into(),
            description: "d".repeat(300),
            input_schema: json!({"type": "object"}),
            deferred: false,
        }];
        let messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "m".repeat(400),
            }],
        )];
        let tail = "t".repeat(400);
        let message_only = estimate_tokens_from_messages(&messages);
        let full = estimate_tokens_from_request(&system, &tools, &messages, Some(&tail));
        assert_eq!(message_only, 100);
        assert!(full > message_only + 200);
    }

    /// The counter must agree with the serializer it replaces, byte for byte,
    /// across every JSON shape the estimator can meet.
    #[test]
    fn json_byte_len_matches_the_serializer() {
        let values = vec![
            json!(null),
            json!(true),
            json!(0),
            json!(-12_345_678_901_234i64),
            json!(1.5),
            json!(""),
            json!("plain"),
            json!("quote\" backslash\\ newline\n tab\t"),
            json!("中文 emoji 🚀"),
            json!([]),
            json!([1, "two", null, [3, 4]]),
            json!({}),
            json!({"a": 1, "b": [true, false]}),
        ];
        for value in values {
            assert_eq!(
                json_byte_len(&value),
                serde_json::to_string(&value).unwrap().len(),
                "counter must match serde_json for {value}"
            );
        }
    }

    #[test]
    fn deferred_tool_def_counts_no_schema_tokens() {
        let tool = |deferred: bool| ToolDef {
            name: "Read".into(),
            description: "d".repeat(300),
            input_schema: json!({"type": "object", "properties": {"p": {"type": "string"}}}),
            deferred,
        };
        let description = "d".repeat(300);
        let full = estimate_tokens_from_tool_def(&tool(false));
        let stub = estimate_tokens_from_tool_def(&tool(true));

        assert_eq!(
            stub,
            estimate_tokens_from_text("Read") + estimate_tokens_from_text(&description),
            "a deferred tool must count only its name and description"
        );
        assert!(stub < full, "a stub schema must not carry schema tokens");
    }
}

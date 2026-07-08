//! `ContextContributor` — the host-agnostic seam (design §3.5) that lets the
//! backend inject dynamic, per-turn context into the **turn tail** (the last
//! user message) rather than the system prompt. This keeps the system prompt
//! byte-stable across turns so DeepSeek's automatic prefix cache stays warm.
//!
//! The engine holds a list of contributors (empty by default → behaviour
//! byte-for-byte unchanged) and, at the start of each turn, collects their
//! contributions and injects them into the messages array (turn tail) instead
//! of the system prompt.
//!
//! This is the foundation for turning "passive" platform features into "active"
//! injection (knowledge auto-RAG, inline memory, etc.) as registered
//! contributors rather than bespoke call-sites. It is purely additive: with no
//! contributors registered, the messages are returned unchanged.

use async_trait::async_trait;

/// A source of dynamic per-turn context. Implementations live in the backend
/// (host) and are registered onto the engine; the engine stays host-agnostic.
#[async_trait]
pub trait ContextContributor: Send + Sync {
    /// Context to add to the system prompt for the upcoming turn, or `None` to
    /// contribute nothing this turn. Called once per turn before the model call.
    async fn pre_turn_context(&self) -> Option<String>;

    /// A short stable label for diagnostics/telemetry.
    fn label(&self) -> &str {
        "context_contributor"
    }
}

/// Join non-empty contributions into a single string, each under a blank
/// line, in registration order. Empty / all-`None` → `None` returned
/// (the zero-contributor fast path the engine relies on). Pure so the
/// merge rule is unit-testable without an engine.
pub fn build_turn_tail_context(contributions: Vec<String>) -> Option<String> {
    let mut out = String::new();
    for c in contributions {
        let trimmed = c.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(trimmed);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Prepend `turn_tail_context` to the last user message in `messages`.
///
/// If the last message is a `Role::User` with at least one `Text` block, the
/// context is prepended as a new `Text` block at position 0. If the last
/// message is not a user message (e.g. a tool result), a new `Role::User`
/// message with the context is appended to the end of the messages.
///
/// This preserves the cache-stable system prompt prefix: only the last
/// message changes, all previous messages (and the system prompt) stay
/// byte-stable for DeepSeek prefix caching.
///
/// If `turn_tail_context` is `None` or empty, `messages` is returned unchanged.
pub fn inject_turn_tail_context(
    mut messages: Vec<nomi_types::message::Message>,
    turn_tail_context: Option<String>,
) -> Vec<nomi_types::message::Message> {
    use nomi_types::message::{ContentBlock, Message, Role};

    let Some(ctx) = turn_tail_context else {
        return messages;
    };
    let ctx = ctx.trim();
    if ctx.is_empty() {
        return messages;
    }

    let text_block = ContentBlock::Text {
        text: format!("[Context]\n{ctx}"),
    };

    // Try to prepend to the last user message
    if let Some(last) = messages.last_mut() {
        if last.role == Role::User {
            // Check if it has any Text blocks (vs pure ToolResult)
            let has_text = last
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::Text { .. }));
            if has_text {
                last.content.insert(0, text_block);
                return messages;
            }
        }
    }

    // Last message is not a user text message — append a new user message
    messages.push(Message::new(Role::User, vec![text_block]));
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_contributions_returns_none() {
        assert_eq!(build_turn_tail_context(vec![]), None);
        // All-empty contributions are also a no-op.
        assert_eq!(
            build_turn_tail_context(vec!["".into(), "   ".into()]),
            None
        );
    }

    #[test]
    fn joins_non_empty_contributions_in_order() {
        let out = build_turn_tail_context(
            vec!["[KB] hit".into(), "".into(), "[memory] fact".into()],
        );
        assert_eq!(out.as_deref(), Some("[KB] hit\n\n[memory] fact"));
    }

    #[test]
    fn single_contribution_no_leading_blank() {
        let out = build_turn_tail_context(vec!["only".into()]);
        assert_eq!(out.as_deref(), Some("only"));
    }

    #[test]
    fn inject_none_returns_messages_unchanged() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hello".into() }],
        )];
        let out = inject_turn_tail_context(msgs.clone(), None);
        assert_eq!(out.len(), msgs.len());
    }

    #[test]
    fn inject_prepends_to_last_user_text_message() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hello".into() }],
        )];
        let out = inject_turn_tail_context(msgs, Some("[RAG] fact".into()));
        assert_eq!(out.len(), 1); // still one message
        // First content block should be the injected context
        match &out[0].content[0] {
            ContentBlock::Text { text } => assert!(text.contains("[RAG] fact")),
            _ => panic!("expected Text block"),
        }
        // Second content block should be the original text
        match &out[0].content[1] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn inject_appends_new_message_when_last_is_tool_result() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "id1".into(),
                content: "result".into(),
                is_error: false,
                images: Vec::new(),
            }],
        )];
        let out = inject_turn_tail_context(msgs, Some("[RAG] fact".into()));
        assert_eq!(out.len(), 2); // original + new context message
        assert_eq!(out[1].role, Role::User);
    }

    #[tokio::test]
    async fn trait_object_contributes_through_build() {
        struct Fixed(&'static str);
        #[async_trait]
        impl ContextContributor for Fixed {
            async fn pre_turn_context(&self) -> Option<String> {
                Some(self.0.to_string())
            }
        }
        let contributors: Vec<Box<dyn ContextContributor>> =
            vec![Box::new(Fixed("alpha")), Box::new(Fixed("beta"))];
        let mut contributions = Vec::new();
        for c in &contributors {
            if let Some(s) = c.pre_turn_context().await {
                contributions.push(s);
            }
        }
        let out = build_turn_tail_context(contributions);
        assert_eq!(out.as_deref(), Some("alpha\n\nbeta"));
    }

    // --- Turn tail injection regression tests (cache-stability invariants) ---

    #[test]
    fn inject_does_not_modify_earlier_messages() {
        // Only the last user message may change — earlier messages must stay
        // byte-stable for DeepSeek prefix caching.
        use nomi_types::message::{ContentBlock, Message, Role};
        let msgs = vec![
            Message::new(
                Role::User,
                vec![ContentBlock::Text { text: "first".into() }],
            ),
            Message::new(
                Role::Assistant,
                vec![ContentBlock::Text { text: "reply".into() }],
            ),
            Message::new(
                Role::User,
                vec![ContentBlock::Text { text: "second".into() }],
            ),
        ];
        let out = inject_turn_tail_context(msgs, Some("[RAG] fact".into()));
        assert_eq!(out.len(), 3, "message count should not change");
        // First message unchanged
        match &out[0].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "first"),
            _ => panic!("first message should be unchanged"),
        }
        // Second message unchanged
        match &out[1].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "reply"),
            _ => panic!("second message should be unchanged"),
        }
        // Third message: context prepended, original text preserved
        assert_eq!(
            out[2].content.len(),
            2,
            "last message should have 2 blocks: context + original"
        );
    }

    #[test]
    fn inject_wraps_context_with_label() {
        // The injected context should be wrapped in [Context] so the model
        // can distinguish it from the user's actual message.
        use nomi_types::message::{ContentBlock, Message, Role};
        let msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hello".into() }],
        )];
        let out = inject_turn_tail_context(msgs, Some("date: 2025-01-01".into()));
        match &out[0].content[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("[Context]"), "should have [Context] label");
                assert!(text.contains("date: 2025-01-01"));
            }
            _ => panic!("expected Text block"),
        }
    }
}

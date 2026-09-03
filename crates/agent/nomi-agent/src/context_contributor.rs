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
//! Injected tail context is **persisted** onto the live transcript, matching
//! DeepSeek-Reasonix `control.Compose`: the next provider request must replay
//! the previous request's messages as a byte-identical prefix and only append.
//! Cloning the tail onto the wire without writing it back makes the next
//! request's first user message diverge, so cache hits collapse to system+tools.
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

    /// Independent contributors may be awaited together. Default true.
    fn parallel_safe(&self) -> bool {
        true
    }

    /// Optional token budget for this contribution. `None` means unbounded.
    fn max_tokens(&self) -> Option<usize> {
        None
    }
}

/// Append non-empty contributor contributions to `system`, each under a blank
/// line, in registration order. Empty / all-empty → `system` returned
/// unchanged. NOT used by the engine's turn path — dynamic content must ride
/// the turn tail via [`build_turn_tail_context`] to keep the system prompt
/// byte-stable for prefix caching; kept as a helper for static augmentation.
pub fn merge_pre_turn_context(system: String, contributions: Vec<String>) -> String {
    let mut out = system;
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
    out
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

/// Marker prepended to persisted turn-tail context. Reserved: the engine
/// injects it, traces/UI strip it, and prefix-cache compares treat it as
/// controller metadata rather than user text.
pub const TURN_TAIL_CONTEXT_PREFIX: &str = "[Context]\n";

pub fn is_turn_tail_context_text(text: &str) -> bool {
    text.starts_with(TURN_TAIL_CONTEXT_PREFIX)
}

/// Drop a leading `[Context]` block, if any. Used when comparing the live
/// tail to the original user requirement so a persisted compose does not
/// look like a different message.
pub fn without_leading_turn_tail(
    content: &[nomi_types::message::ContentBlock],
) -> Vec<nomi_types::message::ContentBlock> {
    match content.first() {
        Some(nomi_types::message::ContentBlock::Text { text })
            if is_turn_tail_context_text(text) =>
        {
            content[1..].to_vec()
        }
        _ => content.to_vec(),
    }
}

/// True when a user message exists only to carry persisted turn-tail extras.
/// Truncation restarts drop these so a later `[Context]` append does not hide
/// the original requirement and trigger a duplicate re-push.
pub fn is_context_only_user_content(
    content: &[nomi_types::message::ContentBlock],
) -> bool {
    matches!(
        content.first(),
        Some(nomi_types::message::ContentBlock::Text { text })
            if is_turn_tail_context_text(text)
    ) && without_leading_turn_tail(content)
        .iter()
        .all(|block| match block {
            nomi_types::message::ContentBlock::Text { text } => text.trim().is_empty(),
            _ => false,
        })
}

fn turn_tail_text_block(ctx: &str) -> nomi_types::message::ContentBlock {
    nomi_types::message::ContentBlock::Text {
        text: format!("{TURN_TAIL_CONTEXT_PREFIX}{ctx}"),
    }
}

/// Persist `turn_tail_context` onto the live transcript.
///
/// `frozen_len` is the count of messages already included in a prior
/// provider request (`0..frozen_len` must stay byte-identical).
///
/// Unsent last user (`index >= frozen_len`): prepend or replace a single
/// `[Context]` block. Identical text is a no-op.
///
/// Already-sent last user (`index < frozen_len`): never mutate. If extras
/// differ, append a new `Role::User` `[Context]` message so the sent prefix
/// stays cache-stable (midnight date, plan mode, resource notices).
///
/// If the last message is not a user message, a new `Role::User` message
/// with the context is appended.
///
/// If `turn_tail_context` is `None` or empty, `messages` is left unchanged.
pub fn persist_turn_tail_context(
    messages: &mut Vec<nomi_types::message::Message>,
    turn_tail_context: Option<String>,
    frozen_len: usize,
) {
    use nomi_types::message::{ContentBlock, Message, Role};

    let Some(ctx) = turn_tail_context else {
        return;
    };
    let ctx = ctx.trim();
    if ctx.is_empty() {
        return;
    }

    let text_block = turn_tail_text_block(ctx);
    let frozen_len = frozen_len.min(messages.len());

    if messages.last().is_some_and(|last| last.role == Role::User) {
        let last_idx = messages.len() - 1;
        if let Some(ContentBlock::Text { text: existing }) = messages[last_idx].content.first()
            && existing == text_of(&text_block)
        {
            return;
        }
        if last_idx < frozen_len {
            messages.push(Message::new(Role::User, vec![text_block]));
            return;
        }
        let last = &mut messages[last_idx];
        if let Some(ContentBlock::Text { text: existing }) = last.content.first()
            && is_turn_tail_context_text(existing)
        {
            last.content[0] = text_block;
            return;
        }
        last.content.insert(0, text_block);
        return;
    }

    messages.push(Message::new(Role::User, vec![text_block]));
}

fn text_of(block: &nomi_types::message::ContentBlock) -> &str {
    match block {
        nomi_types::message::ContentBlock::Text { text } => text,
        _ => "",
    }
}

/// Prepend `turn_tail_context` to the last user message in `messages`.
///
/// Owned-vec wrapper around [`persist_turn_tail_context`] for tests and
/// callers that already hold a clone.
pub fn inject_turn_tail_context(
    mut messages: Vec<nomi_types::message::Message>,
    turn_tail_context: Option<String>,
) -> Vec<nomi_types::message::Message> {
    persist_turn_tail_context(&mut messages, turn_tail_context, 0);
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
        assert_eq!(out.len(), 1, "tool-result user message should stay a single message");
        assert_eq!(out[0].content.len(), 2);
        match &out[0].content[0] {
            ContentBlock::Text { text } => assert!(text.starts_with("[Context]")),
            _ => panic!("context should be prepended as Text"),
        }
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

    #[test]
    fn persist_is_idempotent_on_the_same_last_user() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let mut msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hello".into() }],
        )];
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-01".into()), 0);
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-01".into()), 0);
        assert_eq!(msgs[0].content.len(), 2);
        match &msgs[0].content[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text.matches("[Context]").count(), 1);
            }
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn persist_replaces_context_on_the_same_last_user() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let mut msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hello".into() }],
        )];
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-01".into()), 0);
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-02".into()), 0);
        assert_eq!(msgs[0].content.len(), 2);
        match &msgs[0].content[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("date: 2025-01-02"));
                assert!(!text.contains("date: 2025-01-01"));
            }
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn persist_composes_a_new_last_user_without_touching_earlier_context() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let mut msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "first".into() }],
        )];
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-01".into()), 0);
        msgs.push(Message::new(
            Role::Assistant,
            vec![ContentBlock::Text { text: "ok".into() }],
        ));
        msgs.push(Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "id1".into(),
                content: "result".into(),
                is_error: false,
                images: Vec::new(),
            }],
        ));
        persist_turn_tail_context(&mut msgs, Some("plan mode".into()), 0);
        match &msgs[0].content[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("date: 2025-01-01"));
                assert!(!text.contains("plan mode"));
            }
            _ => panic!("first user must stay frozen"),
        }
        assert_eq!(msgs[2].content.len(), 2);
        match &msgs[2].content[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("plan mode"));
                assert!(!text.contains("date: 2025-01-01"));
            }
            _ => panic!("expected persisted context on the new last user"),
        }
    }

    #[test]
    fn persist_appends_when_last_user_already_sent() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let mut msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hello".into() }],
        )];
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-01".into()), 0);
        let frozen = msgs.len();
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-02".into()), frozen);
        assert_eq!(msgs.len(), 2);
        match &msgs[0].content[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("date: 2025-01-01"));
                assert!(!text.contains("date: 2025-01-02"));
            }
            _ => panic!("sent last user must stay frozen"),
        }
        match &msgs[1].content[0] {
            ContentBlock::Text { text } => {
                assert!(is_turn_tail_context_text(text));
                assert!(text.contains("date: 2025-01-02"));
            }
            _ => panic!("expected appended [Context] user"),
        }
    }

    #[test]
    fn persist_is_noop_when_frozen_context_matches() {
        use nomi_types::message::{ContentBlock, Message, Role};
        let mut msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hello".into() }],
        )];
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-01".into()), 0);
        let frozen = msgs.len();
        persist_turn_tail_context(&mut msgs, Some("date: 2025-01-01".into()), frozen);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content.len(), 2);
    }

    #[test]
    fn without_leading_turn_tail_restores_original_requirement() {
        use nomi_types::message::ContentBlock;
        let content = vec![
            ContentBlock::Text {
                text: "[Context]\nCurrent date: 2025-01-01".into(),
            },
            ContentBlock::Text {
                text: "hello".into(),
            },
        ];
        let stripped = without_leading_turn_tail(&content);
        assert_eq!(stripped.len(), 1);
        match &stripped[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected original user text"),
        }
    }

    #[test]
    fn context_only_user_is_an_appended_turn_tail() {
        use nomi_types::message::ContentBlock;
        let extra = vec![ContentBlock::Text {
            text: "[Context]\nCurrent date: 2025-01-01\n\n[resumable round 2/3]".into(),
        }];
        assert!(is_context_only_user_content(&extra));
        let requirement = vec![
            ContentBlock::Text {
                text: "[Context]\nCurrent date: 2025-01-01".into(),
            },
            ContentBlock::Text {
                text: "write a.html".into(),
            },
        ];
        assert!(!is_context_only_user_content(&requirement));
        assert!(!is_context_only_user_content(&[ContentBlock::Text {
            text: "write a.html".into(),
        }]));
    }
}

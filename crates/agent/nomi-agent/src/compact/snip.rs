//! Cheap compaction: drop old plain user/assistant text without splitting
//! tool_use / tool_result pairs.
//!
//! Compaction artifacts and short user constraints stay. The summarizer
//! folds a prior briefing, and a short user turn is kept verbatim beside it.
//! Long assistant monologues and oversized user pastes are still removed.

use nomi_config::compact::CompactConfig;
use nomi_types::message::{ContentBlock, Message, Role};

use super::auto;

/// Keep this many messages at the tail untouched (plus the first user message).
pub const DEFAULT_SNIP_KEEP_TAIL: usize = 24;

fn is_plain_text_turn(message: &Message) -> bool {
    let mut saw_text = false;
    for block in &message.content {
        match block {
            ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => return false,
            ContentBlock::Text { .. } | ContentBlock::Thinking { .. } => saw_text = true,
            ContentBlock::Image { .. } => {}
        }
    }
    saw_text
}

/// Indices of old plain user/assistant turns that sit before the tail window.
/// Never includes the first message, any tool_use/tool_result carrier, a prior
/// compaction artifact, or a short text-only user turn.
pub fn snip_indices(
    messages: &[Message],
    keep_tail: usize,
    config: &CompactConfig,
) -> Vec<usize> {
    if messages.len() <= keep_tail.saturating_add(1) {
        return Vec::new();
    }
    let keep_tail = keep_tail.max(1);
    let tail_start = messages.len().saturating_sub(keep_tail);
    let mut drop_idx: Vec<usize> = Vec::new();
    for (i, message) in messages.iter().enumerate() {
        if i == 0 || i >= tail_start {
            continue;
        }
        if auto::is_compaction_artifact(message) || auto::is_pinnable_user_turn(message, config) {
            continue;
        }
        if matches!(message.role, Role::User | Role::Assistant) && is_plain_text_turn(message) {
            drop_idx.push(i);
        }
    }
    drop_idx
}

/// Remove old plain user/assistant turns that sit before the tail window.
/// Never drops the first message, any tool_use/tool_result carrier, a prior
/// compaction artifact, or a short text-only user turn.
///
/// Returns the number of messages removed.
pub fn snip_old_plain_turns(
    messages: &mut Vec<Message>,
    keep_tail: usize,
    config: &CompactConfig,
) -> usize {
    let drop_idx = snip_indices(messages, keep_tail, config);
    let removed = drop_idx.len();
    for i in drop_idx.into_iter().rev() {
        messages.remove(i);
    }
    removed
}

/// One-line handoff inserted after snip when older turns were dropped.
pub fn collapse_notice(removed: usize, archive_rel: Option<&str>) -> String {
    let mut text = format!(
        "[Context collapse] Dropped {removed} older plain turns. \
         Keep Goal / Files / Decisions / Errors / Next from the remaining transcript \
         and WorkingSet. Do not restart a workspace tour."
    );
    if let Some(path) = archive_rel.filter(|p| !p.is_empty()) {
        text.push(' ');
        text.push_str(&crate::compact::archive::archive_notice(path));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_types::message::ContentBlock;

    fn text(role: Role, body: &str) -> Message {
        Message::now(role, vec![ContentBlock::Text { text: body.into() }])
    }

    fn tool_use() -> Message {
        Message::now(
            Role::Assistant,
            vec![ContentBlock::ToolUse {
                id: "1".into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": "a.rs"}),
                extra: None,
            }],
        )
    }

    fn tool_result() -> Message {
        Message::now(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "1".into(),
                content: "ok".into(),
                is_error: false,
                images: Vec::new(),
            }],
        )
    }

    #[test]
    fn keeps_tool_pairs_and_first_user() {
        let mut messages = vec![
            text(Role::User, "task"),
            text(Role::Assistant, "old thought"),
            text(Role::User, "old followup"),
            tool_use(),
            tool_result(),
            text(Role::Assistant, "recent"),
        ];
        let removed = snip_old_plain_turns(&mut messages, 2, &CompactConfig::default());
        assert!(removed >= 1);
        assert_eq!(messages[0].role, Role::User);
        assert!(messages.iter().any(|m| message_text(m) == "old followup"));
        assert!(messages.iter().all(|m| message_text(m) != "old thought"));
        assert!(messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
        }));
        assert!(messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
        }));
    }

    fn message_text(message: &Message) -> String {
        message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn keeps_compaction_artifacts_and_short_user_constraints() {
        let config = CompactConfig::default();
        let mut messages = vec![text(Role::User, "task")];
        messages.push(text(
            Role::User,
            "[Conversation compacted]\n{\"trigger\":\"auto\"}",
        ));
        messages.push(text(
            Role::User,
            "This session is being continued from a previous conversation. PRIOR_BRIEFING",
        ));
        messages.push(text(Role::User, "不要改支付接口"));
        messages.push(text(Role::Assistant, "old monologue that should leave"));
        messages.push(text(Role::User, &"x".repeat(8_000)));
        for i in 0..DEFAULT_SNIP_KEEP_TAIL {
            messages.push(text(Role::Assistant, &format!("tail-{i}")));
        }

        let removed = snip_old_plain_turns(&mut messages, DEFAULT_SNIP_KEEP_TAIL, &config);
        assert!(removed >= 2, "long monologue and paste should still drop");
        let kept: Vec<String> = messages.iter().map(message_text).collect();
        assert!(kept.iter().any(|text| text.contains("[Conversation compacted]")));
        assert!(kept.iter().any(|text| text.contains("PRIOR_BRIEFING")));
        assert!(kept.iter().any(|text| text == "不要改支付接口"));
        assert!(kept.iter().all(|text| !text.contains("old monologue")));
        assert!(kept.iter().all(|text| text.len() < 8_000));
    }
}

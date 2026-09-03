//! Cheap compaction: drop old plain user/assistant text without splitting
//! tool_use / tool_result pairs.

use nomi_types::message::{ContentBlock, Message, Role};

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

/// Remove old plain user/assistant turns that sit before the tail window.
/// Never drops the first message or any tool_use/tool_result carrier.
///
/// Returns the number of messages removed.
pub fn snip_old_plain_turns(messages: &mut Vec<Message>, keep_tail: usize) -> usize {
    if messages.len() <= keep_tail.saturating_add(1) {
        return 0;
    }
    let keep_tail = keep_tail.max(1);
    let tail_start = messages.len().saturating_sub(keep_tail);
    let mut drop_idx: Vec<usize> = Vec::new();
    for (i, message) in messages.iter().enumerate() {
        if i == 0 || i >= tail_start {
            continue;
        }
        if matches!(message.role, Role::User | Role::Assistant) && is_plain_text_turn(message) {
            drop_idx.push(i);
        }
    }
    let removed = drop_idx.len();
    for i in drop_idx.into_iter().rev() {
        messages.remove(i);
    }
    removed
}

/// One-line handoff inserted after snip when older turns were dropped.
pub fn collapse_notice(removed: usize) -> String {
    format!(
        "[Context collapse] Dropped {removed} older plain turns. \
         Keep Goal / Files / Decisions / Errors / Next from the remaining transcript \
         and WorkingSet. Do not restart a workspace tour."
    )
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
        let removed = snip_old_plain_turns(&mut messages, 2);
        assert!(removed >= 1);
        assert_eq!(messages[0].role, Role::User);
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
}

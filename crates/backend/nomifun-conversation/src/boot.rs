//! Conversation boot reconciliation.
//!
//! When the process is killed mid-turn (e.g. while a model is streaming its
//! "thinking"), three persisted artifacts survive: the conversation row stays
//! `status = 'running'`, its `thinking` message keeps `content.status != "done"`,
//! and a resumable ACP session remains bound. On the next launch, opening the
//! conversation warms up a fresh runtime, the ACP agent resumes the old session
//! and *replays* the interrupted turn's stream events — the frontend lights up
//! `turnActivity.running` but never receives the matching `finish`, so the turn
//! disclosure spins forever.
//!
//! At boot no in-memory runtime is live, so any `status = 'running'` row is by
//! definition a ghost left behind by a killed process (mirrors the existing
//! `terminal_service` / `creation_service` boot reconciliation). We settle each
//! one so the reopened conversation is idle and usable again.

use std::sync::Arc;

use nomifun_db::models::MessageRow;
use nomifun_db::{
    ConversationRowUpdate, IAcpSessionRepository, IConversationRepository, MessageRowUpdate,
    SortOrder,
};
use tracing::warn;

/// Settles every conversation still persisted as `running` after an unclean
/// shutdown. For each ghost session we:
///
/// 1. Finalize any dangling `thinking` message whose `content.status != "done"`
///    so the frontend renders it as an ended thought rather than a live one.
/// 2. Clear the bound ACP session id, so the next message opens a fresh
///    `session/new` instead of resuming and replaying the interrupted turn.
/// 3. Mark the conversation `finished` (the terminal `ConversationStatus`).
///
/// A single failure is logged and skipped; it never aborts the sweep. Returns
/// the number of conversations that were settled.
pub async fn reconcile_running_conversations_on_boot(
    conversation_repo: &Arc<dyn IConversationRepository>,
    acp_session_repo: &Arc<dyn IAcpSessionRepository>,
) -> usize {
    let running = match conversation_repo.list_running().await {
        Ok(rows) => rows,
        Err(e) => {
            warn!(error = %e, "conversation boot reconciliation: failed to list running conversations");
            return 0;
        }
    };

    let mut reconciled = 0usize;
    for conv in running {
        let conv_id = conv.id.as_str();

        finalize_dangling_thinking(conversation_repo, conv_id).await;

        // Break resume/replay at the root: NULL the session id so warmup opens a
        // fresh session instead of resuming the interrupted turn. Same mechanism
        // as `clear_context` / `clear_messages`.
        if let Err(e) = acp_session_repo.clear_session_id(conv_id).await {
            warn!(conversation_id = conv_id, error = %e, "conversation boot reconciliation: failed to clear acp session id");
        }

        // Settle the conversation status to the terminal state. DB value matches
        // `enum_to_db(&ConversationStatus::Finished)`.
        if let Err(e) = conversation_repo
            .update(
                conv_id,
                &ConversationRowUpdate {
                    status: Some("finished".to_owned()),
                    ..Default::default()
                },
            )
            .await
        {
            warn!(conversation_id = conv_id, error = %e, "conversation boot reconciliation: failed to settle conversation status");
            continue;
        }

        reconciled += 1;
    }

    reconciled
}

/// Rewrites every `thinking` message of a conversation whose `content.status`
/// is not `"done"` to a terminal `"done"` status, preserving the accumulated
/// content. Best-effort per message: parse/update failures are logged, not
/// propagated.
async fn finalize_dangling_thinking(
    conversation_repo: &Arc<dyn IConversationRepository>,
    conv_id: &str,
) {
    // A single ghost turn only carries a handful of messages; a generous page
    // size fetches the whole transcript in one query (matches other callers).
    let messages = match conversation_repo
        .get_messages(conv_id, 1, 5000, SortOrder::Asc)
        .await
    {
        Ok(page) => page.items,
        Err(e) => {
            warn!(conversation_id = conv_id, error = %e, "conversation boot reconciliation: failed to load messages");
            return;
        }
    };

    for msg in messages {
        if msg.r#type != "thinking" {
            continue;
        }
        if let Some(update) = finalize_thinking_content(&msg) {
            if let Err(e) = conversation_repo.update_message(&msg.id, &update).await {
                warn!(conversation_id = conv_id, message_id = %msg.id, error = %e, "conversation boot reconciliation: failed to finalize thinking message");
            }
        }
    }
}

/// Returns the update needed to close a dangling `thinking` message, or `None`
/// when the message is already `done` or its content is unparseable.
fn finalize_thinking_content(msg: &MessageRow) -> Option<MessageRowUpdate> {
    let mut content: serde_json::Value = serde_json::from_str(&msg.content).ok()?;
    let obj = content.as_object_mut()?;

    let already_done = obj
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s == "done")
        .unwrap_or(false);
    if already_done {
        return None;
    }

    obj.insert(
        "status".to_owned(),
        serde_json::Value::String("done".to_owned()),
    );

    Some(MessageRowUpdate {
        content: Some(content.to_string()),
        status: Some(Some("finish".to_owned())),
        ..Default::default()
    })
}

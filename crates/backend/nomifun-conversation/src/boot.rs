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
//! At boot no in-memory runtime is live, so any `status = 'running'` row is a
//! candidate ghost left behind by a killed process (mirrors the existing
//! `terminal_service` / `creation_service` boot reconciliation). We settle each
//! one so the reopened conversation is idle and usable again.
//!
//! Crucially, the destructive step — dropping the resumable ACP session — is
//! gated on a *genuine* crash artifact (a dangling in-progress message). The
//! `running` marker is written on the hot send path and flipped back to
//! `finished` only on specific termination paths, so a turn that ends through a
//! path that misses the write-back leaves a perfectly healthy conversation
//! stuck at `running`. Clearing such a conversation's session would destroy
//! usable context on the next normal restart. We therefore only clear the
//! session when an interrupted turn actually left a dangling message behind.

use std::sync::Arc;

use nomifun_db::models::MessageRow;
use nomifun_db::{
    ConversationRowUpdate, IAcpSessionRepository, IConversationRepository, MessageRowUpdate,
    SortOrder,
};
use tracing::warn;

/// Settles every conversation still persisted as `running` after an unclean
/// shutdown. For each candidate we:
///
/// 1. Finalize any dangling `thinking` message whose `content.status != "done"`
///    and terminalize any message still marked `status = 'work'`, so the
///    frontend renders them as ended rather than live. This also tells us
///    whether the turn was *genuinely* interrupted.
/// 2. **Only when a dangling artifact was found**, clear the bound ACP session
///    id, so the next message opens a fresh `session/new` instead of resuming
///    and replaying the interrupted turn. A `running` row with no dangling
///    message is a healthy conversation whose status write leaked; its session
///    is left intact.
/// 3. Mark the conversation `finished` (the terminal `ConversationStatus`),
///    correcting the stale/leaked marker either way.
///
/// A single failure is logged and skipped; it never aborts the sweep. Returns
/// the number of genuinely-interrupted conversations that were settled.
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

        // Detect + close the actual crash artifacts. `had_dangling` is our
        // proof that a turn was interrupted mid-flight rather than the status
        // marker simply leaking on a healthy termination path.
        let had_dangling = settle_dangling_turn_messages(conversation_repo, conv_id).await;

        // Break resume/replay at the root only for genuinely interrupted turns:
        // NULL the session id so warmup opens a fresh session instead of
        // resuming the interrupted turn. Same mechanism as `clear_context` /
        // `clear_messages`. Healthy conversations keep their resumable session.
        if had_dangling {
            if let Err(e) = acp_session_repo.clear_session_id(conv_id).await {
                warn!(conversation_id = conv_id, error = %e, "conversation boot reconciliation: failed to clear acp session id");
            }
        }

        // Settle the conversation status to the terminal state regardless; this
        // is harmless and corrects a leaked marker without touching the session.
        // DB value matches `enum_to_db(&ConversationStatus::Finished)`.
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

        if had_dangling {
            reconciled += 1;
        }
    }

    reconciled
}

/// Closes the dangling artifacts of a single conversation's interrupted turn:
/// rewrites every `thinking` message whose `content.status` is not `"done"` to
/// a terminal `"done"` (preserving accumulated content), and terminalizes any
/// message still persisted as `status = 'work'`. Best-effort per message:
/// parse/update failures are logged, not propagated.
///
/// Returns `true` if at least one dangling artifact was found — the signal that
/// this conversation's turn was genuinely interrupted and its resumable ACP
/// session must be dropped.
async fn settle_dangling_turn_messages(
    conversation_repo: &Arc<dyn IConversationRepository>,
    conv_id: &str,
) -> bool {
    // A single ghost turn only carries a handful of messages; a generous page
    // size fetches the whole transcript in one query (matches other callers).
    let messages = match conversation_repo
        .get_messages(conv_id, 1, 5000, SortOrder::Asc)
        .await
    {
        Ok(page) => page.items,
        Err(e) => {
            warn!(conversation_id = conv_id, error = %e, "conversation boot reconciliation: failed to load messages");
            return false;
        }
    };

    let mut had_dangling = false;
    for msg in messages {
        // A dangling `thinking` message spins forever on replay; close it while
        // preserving its accumulated content.
        if msg.r#type == "thinking" {
            if let Some(update) = finalize_thinking_content(&msg) {
                had_dangling = true;
                if let Err(e) = conversation_repo.update_message(&msg.id, &update).await {
                    warn!(conversation_id = conv_id, message_id = %msg.id, error = %e, "conversation boot reconciliation: failed to finalize thinking message");
                }
            }
            continue;
        }

        // Any other message left in the in-progress `work` state is a stranded
        // stream from the interrupted turn; terminalize it so it stops
        // rendering as live.
        if msg.status.as_deref() == Some("work") {
            had_dangling = true;
            let update = MessageRowUpdate {
                status: Some(Some("finish".to_owned())),
                ..Default::default()
            };
            if let Err(e) = conversation_repo.update_message(&msg.id, &update).await {
                warn!(conversation_id = conv_id, message_id = %msg.id, error = %e, "conversation boot reconciliation: failed to terminalize work message");
            }
        }
    }

    had_dangling
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

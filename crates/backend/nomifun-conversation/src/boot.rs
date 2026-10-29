//! Transcript/ACP artifact preparation for proven restart orphans.
//!
//! When the process is killed mid-turn (e.g. while a model is streaming its
//! "thinking"), three persisted artifacts survive: the conversation row stays
//! `status = 'running'`, its `thinking` message keeps `content.status != "done"`,
//! and a resumable ACP session remains bound. On the next launch, opening the
//! conversation warms up a fresh runtime, the ACP agent resumes the old session
//! and *replays* the interrupted turn's stream events - the frontend lights up
//! `turnActivity.running` but never receives the matching `finish`, so the turn
//! disclosure spins forever.
//!
//! A persisted `running` row is **not** proof that its turn owner died: work can
//! be hosted by another process or machine. Settlement therefore happens only
//! through the proof-aware recovery seams in [`crate::service`] - after the
//! persisted exact terminal-proof protocol has matched the frozen old
//! generation.
//!
//! This module only *prepares* the transcript updates from state read while the
//! per-Conversation preparation gate is held. The lifecycle finalization and
//! these message/ACP writes are committed in one repository transaction, so a
//! cleanup failure rolls the lifecycle settlement back and the still-running
//! generation stays enumerable for the next boot.
//!
//! The destructive session step is gated on a *genuine* crash artifact (a
//! dangling in-progress message). The `running` marker is written on the hot
//! send path and flipped back to `finished` only on specific termination paths,
//! so a turn that ends through a path that misses the write-back leaves a
//! perfectly healthy conversation stuck at `running`. Clearing such a
//! conversation's session would destroy usable context on the next normal
//! restart. We therefore only clear the session when an interrupted turn
//! actually left a dangling message behind.

use std::sync::Arc;

use nomifun_common::AppError;
use nomifun_db::models::MessageRow;
use nomifun_db::{
    IConversationRepository, MessageRowUpdate, SortOrder,
};
use tracing::warn;

/// Transcript/ACP writes that belong to one proven interrupted generation and
/// must commit atomically with its lifecycle finalization.
pub(crate) struct ProvenTurnArtifactCleanup {
    pub message_updates: Vec<(String, MessageRowUpdate)>,
    pub clear_acp_session: bool,
}

/// Read the interrupted generation's transcript and compute the exact cleanup
/// writes for it. This performs no mutation.
///
/// A read failure is reported as a conflict instead of silently skipping the
/// cleanup: the caller then leaves the generation `running` so a later boot can
/// retry the whole transaction. Callers must hold the per-Conversation
/// preparation gate and have proven the exact old generation first.
pub(crate) async fn prepare_proven_interrupted_turn_artifacts(
    conversation_repo: &Arc<dyn IConversationRepository>,
    conversation_id: &str,
) -> Result<ProvenTurnArtifactCleanup, AppError> {
    // Walk every page instead of treating the first recovery page as the whole
    // transcript: an interrupted turn may be the tail of a long Conversation.
    // The preparation gate keeps this read stable; any page failure aborts the
    // settlement before a transaction starts.
    const RECOVERY_PAGE_SIZE: u32 = 5000;
    let mut message_updates = Vec::new();
    let mut page = 1u32;
    loop {
        let page_result = conversation_repo
            .get_messages(conversation_id, page, RECOVERY_PAGE_SIZE, SortOrder::Asc)
            .await
            .map_err(|error| {
                warn!(
                    conversation_id,
                    page,
                    error = %error,
                    "proven restart-orphan preparation: failed to load messages; deferring settlement"
                );
                AppError::Conflict(format!(
                    "restart-orphan transcript read failed; lifecycle settlement deferred: {error}"
                ))
            })?;
        let has_more = page_result.has_more;
        let fetched = page_result.items.len();
        if has_more && fetched == 0 {
            return Err(AppError::Conflict(
                "restart-orphan transcript pagination returned an empty non-final page; lifecycle settlement deferred"
                    .to_owned(),
            ));
        }

        for msg in page_result.items {
            // A dangling `thinking` message spins forever on replay; close it
            // while preserving its accumulated content.
            if msg.r#type == "thinking" {
                if let Some(update) = finalize_thinking_content(&msg) {
                    message_updates.push((msg.message_id.clone(), update));
                }
                continue;
            }

            // Any other message left in the in-progress `work` state is a
            // stranded stream from the interrupted turn; terminalize it so it
            // stops rendering as live.
            if msg.status.as_deref() == Some("work") {
                message_updates.push((
                    msg.message_id.clone(),
                    MessageRowUpdate {
                        status: Some(Some("finish".to_owned())),
                        ..Default::default()
                    },
                ));
            }
        }

        if !has_more {
            break;
        }
        page = page.checked_add(1).ok_or_else(|| {
            AppError::Conflict(
                "restart-orphan transcript pagination exceeded its page range; lifecycle settlement deferred"
                    .to_owned(),
            )
        })?;
    }

    Ok(ProvenTurnArtifactCleanup {
        clear_acp_session: !message_updates.is_empty(),
        message_updates,
    })
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

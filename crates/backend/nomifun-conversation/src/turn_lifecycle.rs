//! Explicit turn admission / cancel / finish authority (TurnLifecycle).
//!
//! Runtime fencing lives in [`crate::runtime_state`]. This module owns the
//! ConversationService orchestration that used to sit only in `service.rs`,
//! plus a readable state × command matrix for tests and reviewers.
//!
//! ```text
//! Phase \ Command | Admit | Cancel(User) | Cancel(Execution) | Finish
//! ----------------|-------|--------------|-------------------|-------
//! Idle            | ok    | no-op        | no-op             | reject
//! Running         | busy  | ok           | ok (attempt)      | -> Finishing
//! Finishing       | reject| fence owns   | fence owns        | idle+receipt
//! Cancelling      | reject| idempotent   | merge             | idle
//! ```

use std::sync::Arc;

use nomifun_ai_agent::AgentRuntimeRegistry;
use nomifun_api_types::{ConversationRuntimeSummary, WebSocketMessage};
use nomifun_common::{AppError, CompanionId, ErrorChain};
use tracing::warn;

use crate::runtime_state::{AgentTurnHandle, InMemoryCancelAuthority};
use crate::stream_relay::StreamRelay;

use super::{
    CANCEL_AUTH_PREFLIGHT_GRACE, CANCEL_HANDLER_GRACE, CANCEL_TEARDOWN_GRACE, ConversationService,
    parse_conv_id,
};

/// Who requested a turn stop. User stops are stamped for AutoWork; execution
/// infrastructure cleanup must not look like a direct user interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelOrigin {
    User,
    AgentExecution,
}

/// Coarse turn phase used by the matrix above. Not persisted — derived from
/// runtime_state admission + in-flight handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Matrix vocabulary for tests and future gate wiring.
pub(crate) enum TurnPhase {
    Idle,
    Running,
    Finishing,
    Cancelling,
}

/// Commands the turn gate accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TurnCommand {
    Admit,
    Cancel(CancelOrigin),
    Finish,
}

/// Pure gate: whether `command` is allowed in `phase` without side effects.
#[allow(dead_code)]
pub(crate) fn turn_command_allowed(phase: TurnPhase, command: TurnCommand) -> bool {
    match (phase, command) {
        (TurnPhase::Idle, TurnCommand::Admit) => true,
        (TurnPhase::Idle, TurnCommand::Cancel(_)) => true,
        (TurnPhase::Idle, TurnCommand::Finish) => false,
        (TurnPhase::Running, TurnCommand::Admit) => false,
        (TurnPhase::Running, TurnCommand::Cancel(_)) => true,
        (TurnPhase::Running, TurnCommand::Finish) => true,
        (TurnPhase::Finishing, TurnCommand::Admit) => false,
        (TurnPhase::Finishing, TurnCommand::Cancel(_)) => true,
        (TurnPhase::Finishing, TurnCommand::Finish) => true,
        (TurnPhase::Cancelling, TurnCommand::Admit) => false,
        (TurnPhase::Cancelling, TurnCommand::Cancel(_)) => true,
        (TurnPhase::Cancelling, TurnCommand::Finish) => true,
    }
}

impl ConversationService {
    pub async fn complete_turn_with_companion_context(
        &self,
        user_id: &str,
        conversation_id: &str,
        turn_id: &str,
        companion: bool,
        companion_id: Option<CompanionId>,
        origin: Option<String>,
        channel_platform: Option<String>,
    ) {
        let runtime = self.final_completion_runtime(conversation_id);
        StreamRelay::complete_conversation_with_context(
            &self.conversation_repo,
            &self.user_events,
            user_id,
            conversation_id,
            Some(turn_id.to_owned()),
            Some(runtime),
            companion,
            companion_id,
            origin,
            channel_platform,
        )
        .await;
    }

    pub(crate) fn final_completion_runtime(
        &self,
        conversation_id: &str,
    ) -> ConversationRuntimeSummary {
        let agent = self.runtime_registry.get_runtime(conversation_id);
        ConversationRuntimeSummary {
            state: nomifun_api_types::ConversationRuntimeStateKind::Idle,
            can_send_message: true,
            has_runtime: agent.is_some(),
            runtime_status: agent.as_ref().and_then(|agent| agent.status()),
            is_processing: false,
            pending_confirmations: 0,
            processing_started_at: None,
        }
    }

    pub(crate) async fn release_and_complete_turn(
        &self,
        turn_handle: &mut AgentTurnHandle,
        user_id: &str,
        conversation_id: &str,
        turn_id: &str,
        companion: bool,
        companion_id: Option<CompanionId>,
        origin: Option<String>,
        channel_platform: Option<String>,
    ) {
        let completion_fence = self
            .runtime_state
            .begin_turn_completion(conversation_id, turn_handle.turn_id());
        let completion_fence = match completion_fence {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => {
                let _ = turn_handle.release();
                return;
            }
            Err(error) => {
                warn!(
                    conversation_id,
                    error = %ErrorChain(&error),
                    "Failed to acquire completion admission fence"
                );
                None
            }
        };
        if !turn_handle.release() {
            return;
        }
        StreamRelay::persist_conversation_finished(&self.conversation_repo, conversation_id).await;
        let allowed_completion_owners = usize::from(completion_fence.is_some());
        let user_events = Arc::clone(&self.user_events);
        let runtime = self.final_completion_runtime(conversation_id);
        let completion_published = self
            .runtime_state
            .linearize_cleanup_event(
                conversation_id,
                0,
                allowed_completion_owners,
                CANCEL_TEARDOWN_GRACE,
                move || {
                    StreamRelay::broadcast_turn_completed_with_context(
                        &user_events,
                        user_id,
                        conversation_id,
                        Some(turn_id.to_owned()),
                        Some(runtime),
                        companion,
                        companion_id,
                        origin,
                        channel_platform,
                    );
                    drop(completion_fence);
                },
            )
            .await;
        if !completion_published {
            warn!(
                conversation_id,
                turn_id,
                "Completion event withheld because another cleanup fence remained active"
            );
        }
    }

    #[tracing::instrument(skip_all, fields(user_id = %user_id, conversation_id = %conversation_id))]
    pub async fn cancel(
        &self,
        user_id: &str,
        conversation_id: &str,
        runtime_registry: &Arc<dyn AgentRuntimeRegistry>,
    ) -> Result<(), AppError> {
        self.cancel_with_origin(
            user_id,
            conversation_id,
            runtime_registry,
            CancelOrigin::User,
        )
        .await
    }

    pub async fn cancel_for_execution(
        &self,
        user_id: &str,
        conversation_id: &str,
        runtime_registry: &Arc<dyn AgentRuntimeRegistry>,
    ) -> Result<(), AppError> {
        self.cancel_with_origin(
            user_id,
            conversation_id,
            runtime_registry,
            CancelOrigin::AgentExecution,
        )
        .await
    }

    pub(crate) async fn cancel_with_origin(
        &self,
        user_id: &str,
        conversation_id: &str,
        runtime_registry: &Arc<dyn AgentRuntimeRegistry>,
        origin: CancelOrigin,
    ) -> Result<(), AppError> {
        let conversation_key = parse_conv_id(conversation_id)?;
        let mut user_cancel_preflight = None;
        let in_memory_authority = if origin == CancelOrigin::User {
            let authorization = self
                .runtime_state
                .authorize_in_memory_user_cancel(conversation_id, user_id)?;
            user_cancel_preflight = authorization.preflight_guard;
            authorization.authority
        } else if self
            .runtime_state
            .active_turn_allows_cancel(conversation_id, user_id, false)
        {
            InMemoryCancelAuthority::ActiveTurn
        } else {
            InMemoryCancelAuthority::None
        };

        if let InMemoryCancelAuthority::PublicBuilds(cancelled_build_ids) = &in_memory_authority {
            self.note_user_cancel(conversation_id);
            self.runtime_state
                .forget_cancelled_runtime_builds(conversation_id, cancelled_build_ids);
            return Ok(());
        }

        if in_memory_authority != InMemoryCancelAuthority::ActiveTurn {
            let conversation = tokio::time::timeout(
                CANCEL_AUTH_PREFLIGHT_GRACE,
                self.conversation_repo.get(conversation_key),
            )
            .await
            .map_err(|_| {
                AppError::Timeout(
                    "conversation stop authorization exceeded its hard bound".to_owned(),
                )
            })??
            .filter(|row| row.user_id == user_id)
            .ok_or_else(|| AppError::NotFound(format!("Conversation {conversation_id} not found")))?;

            if origin == CancelOrigin::User {
                tokio::time::timeout(
                    CANCEL_AUTH_PREFLIGHT_GRACE,
                    self.ensure_not_retained_execution_attempt(user_id, &conversation.id),
                )
                .await
                .map_err(|_| {
                    AppError::Timeout(
                        "conversation stop retention check exceeded its hard bound".to_owned(),
                    )
                })??;
            }
        }

        if origin == CancelOrigin::User {
            self.note_user_cancel(conversation_id);
        }

        let result_rx = self.spawn_turn_stop_cleanup(
            user_id.to_owned(),
            conversation_id.to_owned(),
            Arc::clone(runtime_registry),
            true,
            false,
        );
        drop(user_cancel_preflight);
        let stop_result = match tokio::time::timeout(CANCEL_HANDLER_GRACE, result_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::Internal(
                "conversation stop worker exited before reporting completion".to_owned(),
            )),
            Err(_) => {
                warn!(
                    conversation_id,
                    "Conversation stop accepted; bounded cleanup continues in the background"
                );
                Ok(())
            }
        };
        stop_result?;
        Ok(())
    }

    pub(crate) fn note_user_cancel(&self, conversation_id: &str) {
        if let Ok(mut stamps) = self.user_cancel_stamps.lock() {
            stamps.insert(conversation_id.to_string(), nomifun_common::now_ms());
        }
    }

    pub(crate) async fn broadcast_turn_started_with_context(
        &self,
        user_id: &str,
        conversation_id: &str,
        turn_id: &str,
        companion: bool,
        companion_id: Option<CompanionId>,
        origin: Option<String>,
        channel_platform: Option<String>,
    ) {
        let runtime = self.runtime_summary_for(conversation_id).await;
        let payload = serde_json::json!({
            "conversation_id": conversation_id,
            "turn_id": turn_id,
            "status": "running",
            "phase": "starting",
            "state": "initializing",
            "can_send_message": runtime.can_send_message,
            "runtime": runtime,
            "companion": companion,
            "companion_id": companion_id,
            "origin": origin,
            "channel_platform": channel_platform,
        });
        self.user_events
            .send_to_user(user_id, WebSocketMessage::new("turn.started", payload));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_allows_admit_and_noop_cancel() {
        assert!(turn_command_allowed(TurnPhase::Idle, TurnCommand::Admit));
        assert!(turn_command_allowed(
            TurnPhase::Idle,
            TurnCommand::Cancel(CancelOrigin::User)
        ));
        assert!(!turn_command_allowed(TurnPhase::Idle, TurnCommand::Finish));
    }

    #[test]
    fn running_rejects_admit_allows_cancel_and_finish() {
        assert!(!turn_command_allowed(TurnPhase::Running, TurnCommand::Admit));
        assert!(turn_command_allowed(
            TurnPhase::Running,
            TurnCommand::Cancel(CancelOrigin::AgentExecution)
        ));
        assert!(turn_command_allowed(TurnPhase::Running, TurnCommand::Finish));
    }
}

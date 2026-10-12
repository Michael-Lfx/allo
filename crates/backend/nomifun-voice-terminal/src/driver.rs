//! Voice Session Driver and Router abstractions.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.5, §3.6.

use async_trait::async_trait;
use crate::contract::{
    AcousticEvent, AcousticFsm, AcousticState, IllegalTransitionError, VoiceDecision,
};

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("Routing generation failed: {0}")]
    Generation(String),
    #[error("Validation failed: {0:?}")]
    Validation(Vec<String>),
    #[error("Network request failed: {0}")]
    Network(String),
}

#[async_trait]
pub trait VoiceRouter: Send + Sync {
    async fn route(&self, input: &str) -> Result<VoiceDecision, RouteError> {
        self.route_with_status(input, None).await
    }

    async fn route_with_status(
        &self,
        input: &str,
        task_status: Option<&str>,
    ) -> Result<VoiceDecision, RouteError>;
}

/// Controllable fixture router for tests.
#[cfg(any(test, feature = "voice-skeleton"))]
#[derive(Debug, Clone)]
pub struct StubVoiceRouter {
    decision: VoiceDecision,
}

#[cfg(any(test, feature = "voice-skeleton"))]
impl StubVoiceRouter {
    pub fn new(decision: VoiceDecision) -> Self {
        Self { decision }
    }

    pub fn chat(reply: impl Into<String>) -> Self {
        Self {
            decision: VoiceDecision {
                intent_type: crate::contract::IntentType::Chat,
                task_control_action: None,
                reply_tts: Some(reply.into()),
                spoken_ack: None,
                task_goal: None,
                requires_confirmation: None,
            },
        }
    }

    pub fn agent_task(
        spoken_ack: impl Into<String>,
        goal: impl Into<String>,
        requires_confirmation: bool,
    ) -> Self {
        Self {
            decision: VoiceDecision {
                intent_type: crate::contract::IntentType::AgentTask,
                task_control_action: None,
                reply_tts: None,
                spoken_ack: Some(spoken_ack.into()),
                task_goal: Some(goal.into()),
                requires_confirmation: Some(requires_confirmation),
            },
        }
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
#[async_trait]
impl VoiceRouter for StubVoiceRouter {
    async fn route(&self, _input: &str) -> Result<VoiceDecision, RouteError> {
        Ok(self.decision.clone())
    }

    async fn route_with_status(
        &self,
        _input: &str,
        _task_status: Option<&str>,
    ) -> Result<VoiceDecision, RouteError> {
        Ok(self.decision.clone())
    }
}

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Headless Voice Session Driver orchestrating FSM state transitions, event buffering, and watchdog timeouts.
pub struct VoiceSessionDriver {
    fsm: AcousticFsm,
    history: Vec<AcousticState>,
    pending_events: VecDeque<AcousticEvent>,
    confirmation_deadline: Option<Instant>,
}

impl VoiceSessionDriver {
    pub const MAX_HISTORY_CAPACITY: usize = 256;

    pub fn new() -> Self {
        Self {
            fsm: AcousticFsm::new(),
            history: vec![AcousticState::Idle],
            pending_events: VecDeque::new(),
            confirmation_deadline: None,
        }
    }

    pub fn current_state(&self) -> AcousticState {
        self.fsm.state()
    }

    pub fn state_history(&self) -> &[AcousticState] {
        &self.history
    }

    pub fn pending_events_count(&self) -> usize {
        self.pending_events.len()
    }

    pub fn handle_event(&mut self, event: AcousticEvent) -> Result<AcousticState, IllegalTransitionError> {
        // If an asynchronous background notification arrives while non-idle, queue it to avoid dropping
        if (event == AcousticEvent::TaskSummaryReady || event == AcousticEvent::ToolPolicyIntercept)
            && self.fsm.state() != AcousticState::Idle
        {
            self.pending_events.push_back(event);
            return Ok(self.fsm.state());
        }

        let next = self.fsm.transition(event)?;
        self.history.push(next);
        if self.history.len() > Self::MAX_HISTORY_CAPACITY {
            self.history.remove(0);
        }

        // Manage 8-second confirmation watchdog timer
        if next == AcousticState::ListeningConfirm || next == AcousticState::AwaitingConfirmation {
            if self.confirmation_deadline.is_none() {
                self.confirmation_deadline = Some(Instant::now() + Duration::from_secs(8));
            }
        } else {
            self.confirmation_deadline = None;
        }

        // When returning to Idle, replay any queued asynchronous events
        if next == AcousticState::Idle {
            if let Some(queued) = self.pending_events.pop_front() {
                return self.handle_event(queued);
            }
        }

        Ok(next)
    }

    /// Periodic watchdog check: fires ConfirmTimeout if 8-second confirmation window expires.
    pub fn check_watchdog(&mut self) -> Result<Option<AcousticState>, IllegalTransitionError> {
        let state = self.fsm.state();
        if (state == AcousticState::ListeningConfirm || state == AcousticState::AwaitingConfirmation)
            && let Some(deadline) = self.confirmation_deadline
        {
            if Instant::now() >= deadline {
                self.confirmation_deadline = None;
                let next = self.handle_event(AcousticEvent::ConfirmTimeout)?;
                return Ok(Some(next));
            }
        }
        Ok(None)
    }

    pub fn reset(&mut self) {
        self.fsm = AcousticFsm::new();
        self.history = vec![AcousticState::Idle];
        self.pending_events.clear();
        self.confirmation_deadline = None;
    }
}

impl Default for VoiceSessionDriver {
    fn default() -> Self {
        Self::new()
    }
}

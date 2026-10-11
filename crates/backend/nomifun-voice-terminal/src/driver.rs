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
    async fn route(&self, input: &str) -> Result<VoiceDecision, RouteError>;
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
}

/// Headless Voice Session Driver orchestrating FSM state transitions and event history.
pub struct VoiceSessionDriver {
    fsm: AcousticFsm,
    history: Vec<AcousticState>,
}

impl VoiceSessionDriver {
    pub fn new() -> Self {
        Self {
            fsm: AcousticFsm::new(),
            history: vec![AcousticState::Idle],
        }
    }

    pub fn current_state(&self) -> AcousticState {
        self.fsm.state()
    }

    pub fn state_history(&self) -> &[AcousticState] {
        &self.history
    }

    pub fn handle_event(&mut self, event: AcousticEvent) -> Result<AcousticState, IllegalTransitionError> {
        let next = self.fsm.transition(event)?;
        self.history.push(next);
        Ok(next)
    }

    pub fn reset(&mut self) {
        self.fsm = AcousticFsm::new();
        self.history = vec![AcousticState::Idle];
    }
}

impl Default for VoiceSessionDriver {
    fn default() -> Self {
        Self::new()
    }
}

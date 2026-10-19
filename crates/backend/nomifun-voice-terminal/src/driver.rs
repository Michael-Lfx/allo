//! Voice Session Driver and Router abstractions.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.5, §3.6.

use crate::contract::{
    AcousticEvent, AcousticFsm, AcousticState, IllegalTransitionError, VoiceDecision,
};
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("Voice history projection failed: {code}")]
    Record {code:String,content:String},
    #[error("Routing generation failed: {0}")]
    Generation(String),
    #[error("Validation failed: {0:?}")]
    Validation(Vec<String>),
    #[error("Network request failed: {0}")]
    Network(String),
}

#[async_trait]
pub trait VoiceRouter: Send + Sync {
    async fn route_scoped_stream(&self,request:&nomifun_api_types::voice::VoiceDialogueRequest,input:&str,task_status:Option<&str>,context:&[crate::router::ChatMessage],_sentences:tokio::sync::mpsc::Sender<nomifun_api_types::voice::VoiceDialogueSentence>)->(Result<VoiceDecision,RouteError>,Option<String>) {
        self.route_scoped(request,input,task_status,context).await
    }
    async fn route_scoped(
        &self, request: &nomifun_api_types::voice::VoiceDialogueRequest,
        input: &str, task_status: Option<&str>, context: &[crate::router::ChatMessage],
    ) -> (Result<VoiceDecision, RouteError>, Option<String>) {
        let _ = request;
        self.route_with_context(input, task_status, context).await
    }
    /// Session-owned history is supplied explicitly, so shared model adapters
    /// cannot retain an ended session or another user's conversation.
    async fn route_with_context(
        &self,
        input: &str,
        task_status: Option<&str>,
        _context: &[crate::router::ChatMessage],
    ) -> (Result<VoiceDecision, RouteError>, Option<String>) {
        self.route_with_output(input, task_status).await
    }
    /// Preserve the actual model message for product display, including invalid
    /// structured replies. The decision remains the only routing authority.
    async fn route_with_output(
        &self,
        input: &str,
        task_status: Option<&str>,
    ) -> (Result<VoiceDecision, RouteError>, Option<String>) {
        (self.route_with_status(input, task_status).await, None)
    }
    fn is_configured(&self) -> bool {
        false
    }
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
    configured: bool,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(any(test, feature = "voice-skeleton"))]
impl StubVoiceRouter {
    pub fn new(decision: VoiceDecision) -> Self {
        Self {
            decision,
            configured: true,
            calls: Default::default(),
        }
    }

    pub fn unavailable(mut self) -> Self {
        self.configured = false;
        self
    }
    pub fn call_count(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn chat(reply: impl Into<String>) -> Self {
        Self::new(VoiceDecision {
            intent_type: crate::contract::IntentType::Chat,
            understood_request: None,
            task_control_action: None,
            reply_tts: Some(reply.into()),
            spoken_ack: None,
            task_goal: None,
            requires_confirmation: None,
        })
    }

    pub fn agent_task(
        spoken_ack: impl Into<String>,
        goal: impl Into<String>,
        requires_confirmation: bool,
    ) -> Self {
        Self::new(VoiceDecision {
            intent_type: crate::contract::IntentType::AgentTask,
            understood_request: None,
            task_control_action: None,
            reply_tts: None,
            spoken_ack: Some(spoken_ack.into()),
            task_goal: Some(goal.into()),
            requires_confirmation: Some(requires_confirmation),
        })
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
#[async_trait]
impl VoiceRouter for StubVoiceRouter {
    fn is_configured(&self) -> bool {
        self.configured
    }
    async fn route(&self, _input: &str) -> Result<VoiceDecision, RouteError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.decision.clone())
    }

    async fn route_with_status(
        &self,
        _input: &str,
        _task_status: Option<&str>,
    ) -> Result<VoiceDecision, RouteError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    listening_deadline: Option<Instant>,
    speech_started: bool,
}

impl VoiceSessionDriver {
    pub const MAX_HISTORY_CAPACITY: usize = 256;

    pub fn new() -> Self {
        Self {
            fsm: AcousticFsm::new(),
            history: vec![AcousticState::Idle],
            pending_events: VecDeque::new(),
            confirmation_deadline: None,
            listening_deadline: None,
            speech_started: false,
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

    pub fn handle_event(
        &mut self,
        event: AcousticEvent,
    ) -> Result<AcousticState, IllegalTransitionError> {
        // If an asynchronous background notification arrives while non-idle, queue it to avoid dropping
        if (event == AcousticEvent::TaskSummaryReady || event == AcousticEvent::ToolPolicyIntercept)
            && self.fsm.state() != AcousticState::Idle
        {
            self.pending_events.push_back(event);
            return Ok(self.fsm.state());
        }

        let next = self.fsm.transition(event)?;
        if event == AcousticEvent::WakeHit {
            self.speech_started = false;
            self.listening_deadline = Some(Instant::now() + Duration::from_secs(6));
        } else if next == AcousticState::FollowUpListening && event == AcousticEvent::PlaybackDone {
            self.speech_started = false;
            self.listening_deadline = None;
        } else if !matches!(
            next,
            AcousticState::Listening | AcousticState::FollowUpListening
        ) {
            self.listening_deadline = None;
        }
        self.history.push(next);
        if self.history.len() > Self::MAX_HISTORY_CAPACITY {
            self.history.remove(0);
        }

        // Manage 8-second confirmation watchdog timer
        if next == AcousticState::ListeningConfirm {
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
        if matches!(
            state,
            AcousticState::Listening | AcousticState::FollowUpListening
        ) && self
            .listening_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            let event = if state == AcousticState::Listening {
                AcousticEvent::ListenTimeout
            } else {
                AcousticEvent::FollowUpTimeout
            };
            return self.handle_event(event).map(Some);
        }
        if (state == AcousticState::ListeningConfirm
            || state == AcousticState::AwaitingConfirmation)
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

    /// Silence waiting ends when the capture worker observes real speech.
    pub fn mark_speech_started(&mut self) {
        self.speech_started = true;
        self.listening_deadline = None;
    }

    pub fn limit_confirmation_window(&mut self,remaining:Duration) {
        if self.current_state()==AcousticState::ListeningConfirm {
            let limit=Instant::now()+remaining.min(Duration::from_secs(8));
            self.confirmation_deadline=Some(self.confirmation_deadline.map_or(limit,|deadline|deadline.min(limit)));
        }
    }

    pub fn pause_listening_for_feedback(&mut self) {
        self.listening_deadline = None;
    }

    pub fn remaining_listening_time(&self) -> Option<Duration> {
        self.listening_deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn compensate_listening_feedback(&mut self, elapsed: Duration) -> Option<Duration> {
        if self.fsm.state() != AcousticState::Listening || self.speech_started {
            return None;
        }
        let deadline = self.listening_deadline.as_mut()?;
        *deadline += elapsed;
        Some(deadline.saturating_duration_since(Instant::now()))
    }

    /// A cue completion must not rearm a deadline after speech has already begun.
    pub fn begin_listening_window(&mut self) -> bool {
        if self.speech_started {
            return false;
        }
        let seconds = match self.fsm.state() {
            AcousticState::Listening => 6,
            AcousticState::FollowUpListening => 4,
            _ => return false,
        };
        self.listening_deadline = Some(Instant::now() + Duration::from_secs(seconds));
        true
    }

    pub fn reset(&mut self) {
        self.fsm = AcousticFsm::new();
        self.history = vec![AcousticState::Idle];
        self.pending_events.clear();
        self.confirmation_deadline = None;
        self.listening_deadline = None;
        self.speech_started = false;
    }
}

impl Default for VoiceSessionDriver {
    fn default() -> Self {
        Self::new()
    }
}

//! C3 and C4 Contract definitions for Voice Terminal
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md (§3.5, §3.6)

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

// ============================================================================
// C3: Voice Decision Contract & JSON Schema
// ============================================================================

/// JSON Schema for single-pass quad-route voice decision contract.
/// Strictly defined in docs/architecture/voice-agent/interaction.zh.md §3.5.
pub const VOICE_DECISION_SCHEMA: &str = r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "VoiceDecisionContract",
  "type": "object",
  "properties": {
    "intent_type": {
      "type": "string",
      "enum": ["ignore", "chat", "agent_task", "task_control"]
    },
    "task_control_action": {
      "type": "string",
      "enum": ["query_status", "cancel_task"],
      "description": "当 intent_type 为 task_control 时的具体控制动作"
    },
    "reply_tts": {
      "type": "string",
      "description": "当 intent_type 为 chat 时的直接口语回答 (≤60字)"
    },
    "understood_request": {
      "type": "string",
      "minLength": 1,
      "description": "向用户展示的需求理解，忠实整理 ASR 输入，不补造未说出的事实"
    },
    "spoken_ack": {
      "type": "string",
      "description": "当 intent_type 为 agent_task 时的先行接单回执 (≤25字)"
    },
    "task_goal": {
      "type": "string",
      "description": "当 intent_type 为 agent_task 时的纯净规整执行目标"
    },
    "requires_confirmation": {
      "type": "boolean",
      "description": "模型建议的是否需要用户口头确认"
    }
  },
  "required": ["intent_type"]
}"#;

pub use nomifun_api_types::voice::{VoiceIntentType as IntentType,VoiceTaskControlAction as TaskControlAction,VoiceDecision};

static COMPILED_SCHEMA: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let schema_json: serde_json::Value = serde_json::from_str(VOICE_DECISION_SCHEMA)
        .expect("VOICE_DECISION_SCHEMA must be valid JSON");
    jsonschema::options()
        .build(&schema_json)
        .expect("VOICE_DECISION_SCHEMA must compile successfully")
});

/// Validates a JSON value against the frozen C3 VoiceDecisionContract JSON Schema.
pub fn validate_voice_decision(value: &serde_json::Value) -> Result<(), Vec<String>> {
    let errors: Vec<String> = COMPILED_SCHEMA
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ============================================================================
// C4: Acoustic FSM State, Event & Declarative Legal Transitions
// ============================================================================

/// Acoustic interaction FSM state enumeration.
/// Strictly defined in docs/architecture/voice-agent/interaction.zh.md §3.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcousticState {
    /// Device ready and idle in standby
    Idle,
    /// Far-field wake hit, listening to user speech
    Listening,
    /// VAD end of utterance detected, awaiting single-pass intent routing
    Deciding,
    /// Fast-path chat streaming playback (<850ms)
    SpeakingFast,
    /// Task control feedback playback (status broadcast or cancellation ack)
    SpeakingControl,
    /// High-risk action intercepted, playing alert prompt before confirmation
    AwaitingConfirmation,
    /// 8-second timeout confirmation listening window
    ListeningConfirm,
    /// Dispatching accepted task to async agent task queue
    DispatchingTask,
    /// Background task completed, playing summary report
    SpeakingSummary,
    /// 180-second warm follow-up listening window without wake word
    FollowUpListening,
}

/// Acoustic interaction FSM event enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcousticEvent {
    /// Far-field keyword "Hey Flowy" detected
    WakeHit,
    /// VAD detected user pause / end of utterance (EOU)
    SpeechEnd,
    /// 6-second silence timeout without speech
    ListenTimeout,
    /// Intent resolved to ignore (false wake)
    IntentIgnore,
    /// Intent resolved to chat (fast conversational response)
    IntentChat,
    /// Intent resolved to task_control (status query or cancel)
    IntentTaskControl,
    /// High-risk operation requires oral confirmation
    ConfirmationRequired,
    /// Intent resolved to agent_task (safe for dispatch)
    IntentAgentTask,
    /// Confirmation prompt finished speaking, enter 8s confirm window
    PromptDone,
    /// User confirmed with "确认" / token verified
    ConfirmAccepted,
    /// User rejected / cancelled
    ConfirmRejected,
    /// 8-second confirmation window timed out
    ConfirmTimeout,
    /// Task accepted by background queue and spoken ack playback finished
    TaskAccepted,
    /// Background task finished and summary ready (Task FSM async notice)
    TaskSummaryReady,
    /// Execution-time Tool Policy intercepted high-risk destructive action
    ToolPolicyIntercept,
    /// Audio playback finished (enters 4s follow-up window)
    PlaybackDone,
    /// Human speech follow-up detected within 4s window
    FollowUpSpeech,
    /// 4-second follow-up window expired without speech
    FollowUpTimeout,
}

/// Error returned when an illegal FSM transition is attempted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Illegal acoustic FSM transition from {from:?} on {event:?}")]
pub struct IllegalTransitionError {
    pub from: AcousticState,
    pub event: AcousticEvent,
}

/// Declarative legal transition table for Acoustic FSM.
/// Contains all 20 canonical state transitions defined in interaction.zh.md §3.6.
pub const LEGAL_TRANSITIONS: &[(AcousticState, AcousticEvent, AcousticState)] = &[
    // 1. Idle -> Listening on wake word
    (
        AcousticState::Idle,
        AcousticEvent::WakeHit,
        AcousticState::Listening,
    ),
    // 2. Listening -> Deciding on speech end
    (
        AcousticState::Listening,
        AcousticEvent::SpeechEnd,
        AcousticState::Deciding,
    ),
    // 3. Listening -> Idle on 6s silence timeout
    (
        AcousticState::Listening,
        AcousticEvent::ListenTimeout,
        AcousticState::Idle,
    ),
    // 4. Deciding -> Idle on intent == ignore
    (
        AcousticState::Deciding,
        AcousticEvent::IntentIgnore,
        AcousticState::Idle,
    ),
    // 5. Deciding -> SpeakingFast on intent == chat
    (
        AcousticState::Deciding,
        AcousticEvent::IntentChat,
        AcousticState::SpeakingFast,
    ),
    // 6. Deciding -> SpeakingControl on intent == task_control
    (
        AcousticState::Deciding,
        AcousticEvent::IntentTaskControl,
        AcousticState::SpeakingControl,
    ),
    // 7. Deciding -> AwaitingConfirmation on routing confirmation needed
    (
        AcousticState::Deciding,
        AcousticEvent::ConfirmationRequired,
        AcousticState::AwaitingConfirmation,
    ),
    // 8. Deciding -> DispatchingTask on intent == agent_task
    (
        AcousticState::Deciding,
        AcousticEvent::IntentAgentTask,
        AcousticState::DispatchingTask,
    ),
    // 9. AwaitingConfirmation -> ListeningConfirm when prompt finishes
    (
        AcousticState::AwaitingConfirmation,
        AcousticEvent::PromptDone,
        AcousticState::ListeningConfirm,
    ),
    // 10. ListeningConfirm -> DispatchingTask on confirm accepted
    (
        AcousticState::ListeningConfirm,
        AcousticEvent::ConfirmAccepted,
        AcousticState::DispatchingTask,
    ),
    // 11. ListeningConfirm -> Idle on user cancel
    (
        AcousticState::ListeningConfirm,
        AcousticEvent::ConfirmRejected,
        AcousticState::Idle,
    ),
    // 12. ListeningConfirm -> Idle on 8s confirm timeout
    (
        AcousticState::ListeningConfirm,
        AcousticEvent::ConfirmTimeout,
        AcousticState::Idle,
    ),
    // 13. DispatchingTask -> Idle on task accepted & ack done
    (
        AcousticState::DispatchingTask,
        AcousticEvent::TaskAccepted,
        AcousticState::Idle,
    ),
    // 14. Idle -> SpeakingSummary on async task completion notice
    (
        AcousticState::Idle,
        AcousticEvent::TaskSummaryReady,
        AcousticState::SpeakingSummary,
    ),
    // 15. Idle -> AwaitingConfirmation on execution-time tool policy intercept
    (
        AcousticState::Idle,
        AcousticEvent::ToolPolicyIntercept,
        AcousticState::AwaitingConfirmation,
    ),
    // 16. SpeakingFast -> FollowUpListening when chat playback completes
    (
        AcousticState::SpeakingFast,
        AcousticEvent::PlaybackDone,
        AcousticState::FollowUpListening,
    ),
    // 17. SpeakingControl -> FollowUpListening when control playback completes
    (
        AcousticState::SpeakingControl,
        AcousticEvent::PlaybackDone,
        AcousticState::FollowUpListening,
    ),
    // 18. SpeakingSummary -> FollowUpListening when summary playback completes
    (
        AcousticState::SpeakingSummary,
        AcousticEvent::PlaybackDone,
        AcousticState::FollowUpListening,
    ),
    // 19. FollowUpListening -> Deciding on follow-up speech within 4s
    (
        AcousticState::FollowUpListening,
        AcousticEvent::FollowUpSpeech,
        AcousticState::Deciding,
    ),
    // 20. FollowUpListening -> Idle on 4s follow-up timeout
    (
        AcousticState::FollowUpListening,
        AcousticEvent::FollowUpTimeout,
        AcousticState::Idle,
    ),
];

/// Looks up the next state in the declarative transition table.
pub fn next_state(
    current: AcousticState,
    event: AcousticEvent,
) -> Result<AcousticState, IllegalTransitionError> {
    for &(from, e, to) in LEGAL_TRANSITIONS {
        if from == current && e == event {
            return Ok(to);
        }
    }
    Err(IllegalTransitionError {
        from: current,
        event,
    })
}

/// Headless Acoustic State Machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcousticFsm {
    current_state: AcousticState,
}

impl AcousticFsm {
    /// Creates a new AcousticFsm initialized to Idle.
    pub fn new() -> Self {
        Self {
            current_state: AcousticState::Idle,
        }
    }

    /// Returns the current FSM state.
    pub fn state(&self) -> AcousticState {
        self.current_state
    }

    /// Transitions to the next state on the given event according to the declarative table.
    pub fn transition(
        &mut self,
        event: AcousticEvent,
    ) -> Result<AcousticState, IllegalTransitionError> {
        let next = next_state(self.current_state, event)?;
        self.current_state = next;
        Ok(next)
    }
}

impl Default for AcousticFsm {
    fn default() -> Self {
        Self::new()
    }
}

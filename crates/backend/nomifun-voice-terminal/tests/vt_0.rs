//! Vertical Slice Test 0 (VT-0) and Contract Verification
//! Covering:
//! - T0-03: C3 Voice Decision JSON Schema & golden tests
//! - T0-04: C4 Acoustic FSM state/event enumerations & declarative legal transitions (VT-0-04)
//! - T0-12: VoiceSessionDriver and StubVoiceRouter (VT-0-01)
//! - T0-13: PlaybackPipeline, AudioSink, and NullTts (VT-0-02)

use std::sync::Arc;
use nomifun_audio::EarconKind;
use nomifun_voice_terminal::*;
use serde_json::json;

// ============================================================================
// VT-0-01: Terminal Driver Wake to Listening
// ============================================================================

#[tokio::test]
async fn test_vt_0_01_wake_to_listening() {
    let mut driver = VoiceSessionDriver::new();
    assert_eq!(driver.current_state(), AcousticState::Idle);
    assert_eq!(driver.state_history(), &[AcousticState::Idle]);

    let next = driver
        .handle_event(AcousticEvent::WakeHit)
        .expect("WakeHit must succeed from Idle");
    assert_eq!(next, AcousticState::Listening);
    assert_eq!(driver.current_state(), AcousticState::Listening);
    assert_eq!(
        driver.state_history(),
        &[AcousticState::Idle, AcousticState::Listening]
    );
}

// ============================================================================
// VT-0-02: Stub Route -> NullTts -> PlaybackPipeline -> AudioSink
// ============================================================================

#[tokio::test]
async fn test_vt_0_02_stub_route_tts_playback() {
    let sink = Arc::new(TestAudioSink::new());
    let pipeline = PlaybackPipeline::new(sink.clone());

    // 1. Earcon wake.wav path produces output frames
    pipeline
        .play_earcon(EarconKind::Wake)
        .await
        .expect("play_earcon wake must succeed");
    let earcon_frames = sink.frame_count();
    assert!(earcon_frames > 0, "Earcon path must push non-empty frames");

    // 2. Stub router triggers Chat decision
    let router = StubVoiceRouter::chat("你好，我是 Flowy");
    let decision = router.route("Hey Flowy").await.expect("stub route must succeed");
    assert_eq!(decision.intent_type, IntentType::Chat);
    let reply_text = decision.reply_tts.as_deref().unwrap();

    // 3. NullTts generates chunks
    let tts = NullTts::new();
    let stream = tts
        .synthesize_stream(reply_text, "zh-CN-XiaoxiaoNeural")
        .await
        .expect("synthesize_stream must succeed");

    // 4. PlaybackPipeline processes stream
    pipeline.play_stream(stream).await.expect("play_stream must succeed");
    let total_frames = sink.frame_count();
    assert!(
        total_frames > earcon_frames,
        "TTS stream must produce additional frames in audio sink"
    );

    for frame in sink.frames() {
        assert!(!frame.is_empty(), "Audio frame must not be empty");
    }
}

// ============================================================================
// T0-03 / C3: Voice Decision JSON Schema Tests
// ============================================================================

#[test]
fn test_c3_voice_decision_schema_golden() {
    let schema_val: serde_json::Value = serde_json::from_str(VOICE_DECISION_SCHEMA)
        .expect("VOICE_DECISION_SCHEMA must be valid JSON");

    assert_eq!(
        schema_val.get("$schema").and_then(|v| v.as_str()),
        Some("http://json-schema.org/draft-07/schema#")
    );
    assert_eq!(
        schema_val.get("title").and_then(|v| v.as_str()),
        Some("VoiceDecisionContract")
    );
    assert_eq!(
        schema_val.get("type").and_then(|v| v.as_str()),
        Some("object")
    );

    let required = schema_val
        .get("required")
        .and_then(|v| v.as_array())
        .expect("required must be an array");
    assert_eq!(required, &vec![json!("intent_type")]);

    let props = schema_val
        .get("properties")
        .and_then(|v| v.as_object())
        .expect("properties must be an object");

    // All 6 contract fields from interaction.zh.md §3.5 must exist
    assert!(props.contains_key("intent_type"));
    assert!(props.contains_key("task_control_action"));
    assert!(props.contains_key("reply_tts"));
    assert!(props.contains_key("spoken_ack"));
    assert!(props.contains_key("task_goal"));
    assert!(props.contains_key("requires_confirmation"));

    // Verify intent_type enum
    let intent_enums = props["intent_type"]["enum"]
        .as_array()
        .expect("intent_type enum must be array");
    let mut intents: Vec<&str> = intent_enums.iter().filter_map(|v| v.as_str()).collect();
    intents.sort();
    assert_eq!(intents, vec!["agent_task", "chat", "ignore", "task_control"]);

    // Verify task_control_action enum
    let action_enums = props["task_control_action"]["enum"]
        .as_array()
        .expect("task_control_action enum must be array");
    let mut actions: Vec<&str> = action_enums.iter().filter_map(|v| v.as_str()).collect();
    actions.sort();
    assert_eq!(actions, vec!["cancel_task", "query_status"]);
}

#[test]
fn test_c3_voice_decision_schema_valid_examples() {
    let valid_chat = json!({
        "intent_type": "chat",
        "reply_tts": "今天北京天气晴朗，气温22摄氏度。"
    });
    assert!(validate_voice_decision(&valid_chat).is_ok());
    let dec_chat: VoiceDecision = serde_json::from_value(valid_chat).unwrap();
    assert_eq!(dec_chat.intent_type, IntentType::Chat);
    assert_eq!(
        dec_chat.reply_tts.as_deref(),
        Some("今天北京天气晴朗，气温22摄氏度。")
    );

    let valid_agent_task = json!({
        "intent_type": "agent_task",
        "spoken_ack": "好的，正在帮您分析最近日志。",
        "task_goal": "分析最近日志",
        "requires_confirmation": false
    });
    assert!(validate_voice_decision(&valid_agent_task).is_ok());
    let dec_task: VoiceDecision = serde_json::from_value(valid_agent_task).unwrap();
    assert_eq!(dec_task.intent_type, IntentType::AgentTask);
    assert_eq!(
        dec_task.spoken_ack.as_deref(),
        Some("好的，正在帮您分析最近日志。")
    );
    assert_eq!(dec_task.task_goal.as_deref(), Some("分析最近日志"));
    assert_eq!(dec_task.requires_confirmation, Some(false));

    let valid_query_status = json!({
        "intent_type": "task_control",
        "task_control_action": "query_status"
    });
    assert!(validate_voice_decision(&valid_query_status).is_ok());
    let dec_query: VoiceDecision = serde_json::from_value(valid_query_status).unwrap();
    assert_eq!(dec_query.intent_type, IntentType::TaskControl);
    assert_eq!(
        dec_query.task_control_action,
        Some(TaskControlAction::QueryStatus)
    );

    let valid_cancel_task = json!({
        "intent_type": "task_control",
        "task_control_action": "cancel_task"
    });
    assert!(validate_voice_decision(&valid_cancel_task).is_ok());
    let dec_cancel: VoiceDecision = serde_json::from_value(valid_cancel_task).unwrap();
    assert_eq!(dec_cancel.intent_type, IntentType::TaskControl);
    assert_eq!(
        dec_cancel.task_control_action,
        Some(TaskControlAction::CancelTask)
    );

    let valid_ignore = json!({
        "intent_type": "ignore"
    });
    assert!(validate_voice_decision(&valid_ignore).is_ok());
    let dec_ignore: VoiceDecision = serde_json::from_value(valid_ignore).unwrap();
    assert_eq!(dec_ignore.intent_type, IntentType::Ignore);
}

#[test]
fn test_c3_voice_decision_schema_missing_intent_type_rejected() {
    let missing_intent = json!({
        "reply_tts": "你好"
    });
    let result = validate_voice_decision(&missing_intent);
    assert!(result.is_err(), "Missing intent_type must be rejected");

    let invalid_intent = json!({
        "intent_type": "unknown_action"
    });
    let result = validate_voice_decision(&invalid_intent);
    assert!(result.is_err(), "Unknown intent_type must be rejected");

    let invalid_action = json!({
        "intent_type": "task_control",
        "task_control_action": "pause_task"
    });
    let result = validate_voice_decision(&invalid_action);
    assert!(result.is_err(), "Unknown task_control_action must be rejected");

    let invalid_bool = json!({
        "intent_type": "agent_task",
        "requires_confirmation": "yes"
    });
    let result = validate_voice_decision(&invalid_bool);
    assert!(
        result.is_err(),
        "Non-boolean requires_confirmation must be rejected"
    );
}

// ============================================================================
// T0-04 / C4: Acoustic FSM State, Event, and Transition Table Tests (VT-0-04)
// ============================================================================

#[test]
fn test_c4_fsm_legal_transitions_vt_0_04() {
    assert_eq!(
        LEGAL_TRANSITIONS.len(),
        25,
        "Includes the recoverable ignored-utterance transition and barge-in transitions"
    );

    for (index, &(from, event, expected_to)) in LEGAL_TRANSITIONS.iter().enumerate() {
        assert!(!LEGAL_TRANSITIONS[..index].iter().any(|&(s, e, _)| s == from && e == event), "Ambiguous transition");
        let res = next_state(from, event);
        assert_eq!(
            res,
            Ok(expected_to),
            "Expected legal transition from {from:?} on {event:?} to {expected_to:?}"
        );
    }
}

#[test]
fn test_c4_fsm_illegal_transitions_rejected() {
    let all_states = [
        AcousticState::Idle,
        AcousticState::Listening,
        AcousticState::Deciding,
        AcousticState::SpeakingFast,
        AcousticState::SpeakingControl,
        AcousticState::AwaitingConfirmation,
        AcousticState::ListeningConfirm,
        AcousticState::DispatchingTask,
        AcousticState::SpeakingSummary,
        AcousticState::FollowUpListening,
    ];

    let all_events = [
        AcousticEvent::WakeHit,
        AcousticEvent::SpeechEnd,
        AcousticEvent::ListenTimeout,
        AcousticEvent::IntentIgnore,
        AcousticEvent::UtteranceIgnored,
        AcousticEvent::IntentChat,
        AcousticEvent::IntentTaskControl,
        AcousticEvent::ConfirmationRequired,
        AcousticEvent::IntentAgentTask,
        AcousticEvent::PromptDone,
        AcousticEvent::ConfirmAccepted,
        AcousticEvent::ConfirmRejected,
        AcousticEvent::ConfirmTimeout,
        AcousticEvent::TaskAccepted,
        AcousticEvent::TaskSummaryReady,
        AcousticEvent::ToolPolicyIntercept,
        AcousticEvent::PlaybackDone,
        AcousticEvent::FollowUpSpeech,
        AcousticEvent::FollowUpTimeout,
        AcousticEvent::BargeIn,
    ];

    assert!(LEGAL_TRANSITIONS.iter().all(|&(s, e, to)| all_states.contains(&s) && all_states.contains(&to) && all_events.contains(&e)));
    let mut illegal_count = 0;
    for state in all_states {
        for event in all_events {
            let is_legal = LEGAL_TRANSITIONS
                .iter()
                .any(|&(s, e, _)| s == state && e == event);
            let res = next_state(state, event);
            if is_legal {
                assert!(res.is_ok());
            } else {
                illegal_count += 1;
                assert_eq!(
                    res,
                    Err(IllegalTransitionError { from: state, event }),
                    "Transition from {state:?} on {event:?} must be rejected"
                );
            }
        }
    }

    assert_eq!(
        illegal_count,
        all_states.len() * all_events.len() - LEGAL_TRANSITIONS.len()
    );
}

#[test]
fn test_c4_fsm_full_lifecycle_scenarios() {
    // Scenario 1: Chat loop with follow-up
    let mut fsm = AcousticFsm::new();
    assert_eq!(fsm.state(), AcousticState::Idle);

    assert_eq!(
        fsm.transition(AcousticEvent::WakeHit).unwrap(),
        AcousticState::Listening
    );
    assert_eq!(
        fsm.transition(AcousticEvent::SpeechEnd).unwrap(),
        AcousticState::Deciding
    );
    assert_eq!(
        fsm.transition(AcousticEvent::IntentChat).unwrap(),
        AcousticState::SpeakingFast
    );
    assert_eq!(
        fsm.transition(AcousticEvent::PlaybackDone).unwrap(),
        AcousticState::FollowUpListening
    );
    // Follow-up question without wake word
    assert_eq!(
        fsm.transition(AcousticEvent::FollowUpSpeech).unwrap(),
        AcousticState::Deciding
    );
    assert_eq!(
        fsm.transition(AcousticEvent::IntentChat).unwrap(),
        AcousticState::SpeakingFast
    );
    assert_eq!(
        fsm.transition(AcousticEvent::PlaybackDone).unwrap(),
        AcousticState::FollowUpListening
    );
    // Timeout to Idle
    assert_eq!(
        fsm.transition(AcousticEvent::FollowUpTimeout).unwrap(),
        AcousticState::Idle
    );

    // Scenario 2: False wake up timeout (TC-AM-003)
    let mut fsm = AcousticFsm::new();
    assert_eq!(
        fsm.transition(AcousticEvent::WakeHit).unwrap(),
        AcousticState::Listening
    );
    assert_eq!(
        fsm.transition(AcousticEvent::ListenTimeout).unwrap(),
        AcousticState::Idle
    );

    // An unusable utterance keeps the call alive and the next utterance can reply.
    let mut fsm = AcousticFsm::new();
    fsm.transition(AcousticEvent::WakeHit).unwrap();
    fsm.transition(AcousticEvent::SpeechEnd).unwrap();
    assert_eq!(fsm.transition(AcousticEvent::UtteranceIgnored).unwrap(), AcousticState::Listening);
    fsm.transition(AcousticEvent::SpeechEnd).unwrap();
    assert_eq!(fsm.transition(AcousticEvent::IntentChat).unwrap(), AcousticState::SpeakingFast);

    // Explicit abort remains distinct from an ignored utterance.
    let mut fsm = AcousticFsm::new();
    fsm.transition(AcousticEvent::WakeHit).unwrap();
    fsm.transition(AcousticEvent::SpeechEnd).unwrap();
    assert_eq!(
        fsm.transition(AcousticEvent::IntentIgnore).unwrap(),
        AcousticState::Idle
    );

    // Scenario 4: Task control status query (TC-AM-006)
    let mut fsm = AcousticFsm::new();
    fsm.transition(AcousticEvent::WakeHit).unwrap();
    fsm.transition(AcousticEvent::SpeechEnd).unwrap();
    assert_eq!(
        fsm.transition(AcousticEvent::IntentTaskControl).unwrap(),
        AcousticState::SpeakingControl
    );
    assert_eq!(
        fsm.transition(AcousticEvent::PlaybackDone).unwrap(),
        AcousticState::FollowUpListening
    );
    assert_eq!(
        fsm.transition(AcousticEvent::FollowUpTimeout).unwrap(),
        AcousticState::Idle
    );

    // Scenario 5: High-risk confirmation flow (TC-AM-008)
    let mut fsm = AcousticFsm::new();
    fsm.transition(AcousticEvent::WakeHit).unwrap();
    fsm.transition(AcousticEvent::SpeechEnd).unwrap();
    assert_eq!(
        fsm.transition(AcousticEvent::ConfirmationRequired).unwrap(),
        AcousticState::AwaitingConfirmation
    );
    assert_eq!(
        fsm.transition(AcousticEvent::PromptDone).unwrap(),
        AcousticState::ListeningConfirm
    );
    assert_eq!(
        fsm.transition(AcousticEvent::ConfirmAccepted).unwrap(),
        AcousticState::DispatchingTask
    );
    assert_eq!(
        fsm.transition(AcousticEvent::TaskAccepted).unwrap(),
        AcousticState::Idle
    );

    // Scenario 6: Confirmation timeout (TC-AM-009)
    let mut fsm = AcousticFsm::new();
    fsm.transition(AcousticEvent::WakeHit).unwrap();
    fsm.transition(AcousticEvent::SpeechEnd).unwrap();
    fsm.transition(AcousticEvent::ConfirmationRequired).unwrap();
    fsm.transition(AcousticEvent::PromptDone).unwrap();
    assert_eq!(
        fsm.transition(AcousticEvent::ConfirmTimeout).unwrap(),
        AcousticState::Idle
    );

    // Scenario 7: Execution-time Tool Policy intercept (interaction.zh.md §3.6)
    let mut fsm = AcousticFsm::new();
    assert_eq!(
        fsm.transition(AcousticEvent::ToolPolicyIntercept).unwrap(),
        AcousticState::AwaitingConfirmation
    );
    assert_eq!(
        fsm.transition(AcousticEvent::PromptDone).unwrap(),
        AcousticState::ListeningConfirm
    );
    assert_eq!(
        fsm.transition(AcousticEvent::ConfirmRejected).unwrap(),
        AcousticState::Idle
    );

    // Scenario 8: Background task completion and summary report (TC-AM-013)
    let mut fsm = AcousticFsm::new();
    assert_eq!(
        fsm.transition(AcousticEvent::TaskSummaryReady).unwrap(),
        AcousticState::SpeakingSummary
    );
    assert_eq!(
        fsm.transition(AcousticEvent::PlaybackDone).unwrap(),
        AcousticState::FollowUpListening
    );
    assert_eq!(
        fsm.transition(AcousticEvent::FollowUpTimeout).unwrap(),
        AcousticState::Idle
    );

    // Scenario 9: Barge-in during confirmation prompt transitions to ListeningConfirm without dialogue deadline
    let mut driver = VoiceSessionDriver::new();
    assert_eq!(
        driver
            .handle_event(AcousticEvent::ToolPolicyIntercept)
            .unwrap(),
        AcousticState::AwaitingConfirmation
    );
    assert_eq!(
        driver.handle_event(AcousticEvent::BargeIn).unwrap(),
        AcousticState::ListeningConfirm
    );
    assert!(
        driver.remaining_listening_time().is_none(),
        "ListeningConfirm must not retain dialogue 180s deadline"
    );
}

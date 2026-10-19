//! Single-writer session projection; snapshots survive late subscription.
use nomifun_api_types::voice::{
    VoiceCompanionEvent, VoiceConnectionState, VoiceConversationTurn, VoiceSessionError,
    VoiceSessionPhase, VoiceSessionSnapshot,
};
use std::sync::Mutex;
use tokio::sync::broadcast;

pub(crate) struct VoiceEventHub {
    tx: broadcast::Sender<VoiceCompanionEvent>,
    state: Mutex<VoiceSessionSnapshot>,
    feedback: Mutex<nomifun_api_types::voice::VoiceFeedbackConfig>,
    context: Mutex<Vec<nomifun_voice_terminal::router::ChatMessage>>,
}

impl VoiceEventHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            tx,
            feedback: Mutex::new(Default::default()),
            context: Mutex::new(Vec::new()),
            state: Mutex::new(VoiceSessionSnapshot {
                instance_id: nomifun_common::generate_id(),
                enabled: true,
                connection: VoiceConnectionState::Connected,
                ..Default::default()
            }),
        }
    }

    #[allow(dead_code)]
    pub fn announcement_is_speaking(&self,command_id:&str)->bool {
        let state=self.state.lock().unwrap_or_else(|e|e.into_inner());
        state.phase==VoiceSessionPhase::Speaking && state.turn_id.as_deref()==Some(format!("task-result-{command_id}").as_str())
    }

    pub fn sender(&self) -> broadcast::Sender<VoiceCompanionEvent> {
        self.tx.clone()
    }

    pub fn feedback(&self) -> nomifun_api_types::voice::VoiceFeedbackConfig {
        *self.feedback.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn context(&self) -> Vec<nomifun_voice_terminal::router::ChatMessage> {
        self.context
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_feedback(&self, config: nomifun_api_types::voice::VoiceFeedbackConfig) {
        *self.feedback.lock().unwrap_or_else(|e| e.into_inner()) = config;
    }
    pub fn snapshot(&self) -> VoiceSessionSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn send(
        &self,
        event: VoiceCompanionEvent,
    ) -> Result<usize, broadcast::error::SendError<VoiceCompanionEvent>> {
        use VoiceSessionPhase as Phase;
        if matches!(event, VoiceCompanionEvent::Snapshot { .. }) {
            return self.tx.send(event);
        }
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if let VoiceCompanionEvent::Feedback { diagnostic } = &event {
            if diagnostic.status != nomifun_api_types::voice::VoiceCueStatus::Pending
                && state
                    .feedback
                    .as_ref()
                    .is_none_or(|current| current.request_id != diagnostic.request_id)
            {
                return Ok(0);
            }
        }
        state.seq += 1;
        if matches!(event,VoiceCompanionEvent::SessionEnded{..}|VoiceCompanionEvent::Error{stage:_,code:_}) {
            if let Some(turn)=state.conversation.last_mut(){if turn.reply_state==Some(nomifun_api_types::voice::VoiceReplyState::Streaming){turn.reply_state=Some(nomifun_api_types::voice::VoiceReplyState::Interrupted);}}
        }
        match &event {
            VoiceCompanionEvent::SessionEnded { reason } => {
                self.context
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                state.ended_reason = Some(*reason);
                state.phase = Phase::Idle;
                state.listening_deadline_ms = None;
                state.follow_up_deadline_ms = None;
            }
            VoiceCompanionEvent::Feedback { diagnostic } => {
                state.feedback = Some(diagnostic.clone());
            }
            VoiceCompanionEvent::ModelResponse { text } => {
                if let Some(turn) = state.conversation.last_mut() {
                    turn.model_text = Some(text.clone());
                }
            }
            VoiceCompanionEvent::AudioDiagnostics { diagnostics } => {
                state.audio = Some(diagnostics.clone());
            }
            VoiceCompanionEvent::Availability {
                acoustic_ready,
                dialogue_ready,
                playback_ready,
            } => {
                state.acoustic_ready = *acoustic_ready;
                state.dialogue_ready = *dialogue_ready;
                state.playback_ready = *playback_ready;
            }
            VoiceCompanionEvent::WakeHit { .. } => {
                self.context
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                state.ended_reason = None;
                state.feedback = None;
                state.session_id = Some(nomifun_common::generate_id());
                state.turn_id = None;
                state.user_text = None;
                state.understood_request = None;
                state.assistant_text = None;
                state.error = None;
                state.follow_up_deadline_ms = None;
                state.listening_deadline_ms = None;
                state.phase = Phase::Listening;
            }
            VoiceCompanionEvent::Listening => state.phase = Phase::Listening,
            VoiceCompanionEvent::ListeningDeadlineChanged { remaining_ms } => {
                if state.phase == Phase::Listening && state.listening_deadline_ms.is_some() {
                    state.listening_deadline_ms =
                        Some(chrono::Utc::now().timestamp_millis() + *remaining_ms as i64);
                }
            }
            VoiceCompanionEvent::ListeningReady { timeout_secs } => {
                if state.phase == Phase::Listening {
                    state.listening_deadline_ms = Some(
                        chrono::Utc::now().timestamp_millis() + i64::from(*timeout_secs) * 1000,
                    );
                }
            }
            VoiceCompanionEvent::SpeechStarted => {
                state.listening_deadline_ms = None;
                state.follow_up_deadline_ms = None;
            }
            VoiceCompanionEvent::Transcribing => {
                state.ended_reason = None;
                state.listening_deadline_ms = None;
                state.error = None;
                state.phase = Phase::Transcribing;
                state.turn_id = Some(nomifun_common::generate_id());
                let turn_id = state.turn_id.clone().unwrap();
                state.conversation.push(VoiceConversationTurn {
                    turn_id,
                    ..Default::default()
                });
                if state.conversation.len() > 3 {
                    state.conversation.remove(0);
                }
                state.follow_up_deadline_ms = None;
                state.user_text = None;
                state.understood_request = None;
                state.assistant_text = None;
            }
            VoiceCompanionEvent::SpeechRecognized { text } => {
                state.user_text = Some(text.clone());
                if let Some(turn) = state.conversation.last_mut() {
                    turn.user_text = Some(text.clone());
                }
                state.phase = Phase::Deciding;
            }
            VoiceCompanionEvent::IntentDecided {
                intent,
                reply,
                understood_request,
                ..
            } => {
                if intent == "Chat" {
                    if let (Some(user), Some(assistant)) = (&state.user_text, reply) {
                        use nomifun_voice_terminal::router::ChatMessage;
                        let mut context = self.context.lock().unwrap_or_else(|e| e.into_inner());
                        context.push(ChatMessage {
                            role: "user".into(),
                            content: user.clone(),
                        });
                        context.push(ChatMessage {
                            role: "assistant".into(),
                            content: assistant.clone(),
                        });
                        if context.len() > 6 {
                            context.drain(..2);
                        }
                    }
                }
                state.understood_request = understood_request.clone();
                if let Some(turn) = state.conversation.last_mut() {
                    turn.understood_request = understood_request.clone();
                }
                state.assistant_text = reply.clone();
                if let Some(turn) = state.conversation.last_mut() {
                    turn.assistant_text = reply.clone();
                    turn.reply_state=Some(nomifun_api_types::voice::VoiceReplyState::Complete);
                }
            }
            VoiceCompanionEvent::TaskDispatching => {state.phase = Phase::Dispatching;}
            VoiceCompanionEvent::TaskResultReady { command_id, goal, text, .. } => {
                if let Some(goal) = goal {
                    use nomifun_voice_terminal::router::ChatMessage;
                    let mut context = self.context.lock().unwrap_or_else(|error| error.into_inner());
                    context.push(ChatMessage {role:"user".into(), content:goal.clone()});
                    context.push(ChatMessage {role:"assistant".into(), content:text.clone()});
                    if context.len()>6 {context.drain(..2);}
                }
                let turn_id = format!("task-result-{command_id}");
                state.turn_id = Some(turn_id.clone());
                state.user_text = None;
                state.understood_request = None;
                state.assistant_text = Some(text.clone());
                state.error = None;
                state.ended_reason = None;
                state.conversation.push(VoiceConversationTurn {turn_id,assistant_text:Some(text.clone()),..Default::default()});
                if state.conversation.len()>3 {state.conversation.remove(0);}
                state.phase = Phase::Synthesizing;
            }
            VoiceCompanionEvent::ReplySentence {text,understood_request}=>{
                if let Some(understood)=understood_request {state.understood_request=Some(understood.clone());}
                state.assistant_text.get_or_insert_with(String::new).push_str(text);
                if let Some(turn)=state.conversation.last_mut(){
                    turn.assistant_text.get_or_insert_with(String::new).push_str(text);
                    turn.reply_state=Some(nomifun_api_types::voice::VoiceReplyState::Streaming);
                    if let Some(understood)=understood_request {turn.understood_request=Some(understood.clone());}
                }
            }
            VoiceCompanionEvent::ResponseReady {text} => {
                state.assistant_text=Some(text.clone());
                if let Some(turn)=state.conversation.last_mut() {turn.assistant_text=Some(text.clone());}
            }
            VoiceCompanionEvent::ConfirmationPrompted {approval_id,text,..}=>{
                let turn_id=format!("approval-{approval_id}");
                state.session_id=Some(nomifun_common::generate_id());
                state.turn_id=Some(turn_id.clone());
                state.user_text=None;
                state.understood_request=None;
                state.assistant_text=Some(text.clone());
                state.ended_reason=None;
                state.error=None;
                state.listening_deadline_ms=None;
                state.follow_up_deadline_ms=None;
                state.conversation.push(VoiceConversationTurn {turn_id,assistant_text:Some(text.clone()),..Default::default()});
                if state.conversation.len()>3 {state.conversation.remove(0);}
                state.phase=Phase::AwaitingConfirmation;
            }
            VoiceCompanionEvent::ConfirmationListening {deadline_ms}=>{
                state.phase=Phase::ListeningConfirm;
                state.listening_deadline_ms=Some(*deadline_ms);
            }
            VoiceCompanionEvent::ConfirmationHeard {text}=>{
                state.user_text=Some(text.clone());
                if let Some(turn)=state.conversation.last_mut(){turn.user_text=Some(text.clone());}
            }
            VoiceCompanionEvent::TtsSynthesizing { .. }
            | VoiceCompanionEvent::TtsSpeaking { .. } => {
                state.phase = if matches!(event, VoiceCompanionEvent::TtsSynthesizing { .. }) {
                    Phase::Synthesizing
                } else {
                    Phase::Speaking
                };
            }
            VoiceCompanionEvent::FollowUpWindow { timeout_secs } => {
                state.phase = Phase::FollowUp;
                state.follow_up_deadline_ms =
                    Some(chrono::Utc::now().timestamp_millis() + i64::from(*timeout_secs) * 1000);
            }
            VoiceCompanionEvent::FollowUpStarted { text_only } => {
                state.phase = if *text_only {
                    Phase::TextFollowUp
                } else {
                    Phase::FollowUp
                };
                state.follow_up_deadline_ms = None;
            }
            VoiceCompanionEvent::TextFollowUpWindow { timeout_secs } => {
                state.phase = Phase::TextFollowUp;
                state.follow_up_deadline_ms =
                    Some(chrono::Utc::now().timestamp_millis() + i64::from(*timeout_secs) * 1000);
            }
            VoiceCompanionEvent::Error { stage, code } => {
                let turn_id=if matches!(state.phase,Phase::Idle|Phase::Listening){None}else{state.turn_id.clone()};
                state.phase = Phase::Error;
                state.error = Some(VoiceSessionError {
                    stage: stage.clone(),
                    code: code.clone(),
                });
                if let Some(turn)=state.conversation.last_mut(){if turn_id.as_deref()==Some(turn.turn_id.as_str()){turn.error=Some(VoiceSessionError{stage:stage.clone(),code:code.clone()});}}
            }
            VoiceCompanionEvent::Idle => {
                self.context
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                state.phase = Phase::Idle;
                state.follow_up_deadline_ms = None;
                state.listening_deadline_ms = None;
            }
            VoiceCompanionEvent::Snapshot { .. } => unreachable!(),
        }
        // Publishing under the state lock keeps snapshot sequence and event order consistent.
        let ended = matches!(&event, VoiceCompanionEvent::SessionEnded { .. });
        let _ = self.tx.send(event);
        // Preserve the existing Idle event for consumers that only observe termination.
        if ended {
            let _ = self.tx.send(VoiceCompanionEvent::Idle);
        }
        self.tx.send(VoiceCompanionEvent::Snapshot {
            snapshot: state.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_reply_and_its_error_stay_bound_to_the_failed_turn() {
        let hub=VoiceEventHub::new();let _listener=hub.sender().subscribe();
        hub.send(VoiceCompanionEvent::WakeHit{keyword:"Hey Flowy".into()}).unwrap();
        hub.send(VoiceCompanionEvent::Transcribing).unwrap();
        hub.send(VoiceCompanionEvent::SpeechRecognized{text:"问题".into()}).unwrap();
        hub.send(VoiceCompanionEvent::ReplySentence{text:"第一句。".into(),understood_request:Some("需求".into())}).unwrap();
        assert!(hub.context().is_empty(),"partial generation must not enter completed context");
        assert_eq!(hub.snapshot().conversation[0].reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Streaming));
        hub.send(VoiceCompanionEvent::Error{stage:"dialogue".into(),code:"model_timeout".into()}).unwrap();
        hub.send(VoiceCompanionEvent::WakeHit{keyword:"Hey Flowy".into()}).unwrap();
        hub.send(VoiceCompanionEvent::Transcribing).unwrap();
        let snap=hub.snapshot();assert!(snap.error.is_none());
        assert_eq!(snap.conversation[0].reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Interrupted));
        assert_eq!(snap.conversation[0].error.as_ref().unwrap().code,"model_timeout");
        assert_eq!(snap.conversation[0].assistant_text.as_deref(),Some("第一句。"));
    }

    #[test]
    fn session_end_and_failure_idle_erase_inference_context_but_keep_reading_history() {
        for end in [
            VoiceCompanionEvent::SessionEnded {
                reason: nomifun_api_types::voice::VoiceSessionEndReason::UserStop,
            },
            VoiceCompanionEvent::Idle,
        ] {
            let hub = VoiceEventHub::new();
            let _listener = hub.sender().subscribe();
            hub.send(VoiceCompanionEvent::WakeHit {
                keyword: "Hey Flowy".into(),
            })
            .unwrap();
            hub.send(VoiceCompanionEvent::Transcribing).unwrap();
            hub.send(VoiceCompanionEvent::SpeechRecognized {
                text: "记住这是当前会话的内容".into(),
            })
            .unwrap();
            hub.send(VoiceCompanionEvent::IntentDecided {
                intent: "Chat".into(),
                reply: Some("这是本轮回答".into()),
                understood_request: None,
            })
            .unwrap();
            assert_eq!(hub.context().len(), 2);
            hub.send(end).unwrap();
            assert!(hub.context().is_empty());
            let snapshot = hub.snapshot();
            assert_eq!(snapshot.conversation.len(), 1);
            assert_eq!(
                snapshot.conversation[0].assistant_text.as_deref(),
                Some("这是本轮回答")
            );
        }
    }
}

//! Single-writer session projection; snapshots survive late subscription.
use nomifun_api_types::voice::{
    VoiceCompanionEvent, VoiceConnectionState, VoiceConversationTurn, VoiceSessionError,
    VoiceSessionControlRequest, VoiceSessionPhase, VoiceSessionSnapshot,
};
use std::sync::Mutex;
use tokio::sync::broadcast;

pub(crate) struct VoiceEventHub {
    tx: broadcast::Sender<VoiceCompanionEvent>,
    state: Mutex<VoiceSessionSnapshot>,
    feedback: Mutex<nomifun_api_types::voice::VoiceFeedbackConfig>,
    context: Mutex<Vec<nomifun_voice_terminal::router::ChatMessage>>,
    last_task_seq: std::sync::atomic::AtomicU64,
    worker_epoch: std::sync::atomic::AtomicU64,
}

fn current_turn_mut(state: &mut VoiceSessionSnapshot) -> Option<&mut VoiceConversationTurn> {
    let current_id = state.turn_id.as_ref()?;
    state.conversation.iter_mut().rev().find(|t| &t.turn_id == current_id)
}

impl VoiceEventHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            tx,
            feedback: Mutex::new(Default::default()),
            context: Mutex::new(Vec::new()),
            last_task_seq: std::sync::atomic::AtomicU64::new(0),
            worker_epoch: std::sync::atomic::AtomicU64::new(0),
            state: Mutex::new(VoiceSessionSnapshot {
                instance_id: nomifun_common::generate_id(),
                enabled: true,
                connection: VoiceConnectionState::Connected,
                ..Default::default()
            }),
        }
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

    pub fn set_worker_epoch(&self, epoch: u64) {
        self.worker_epoch.store(epoch, std::sync::atomic::Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.worker_epoch = (epoch > 0).then_some(epoch);
    }

    #[allow(dead_code)]
    pub fn worker_epoch(&self) -> u64 {
        self.worker_epoch.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn snapshot(&self) -> VoiceSessionSnapshot {
        let mut state = self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let epoch = self.worker_epoch.load(std::sync::atomic::Ordering::Acquire);
        if epoch > 0 {
            state.worker_epoch = Some(epoch);
        }
        state
    }

    pub fn update_task(&self, task: Option<nomifun_api_types::voice::VoiceTaskSnapshot>) {
        self.update_task_versioned(task, None, None);
    }

    pub fn update_task_versioned(
        &self,
        task: Option<nomifun_api_types::voice::VoiceTaskSnapshot>,
        instance_id: Option<&str>,
        seq: Option<u64>,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        let epoch = self.worker_epoch.load(std::sync::atomic::Ordering::Acquire);
        if epoch > 0 {
            state.worker_epoch = Some(epoch);
        }
        if let Some(inst) = instance_id {
            if !state.instance_id.is_empty() && inst != state.instance_id {
                return false;
            }
        }
        if let Some(s) = seq {
            let last = self.last_task_seq.load(std::sync::atomic::Ordering::SeqCst);
            if s < last {
                return false;
            }
            self.last_task_seq.store(s, std::sync::atomic::Ordering::SeqCst);
        }
        if let (Some(current_task), Some(new_task)) = (&state.task, &task) {
            if current_task.command_id == new_task.command_id {
                let current_is_terminal = matches!(
                    current_task.status,
                    nomifun_api_types::voice::VoiceTaskStatus::Completed
                        | nomifun_api_types::voice::VoiceTaskStatus::Failed
                );
                let new_is_terminal = matches!(
                    new_task.status,
                    nomifun_api_types::voice::VoiceTaskStatus::Completed
                        | nomifun_api_types::voice::VoiceTaskStatus::Failed
                );
                if current_is_terminal && !new_is_terminal {
                    return false;
                }
            }
        }
        state.task = task;
        state.seq += 1;
        true
    }

    pub fn send(
        &self,
        event: VoiceCompanionEvent,
    ) -> Result<usize, broadcast::error::SendError<VoiceCompanionEvent>> {
        self.send_inner(event, None)
    }

    pub(crate) fn send_scoped(
        &self,
        event: VoiceCompanionEvent,
        scope: &VoiceSessionControlRequest,
    ) -> Result<usize, broadcast::error::SendError<VoiceCompanionEvent>> {
        self.send_inner(event, Some(scope))
    }

    fn send_inner(
        &self,
        event: VoiceCompanionEvent,
        scope: Option<&VoiceSessionControlRequest>,
    ) -> Result<usize, broadcast::error::SendError<VoiceCompanionEvent>> {
        use VoiceSessionPhase as Phase;
        if matches!(event, VoiceCompanionEvent::Snapshot { .. }) {
            return self.tx.send(event);
        }
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if scope.is_some_and(|scope| !scope.matches(&state) || state.ended_reason.is_some()) {
            return Ok(0);
        }
        let epoch = self.worker_epoch.load(std::sync::atomic::Ordering::Acquire);
        if epoch > 0 {
            state.worker_epoch = Some(epoch);
        }
        if let VoiceCompanionEvent::TtsSource {session_id, turn_id, ..} = &event {
            if state.session_id != *session_id || state.turn_id != *turn_id || state.ended_reason.is_some() {
                return Ok(0);
            }
        }
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
            let output_interrupted = matches!(event, VoiceCompanionEvent::SessionEnded { .. })
                && matches!(state.phase, Phase::Speaking | Phase::Synthesizing);
            if let Some(turn) = state.conversation.last_mut() {
                if turn.reply_state == Some(nomifun_api_types::voice::VoiceReplyState::Streaming)
                    || (output_interrupted && turn.assistant_text.is_some())
                {
                    turn.reply_state = Some(nomifun_api_types::voice::VoiceReplyState::Interrupted);
                }
            }
        }
        match &event {
            VoiceCompanionEvent::TtsSource {diagnostic, ..} => { state.tts = Some(diagnostic.clone()); }
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
            VoiceCompanionEvent::WakeHit { request_id, locale, .. } => {
                self.context
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                state.conversation.clear();
                state.ended_reason = None;
                state.call_request_id = request_id.clone();
                state.locale = locale.clone();
                state.feedback = None;
                state.tts = None;
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
                if matches!(state.phase, Phase::Speaking | Phase::Synthesizing) {
                    state.phase = Phase::Listening;
                    if let Some(turn) = current_turn_mut(&mut state) {
                        if turn.assistant_text.is_some() {
                            turn.reply_state = Some(nomifun_api_types::voice::VoiceReplyState::Interrupted);
                        }
                    }
                }
                state.listening_deadline_ms = None;
                state.follow_up_deadline_ms = None;
            }
            VoiceCompanionEvent::Transcribing => {
                if state.ended_reason.is_none() {
                    state.ended_reason = None;
                    state.listening_deadline_ms = None;
                    state.error = None;
                    state.phase = Phase::Transcribing;
                    state.turn_id = Some(nomifun_common::generate_id());
                    let turn_id = state.turn_id.clone().unwrap();
                    let session_id = state.session_id.clone();
                    state.conversation.push(VoiceConversationTurn {
                        turn_id,
                        session_id,
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
            }
            VoiceCompanionEvent::SpeechRecognized { text } => {
                state.user_text = Some(text.clone());
                if let Some(turn) = current_turn_mut(&mut state) {
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
                if intent == "Ignore" {
                    let ignored_turn = state.turn_id.clone();
                    state.conversation.retain(|turn| Some(&turn.turn_id) != ignored_turn.as_ref());
                    state.user_text = None;
                    state.assistant_text = None;
                    state.understood_request = None;
                    state.error = None;
                    state.phase = Phase::Listening;
                } else {
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
                    if let Some(turn) = current_turn_mut(&mut state) {
                        turn.understood_request = understood_request.clone();
                    }
                    state.assistant_text = reply.clone();
                    if let Some(turn) = current_turn_mut(&mut state) {
                        turn.assistant_text = reply.clone();
                        turn.reply_state=Some(nomifun_api_types::voice::VoiceReplyState::Complete);
                    }
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
                state.turn_id = Some(turn_id);
                state.user_text = None;
                state.understood_request = None;
                state.assistant_text = Some(text.clone());
                state.error = None;
                state.ended_reason = None;
                // Synthetic notice is NOT a chat row; do NOT push to state.conversation
                state.phase = Phase::Synthesizing;
            }
            VoiceCompanionEvent::ReplySentence {text,understood_request}=>{
                if let Some(understood)=understood_request {state.understood_request=Some(understood.clone());}
                state.assistant_text.get_or_insert_with(String::new).push_str(text);
                if let Some(turn)=current_turn_mut(&mut state){
                    turn.assistant_text.get_or_insert_with(String::new).push_str(text);
                    turn.reply_state=Some(nomifun_api_types::voice::VoiceReplyState::Streaming);
                    if let Some(understood)=understood_request {turn.understood_request=Some(understood.clone());}
                }
            }
            VoiceCompanionEvent::ResponseReady {text} => {
                state.assistant_text=Some(text.clone());
                if let Some(turn)=current_turn_mut(&mut state) {turn.assistant_text=Some(text.clone());}
            }
            VoiceCompanionEvent::ConfirmationPrompted {approval_id,text,..}=>{
                let turn_id=format!("approval-{approval_id}");
                state.session_id.get_or_insert_with(nomifun_common::generate_id);
                state.turn_id=Some(turn_id);
                state.user_text=None;
                state.understood_request=None;
                state.assistant_text=Some(text.clone());
                state.ended_reason=None;
                state.error=None;
                state.listening_deadline_ms=None;
                state.follow_up_deadline_ms=None;
                // Synthetic notice is NOT a chat row; do NOT push to state.conversation
                state.phase=Phase::AwaitingConfirmation;
            }
            VoiceCompanionEvent::ConfirmationListening {deadline_ms}=>{
                state.phase=Phase::ListeningConfirm;
                state.listening_deadline_ms=Some(*deadline_ms);
            }
            VoiceCompanionEvent::ConfirmationHeard {text}=>{
                state.user_text=Some(text.clone());
                if let Some(turn)=current_turn_mut(&mut state){turn.user_text=Some(text.clone());}
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
                let has_active_turn = !matches!(state.phase, Phase::Idle | Phase::Listening);
                state.phase = Phase::Error;
                state.error = Some(VoiceSessionError {
                    stage: stage.clone(),
                    code: code.clone(),
                });
                if has_active_turn { if let Some(turn)=current_turn_mut(&mut state){turn.error=Some(VoiceSessionError{stage:stage.clone(),code:code.clone()});} }
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
    fn stale_asr_failure_cannot_change_a_new_turn_or_an_ended_session() {
        let hub = VoiceEventHub::new();
        let _listener = hub.sender().subscribe();
        let _ = hub.send(VoiceCompanionEvent::WakeHit { keyword: "Hey Flowy".into(), request_id: None, locale: None });
        let _ = hub.send(VoiceCompanionEvent::Transcribing);
        let first = hub.snapshot();
        let old = VoiceSessionControlRequest { instance_id: first.instance_id, session_id: first.session_id, turn_id: first.turn_id };
        let _ = hub.send(VoiceCompanionEvent::Transcribing);
        let current = hub.snapshot();
        assert_ne!(current.turn_id, old.turn_id);
        assert_eq!(hub.send_scoped(VoiceCompanionEvent::SpeechRecognized { text: "stale".into() }, &old).unwrap(), 0);
        assert_eq!(hub.send_scoped(VoiceCompanionEvent::Error { stage: "asr".into(), code: "asr_failed".into() }, &old).unwrap(), 0);
        let after = hub.snapshot();
        assert_eq!(after.seq, current.seq);
        assert_eq!(after.turn_id, current.turn_id);
        assert!(after.error.is_none());
        let scope = VoiceSessionControlRequest { instance_id: after.instance_id, session_id: after.session_id, turn_id: after.turn_id };
        let _ = hub.send(VoiceCompanionEvent::SessionEnded { reason: nomifun_api_types::voice::VoiceSessionEndReason::UserStop });
        let ended = hub.snapshot();
        assert_eq!(hub.send_scoped(VoiceCompanionEvent::Error { stage: "asr".into(), code: "asr_failed".into() }, &scope).unwrap(), 0);
        assert_eq!(hub.snapshot().seq, ended.seq);
    }

    #[test]
    fn wake_identity_is_published_atomically_and_not_inherited_by_kws() {
        let hub=VoiceEventHub::new();
        let _=hub.send(VoiceCompanionEvent::WakeHit {keyword:"Hey Flowy".into(),request_id:Some("start-p".into()),locale:None});
        let first=hub.snapshot();
        assert_eq!(first.call_request_id.as_deref(),Some("start-p"));
        let _=hub.send(VoiceCompanionEvent::WakeHit {keyword:"Hey Flowy".into(),request_id:None,locale:None});
        assert_ne!(hub.snapshot().session_id,first.session_id);
        assert!(hub.snapshot().call_request_id.is_none());
    }

    #[test]
    fn ignored_input_is_removed_without_ending_or_erasing_valid_context() {
        let hub = VoiceEventHub::new();
        let mut events = hub.sender().subscribe();
        hub.send(VoiceCompanionEvent::WakeHit { keyword: "Hey Flowy".into(), request_id: None, locale: None }).unwrap();
        let session = hub.snapshot().session_id;
        hub.send(VoiceCompanionEvent::Transcribing).unwrap();
        hub.send(VoiceCompanionEvent::SpeechRecognized { text: "深圳，不是北京".into() }).unwrap();
        hub.send(VoiceCompanionEvent::IntentDecided { intent: "Chat".into(), reply: Some("记住了。".into()), understood_request: None }).unwrap();
        let context = hub.context();
        hub.send(VoiceCompanionEvent::Transcribing).unwrap();
        hub.send(VoiceCompanionEvent::SpeechRecognized { text: "杂音".into() }).unwrap();
        hub.send(VoiceCompanionEvent::ModelResponse { text: r#"{"intent_type":"ignore"}"#.into() }).unwrap();
        hub.send(VoiceCompanionEvent::IntentDecided { intent: "Ignore".into(), reply: None, understood_request: None }).unwrap();
        let snapshot = hub.snapshot();
        assert_eq!(snapshot.session_id, session);
        assert_eq!(snapshot.phase, VoiceSessionPhase::Listening);
        assert!(snapshot.ended_reason.is_none() && snapshot.user_text.is_none());
        assert_eq!(snapshot.conversation.len(), 1);
        assert_eq!(hub.context().iter().map(|message|&message.content).collect::<Vec<_>>(), context.iter().map(|message|&message.content).collect::<Vec<_>>());
        let mut published = false;
        while let Ok(event) = events.try_recv() {
            if let VoiceCompanionEvent::Snapshot { snapshot } = event {
                published |= snapshot.phase == VoiceSessionPhase::Listening && snapshot.conversation.len() == 1 && snapshot.user_text.is_none();
            }
        }
        assert!(published, "ignored cleanup must be published to every view");
    }

    #[test]
    fn partial_reply_and_its_error_stay_bound_to_the_failed_turn() {
        let hub=VoiceEventHub::new();let _listener=hub.sender().subscribe();
        hub.send(VoiceCompanionEvent::WakeHit{keyword:"Hey Flowy".into(),request_id:None,locale:None}).unwrap();
        hub.send(VoiceCompanionEvent::Transcribing).unwrap();
        hub.send(VoiceCompanionEvent::SpeechRecognized{text:"问题".into()}).unwrap();
        hub.send(VoiceCompanionEvent::ReplySentence{text:"第一句。".into(),understood_request:Some("需求".into())}).unwrap();
        assert!(hub.context().is_empty(),"partial generation must not enter completed context");
        assert_eq!(hub.snapshot().conversation[0].reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Streaming));
        hub.send(VoiceCompanionEvent::Error{stage:"dialogue".into(),code:"model_timeout".into()}).unwrap();
        hub.send(VoiceCompanionEvent::SpeechStarted).unwrap();
        let snap=hub.snapshot();
        assert_eq!(snap.conversation[0].reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Interrupted));
        assert_eq!(snap.conversation[0].error.as_ref().unwrap().code,"model_timeout");
        assert_eq!(snap.conversation[0].assistant_text.as_deref(),Some("第一句。"));
        hub.send(VoiceCompanionEvent::WakeHit{keyword:"Hey Flowy".into(),request_id:None,locale:None}).unwrap();
        assert!(hub.snapshot().conversation.is_empty());
        assert!(hub.snapshot().error.is_none());
    }

    #[test]
    fn notices_do_not_overwrite_real_turn_or_replace_active_session() {
        let hub = VoiceEventHub::new();
        let _listener = hub.sender().subscribe();
        hub.send(VoiceCompanionEvent::WakeHit { keyword: "Hey Flowy".into(), request_id: None, locale: None }).unwrap();
        hub.send(VoiceCompanionEvent::Transcribing).unwrap();
        hub.send(VoiceCompanionEvent::SpeechRecognized { text: "真实问题".into() }).unwrap();
        hub.send(VoiceCompanionEvent::IntentDecided { intent: "Chat".into(), reply: Some("真实回答".into()), understood_request: None }).unwrap();
        let original = hub.snapshot();
        assert_eq!(original.conversation[0].session_id, original.session_id);
        hub.send(VoiceCompanionEvent::ConfirmationPrompted { approval_id: "approval".into(), text: "授权提示".into(), expires_at_ms: i64::MAX }).unwrap();
        hub.send(VoiceCompanionEvent::ConfirmationHeard { text: "同意".into() }).unwrap();
        hub.send(VoiceCompanionEvent::ResponseReady { text: "提示播报".into() }).unwrap();
        let after = hub.snapshot();
        assert_eq!(after.session_id, original.session_id);
        assert_eq!(after.conversation.len(), 1);
        assert_eq!(after.conversation[0].user_text, original.conversation[0].user_text);
        assert_eq!(after.conversation[0].assistant_text, original.conversation[0].assistant_text);
        hub.send(VoiceCompanionEvent::WakeHit { keyword: "Hey Flowy".into(), request_id: None, locale: None }).unwrap();
        assert!(hub.snapshot().conversation.is_empty(), "KWS must clear volatile old turns too");
    }

    #[test]
    fn completed_text_keeps_an_interrupted_audio_marker_on_speech_and_stop() {
        for end in [
            VoiceCompanionEvent::SpeechStarted,
            VoiceCompanionEvent::SessionEnded { reason: nomifun_api_types::voice::VoiceSessionEndReason::UserStop },
        ] {
            let hub = VoiceEventHub::new();
            let _listener = hub.sender().subscribe();
            hub.send(VoiceCompanionEvent::Transcribing).unwrap();
            hub.send(VoiceCompanionEvent::SpeechRecognized { text: "问题".into() }).unwrap();
            hub.send(VoiceCompanionEvent::IntentDecided { intent: "Chat".into(), reply: Some("完整生成但尚未播完。".into()), understood_request: None }).unwrap();
            hub.send(VoiceCompanionEvent::TtsSpeaking { text: "完整生成但尚未播完。".into() }).unwrap();
            hub.send(end).unwrap();
            let turn = hub.snapshot().conversation.pop().unwrap();
            assert_eq!(turn.reply_state, Some(nomifun_api_types::voice::VoiceReplyState::Interrupted));
            assert_eq!(turn.assistant_text.as_deref(), Some("完整生成但尚未播完。"));
        }
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
                request_id: None,
                keyword: "Hey Flowy".into(),
                locale: None,
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

    #[test]
    fn worker_epoch_is_preserved_on_ordinary_events_and_snapshots() {
        let hub = VoiceEventHub::new();
        assert_eq!(hub.worker_epoch(), 0);
        assert_eq!(hub.snapshot().worker_epoch, None);

        hub.set_worker_epoch(42);
        assert_eq!(hub.worker_epoch(), 42);
        assert_eq!(hub.snapshot().worker_epoch, Some(42));

        let _listener = hub.sender().subscribe();
        hub.send(VoiceCompanionEvent::Transcribing).unwrap();
        let snap = hub.snapshot();
        assert_eq!(snap.worker_epoch, Some(42));
    }
}

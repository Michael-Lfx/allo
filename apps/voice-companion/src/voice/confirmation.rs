//! Presents one actual runtime approval. The host remains the approval authority.
use super::*;
use nomifun_api_types::voice::{VoiceToolConfirmationPrompt,VoiceToolConfirmationReply};
use std::time::Duration;

pub(super) struct PendingConfirmation {
    pub approval_id:String,
    pub generation:u64,
    pub deadline_ms:i64,
    pub response:Option<tokio::sync::oneshot::Sender<Option<String>>>,
}

impl VoiceCompanionRuntime {
    pub async fn request_tool_confirmation(&self,request:VoiceToolConfirmationPrompt)->Result<VoiceToolConfirmationReply,&'static str> {
        if request.text.trim().is_empty() || request.text.len()>4096 || [&request.approval_id,&request.msg_id,&request.call_id].iter().any(|id|id.is_empty()||id.len()>128)
            || request.task.conversation_id.is_none() || request.task.message_id.is_none() {return Err("confirmation_invalid");}
        if request.expires_at_ms<=chrono::Utc::now().timestamp_millis(){return Err("confirmation_expired");}
        let mut driver=self.driver.lock().await;
        let snapshot=self.snapshot();
        if !request.scope.matches(&snapshot){return Err("task_scope_changed");}
        if driver.current_state()!=AcousticState::Idle || snapshot.ended_reason==Some(VoiceSessionEndReason::UserStop)
            || snapshot.audio.as_ref().is_some_and(|audio|audio.speech_detected)
            || self.pending_confirmation.lock().unwrap_or_else(|error|error.into_inner()).is_some(){return Err("voice_busy");}
        if !self.capture_available(){return Err("microphone_unavailable");}
        let generation=self.playback.generation_id();
        driver.handle_event(AcousticEvent::ToolPolicyIntercept).map_err(|_|"voice_busy")?;
        *self.pending_confirmation.lock().unwrap_or_else(|error|error.into_inner())=Some(PendingConfirmation {
            approval_id:request.approval_id.clone(),generation,deadline_ms:request.expires_at_ms,response:None,
        });
        let _=self.event_tx.send(VoiceCompanionEvent::ConfirmationPrompted {approval_id:request.approval_id.clone(),text:request.text.clone(),expires_at_ms:request.expires_at_ms});
        drop(driver);
        let budget=Duration::from_millis(request.expires_at_ms.saturating_sub(chrono::Utc::now().timestamp_millis()).max(1) as u64);
        let outcome=tokio::time::timeout(budget,async {
            tokio::select! {
                result=self.play_prompt_and_listen(&request,generation)=>result,
                _=async {while self.playback.generation_id()==generation {tokio::time::sleep(Duration::from_millis(20)).await;}}=>Ok(false),
            }
        }).await;
        let approved=matches!(outcome,Ok(Ok(true))) && self.playback.generation_id()==generation
            && chrono::Utc::now().timestamp_millis()<request.expires_at_ms;
        let owned={
            let mut pending=self.pending_confirmation.lock().unwrap_or_else(|error|error.into_inner());
            if pending.as_ref().is_some_and(|pending|pending.approval_id==request.approval_id){pending.take();true}else{false}
        };
        let mut driver=self.driver.lock().await;
        if owned && self.playback.generation_id()==generation {
            if let Some(turn)=self.active_turn.lock().unwrap_or_else(|error|error.into_inner()).take(){turn.abort();}
            driver.reset();
            if !matches!(outcome,Ok(Ok(_))){
                let _=self.playback.interrupt().await;
                let _=self.event_tx.send(VoiceCompanionEvent::Error {stage:"confirmation".into(),code:"confirmation_audio_or_timeout".into()});
            }
            let text=if approved {"已提交本次确认，等待执行侧校验。"}else{"未获得本次确认，本次语音答复为拒绝。"};
            let _=self.event_tx.send(VoiceCompanionEvent::ResponseReady {text:text.into()});
            let _=self.event_tx.send(VoiceCompanionEvent::Idle);
        }
        Ok(VoiceToolConfirmationReply {approval_id:request.approval_id,msg_id:request.msg_id,call_id:request.call_id,approved})
    }

    async fn play_prompt_and_listen(&self,request:&VoiceToolConfirmationPrompt,generation:u64)->Result<bool,AudioSinkError> {
        self.playback.speak_text_with_started(self.tts.as_ref(),&request.text,"",||{}).await?;
        let mut driver=self.driver.lock().await;
        if self.playback.generation_id()!=generation || driver.current_state()!=AcousticState::AwaitingConfirmation{return Ok(false);}
        let total_remaining=request.expires_at_ms.saturating_sub(chrono::Utc::now().timestamp_millis()).min(8000);
        if total_remaining<=0{return Ok(false);}
        let deadline_ms=chrono::Utc::now().timestamp_millis()+total_remaining;
        driver.handle_event(AcousticEvent::PromptDone).map_err(|_|AudioSinkError::Write("confirmation_scope_changed".into()))?;
        driver.limit_confirmation_window(Duration::from_millis(total_remaining as u64));
        drop(driver);

        let mut round = 0;
        loop {
            let now_ms = chrono::Utc::now().timestamp_millis();
            let remaining = deadline_ms.saturating_sub(now_ms);
            if remaining <= 0 || self.playback.generation_id() != generation {
                return Ok(false);
            }

            let (tx, rx) = tokio::sync::oneshot::channel();
            {
                let mut pending = self.pending_confirmation.lock().unwrap_or_else(|error| error.into_inner());
                let Some(pending) = pending.as_mut().filter(|pending| pending.approval_id == request.approval_id) else {
                    return Ok(false);
                };
                pending.deadline_ms = deadline_ms;
                pending.response = Some(tx);
            }
            let _ = self.event_tx.send(VoiceCompanionEvent::ConfirmationListening { deadline_ms });

            let answer = tokio::time::timeout(Duration::from_millis(remaining as u64), rx).await.ok().and_then(Result::ok).flatten();
            if self.playback.generation_id() != generation || chrono::Utc::now().timestamp_millis() >= deadline_ms {
                return Ok(false);
            }

            let answer = answer.unwrap_or_default();
            match evaluate_confirmation_text(&answer) {
                ConfirmationDecision::Approved => return Ok(true),
                ConfirmationDecision::Denied => return Ok(false),
                ConfirmationDecision::Ambiguous => {
                    round += 1;
                    let remaining_after = deadline_ms.saturating_sub(chrono::Utc::now().timestamp_millis());
                    if round < 2 && remaining_after > 2500 {
                        let _ = self.playback.speak_text_with_started(self.tts.as_ref(), "没听清，请说可以执行或不要执行", "", || {}).await;
                        continue;
                    }
                    return Ok(false);
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmationDecision {
    Approved,
    Denied,
    Ambiguous,
}

pub fn evaluate_confirmation_text(answer: &str) -> ConfirmationDecision {
    let clean = answer.trim().trim_end_matches(['。', '！', '!', '.', '？', '?', '，', ',']).to_lowercase();
    if clean.is_empty() {
        return ConfirmationDecision::Ambiguous;
    }
    // Negation has highest priority!
    const DENIAL_KEYWORDS: &[&str] = &[
        "不同意", "不要执行", "不要", "先别", "别执行", "别删", "别写", "别动",
        "取消", "不行", "拒绝", "不确认", "不许可", "否", "还没", "没有确认", "还没确认", "未确认",
        "no", "cancel", "deny", "stop"
    ];
    if DENIAL_KEYWORDS.iter().any(|k| clean.contains(k)) {
        return ConfirmationDecision::Denied;
    }
    // Consent keywords
    const CONSENT_KEYWORDS: &[&str] = &[
        "确认", "同意", "确认执行", "可以执行", "执行吧", "允许", "好", "好的",
        "可以", "行", "执行", "准许", "confirm", "yes", "approve", "ok", "proceed"
    ];
    if CONSENT_KEYWORDS.iter().any(|k| clean.contains(k)) {
        return ConfirmationDecision::Approved;
    }
    ConfirmationDecision::Ambiguous
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confirmation_decision_semantics() {
        assert_eq!(evaluate_confirmation_text("可以执行"), ConfirmationDecision::Approved);
        assert_eq!(evaluate_confirmation_text("好的"), ConfirmationDecision::Approved);
        assert_eq!(evaluate_confirmation_text("允许"), ConfirmationDecision::Approved);
        assert_eq!(evaluate_confirmation_text("确认执行。"), ConfirmationDecision::Approved);
        assert_eq!(evaluate_confirmation_text("yes!"), ConfirmationDecision::Approved);

        // Negation has priority
        assert_eq!(evaluate_confirmation_text("不同意"), ConfirmationDecision::Denied);
        assert_eq!(evaluate_confirmation_text("先别执行"), ConfirmationDecision::Denied);
        assert_eq!(evaluate_confirmation_text("不要"), ConfirmationDecision::Denied);
        assert_eq!(evaluate_confirmation_text("取消"), ConfirmationDecision::Denied);

        // Ambiguous / noise / misrecognized
        assert_eq!(evaluate_confirmation_text("确aren"), ConfirmationDecision::Ambiguous);
        assert_eq!(evaluate_confirmation_text("今天天气怎么样"), ConfirmationDecision::Ambiguous);
        assert_eq!(evaluate_confirmation_text(""), ConfirmationDecision::Ambiguous);
    }
}

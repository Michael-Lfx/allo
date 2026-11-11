use super::*;

#[derive(Clone, Debug)]
pub(super) struct AnnouncementRecord {
    pub(super) command_id: String,
    pub(super) notification_id: String,
    pub(super) attempt_id: String,
    pub(super) scope: nomifun_api_types::voice::VoiceSessionControlRequest,
    pub(super) text: String,
    pub(super) status: String,
    pub(super) reason_code: Option<String>,
}

impl VoiceCompanionRuntime {
    pub async fn announce_task_result(&self, request: nomifun_api_types::voice::VoiceTaskResultAnnouncement) -> Result<(), &'static str> {
        if request.text.trim().is_empty() || request.text.len()>16_384 || request.command_id.is_empty() || request.command_id.len()>128 {return Err("task_result_invalid");}
        let notification_id = request.notification_id.clone().unwrap_or_else(|| request.command_id.clone());
        let attempt = request.attempt_id.clone().unwrap_or_else(|| notification_id.clone());
        let mut announced = self.announced_tasks.lock().await;

        // 1. Exact attempt deduplication
        if let Some(existing) = announced.iter().find(|r| r.attempt_id == attempt) {
            if existing.scope == request.scope && existing.text == request.text {
                return Ok(());
            } else {
                return Err("task_result_conflict");
            }
        }

        // 2. Logical notification checks
        if let Some(existing) = announced.iter().find(|r| r.notification_id == notification_id) {
            if existing.text != request.text {
                return Err("task_result_conflict");
            }
            if existing.status == "started" {
                return Err("voice_busy");
            }
        }

        let mut driver = self.driver.lock().await;
        let snapshot = self.snapshot();
        if !request.scope.matches(&snapshot) {return Err("task_scope_changed");}
        let active_busy = self.active_turn.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|h| !h.is_finished());
        if snapshot.ended_reason==Some(VoiceSessionEndReason::UserStop)
            || snapshot.audio.as_ref().is_some_and(|audio|audio.speech_detected)
            || !self.pending_utterances.lock().unwrap_or_else(|e|e.into_inner()).is_empty()
            || active_busy
            || !driver.yield_for_notification() {return Err("voice_busy");}
        driver.handle_event(AcousticEvent::TaskSummaryReady).map_err(|_| "voice_busy")?;
        let generation = self.playback.generation_id();
        announced.push_back(AnnouncementRecord {
            command_id: request.command_id.clone(),
            notification_id: notification_id.clone(),
            attempt_id: attempt.clone(),
            scope: request.scope.clone(),
            text: request.text.clone(),
            status: "started".into(),
            reason_code: None,
        });
        if announced.len()>64 {announced.pop_front();}
        let _ = self.event_tx.send(VoiceCompanionEvent::TaskResultReady {
            command_id: request.command_id,
            notification_id: request.notification_id,
            attempt_id: request.attempt_id,
            goal: request.goal,
            text: request.text.clone(),
        });
        let playback_snapshot = self.event_tx.snapshot();
        let playback_scope = VoiceSessionControlRequest {
            instance_id: playback_snapshot.instance_id,
            session_id: playback_snapshot.session_id,
            turn_id: playback_snapshot.turn_id,
        };
        drop(driver);
        drop(announced);
        voice_feedback::play(&self.playback, &self.event_tx, VoiceCueKind::Recognized, generation).await;
        let play_result = Self::play_response(&request.text,&self.driver,&self.playback,&self.event_tx,&self.tts,generation,&playback_scope).await;
        let mut announced = self.announced_tasks.lock().await;
        if let Some(record) = announced.iter_mut().find(|r| r.attempt_id == attempt) {
            match play_result {
                Ok(ReplyPlayback::Played) => {
                    record.status = "played".into();
                    record.reason_code = None;
                }
                Ok(ReplyPlayback::Interrupted) => {
                    record.status = "interrupted".into();
                    record.reason_code = Some("playback_interrupted".into());
                }
                Ok(ReplyPlayback::Muted) => {
                    record.status = "muted".into();
                    record.reason_code = Some("tts_muted".into());
                }
                Err(code) => {
                    record.status = "failed".into();
                    record.reason_code = Some(code.into());
                }
            }
        }
        Ok(())
    }

    pub async fn notification_status(&self, attempt: &str) -> Option<nomifun_api_types::voice::VoicePlaybackReceipt> {
        let announced = self.announced_tasks.lock().await;
        announced
            .iter()
            .rev()
            .find(|r| r.attempt_id == attempt || r.notification_id == attempt || r.command_id == attempt)
            .map(|r| nomifun_api_types::voice::VoicePlaybackReceipt {
                attempt_id: r.attempt_id.clone(),
                status: r.status.clone(),
                reason_code: r.reason_code.clone(),
            })
    }
}

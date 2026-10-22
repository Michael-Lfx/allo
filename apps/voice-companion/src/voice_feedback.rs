//! Session-scoped sound feedback. The renderer only observes these receipts.
use std::sync::Arc;

use nomifun_api_types::voice::{
    VoiceCompanionEvent, VoiceCueKind, VoiceCueStatus, VoiceFeedbackDiagnostic, VoiceFeedbackMode,
};
use nomifun_audio::EarconKind;
use nomifun_voice_terminal::playback::PlaybackPipeline;

use crate::voice_events::VoiceEventHub;

pub(crate) async fn play(
    playback: &Arc<PlaybackPipeline>,
    events: &Arc<VoiceEventHub>,
    kind: VoiceCueKind,
    generation: u64,
) -> bool {
    if playback.generation_id() != generation {
        return false;
    }
    let scope = events.snapshot();
    let mut diagnostic = VoiceFeedbackDiagnostic {
        request_id: nomifun_common::generate_id(),
        session_id: scope.session_id,
        turn_id: scope.turn_id,
        kind,
        status: VoiceCueStatus::Pending,
        reason_code: None,
        at_ms: chrono::Utc::now().timestamp_millis(),
    };
    let publish = |diagnostic: &VoiceFeedbackDiagnostic| {
        let current = events.snapshot();
        if current.session_id == diagnostic.session_id && current.turn_id == diagnostic.turn_id {
            let _ = events.send(VoiceCompanionEvent::Feedback {
                diagnostic: diagnostic.clone(),
            });
        }
    };
    publish(&diagnostic);
    let config = events.feedback();
    if config.mode == VoiceFeedbackMode::Off || config.volume == 0 {
        diagnostic.status = VoiceCueStatus::Skipped;
        diagnostic.reason_code = Some("feedback_muted".into());
        publish(&diagnostic);
        return playback.generation_id() == generation;
    }
    let asset = match kind {
        VoiceCueKind::Wake | VoiceCueKind::FollowUp | VoiceCueKind::ListeningReminder => {
            EarconKind::Wake
        }
        VoiceCueKind::Recognized | VoiceCueKind::Processing => EarconKind::Guide,
        VoiceCueKind::Sleep | VoiceCueKind::Stopped => EarconKind::Sleep,
        VoiceCueKind::Error => EarconKind::Error,
    };
    let result = playback
        .play_cue_frame_with_started(cue_frame(asset, config.volume, false), generation, || {
            let mut started = diagnostic.clone();
            started.status = VoiceCueStatus::Started;
            started.at_ms = chrono::Utc::now().timestamp_millis();
            publish(&started);
        })
        .await;
    diagnostic.at_ms = chrono::Utc::now().timestamp_millis();
    let completed = playback.generation_id() == generation && result.is_ok();
    diagnostic.status = if playback.generation_id() != generation {
        VoiceCueStatus::Cancelled
    } else if result.is_ok() {
        VoiceCueStatus::Completed
    } else {
        diagnostic.reason_code = Some(match &result {
            Err(nomifun_voice_terminal::playback::AudioSinkError::Write(code))
                if matches!(
                    code.as_str(),
                    "speaker_unavailable" | "speaker_stream_failed" | "earcon_drain_timeout"
                ) =>
            {
                code.clone()
            }
            _ => "cue_output_failed".into(),
        });
        VoiceCueStatus::Failed
    };
    tracing::info!(kind = ?kind, status = ?diagnostic.status, request_id = %diagnostic.request_id,
        "voice_feedback_finished");
    publish(&diagnostic);
    completed
}

pub(crate) fn schedule_optional(
    playback: Arc<PlaybackPipeline>,
    events: Arc<VoiceEventHub>,
    kind: VoiceCueKind,
    generation: u64,
    quiet: Arc<std::sync::atomic::AtomicBool>,
    listening_driver: Option<
        Arc<tokio::sync::Mutex<nomifun_voice_terminal::driver::VoiceSessionDriver>>,
    >,
) {
    tokio::spawn(async move {
        use std::sync::atomic::{AtomicBool, Ordering};
        let scope = events.snapshot();
        let mut diagnostic = VoiceFeedbackDiagnostic {
            request_id: nomifun_common::generate_id(),
            session_id: scope.session_id,
            turn_id: scope.turn_id,
            kind,
            status: VoiceCueStatus::Pending,
            reason_code: None,
            at_ms: chrono::Utc::now().timestamp_millis(),
        };
        let eligible = || {
            let current = events.snapshot();
            let config = events.feedback();
            config.mode == VoiceFeedbackMode::Light
                && config.volume > 0
                && current.session_id == diagnostic.session_id
                && current.turn_id == diagnostic.turn_id
                && quiet.load(Ordering::Acquire)
                && match kind {
                    VoiceCueKind::Processing => matches!(
                        current.phase,
                        nomifun_api_types::voice::VoiceSessionPhase::Transcribing
                            | nomifun_api_types::voice::VoiceSessionPhase::Deciding
                            | nomifun_api_types::voice::VoiceSessionPhase::Synthesizing
                    ),
                    VoiceCueKind::ListeningReminder => {
                        current.phase == nomifun_api_types::voice::VoiceSessionPhase::Listening
                            && current.listening_deadline_ms.is_some()
                    }
                    _ => false,
                }
        };
        if playback.generation_id() != generation || !eligible() {
            return;
        }
        let publish = |diagnostic: &VoiceFeedbackDiagnostic| {
            let current = events.snapshot();
            if current.session_id == diagnostic.session_id && current.turn_id == diagnostic.turn_id
            {
                let _ = events.send(VoiceCompanionEvent::Feedback {
                    diagnostic: diagnostic.clone(),
                });
            }
        };
        publish(&diagnostic);
        let asset = if kind == VoiceCueKind::Processing {
            EarconKind::Guide
        } else {
            EarconKind::Wake
        };
        let (mut pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&cue_frame(
            asset,
            events.feedback().volume,
            true,
        ));
        if kind == VoiceCueKind::Processing {
            let second = pcm.clone();
            pcm.extend(vec![0.0; (rate / 20) as usize]);
            pcm.extend(second);
        }
        let started = AtomicBool::new(false);
        let started_at = std::sync::Mutex::new(None::<std::time::Instant>);
        let result = playback
            .play_optional_frame(
                nomifun_audio::pcm_to_wav(&pcm, rate),
                generation,
                eligible,
                || {
                    started.store(true, Ordering::Release);
                    *started_at.lock().unwrap() = Some(std::time::Instant::now());
                    let mut record = diagnostic.clone();
                    record.status = VoiceCueStatus::Started;
                    record.at_ms = chrono::Utc::now().timestamp_millis();
                    publish(&record);
                },
            )
            .await;
        let occupied = started_at.lock().unwrap().map(|instant| instant.elapsed());
        if playback.generation_id() == generation && kind == VoiceCueKind::ListeningReminder {
            if let (Some(driver), Some(elapsed)) = (listening_driver, occupied) {
                let mut driver = driver.lock().await;
                if playback.generation_id() == generation {
                    if let Some(remaining) = driver.compensate_listening_feedback(elapsed) {
                        let _ = events.send(VoiceCompanionEvent::ListeningDeadlineChanged {
                            remaining_ms: remaining.as_millis() as u64,
                        });
                    }
                }
            }
        }
        diagnostic.status = if playback.generation_id() != generation {
            VoiceCueStatus::Cancelled
        } else {
            match result {
                Ok(true) => VoiceCueStatus::Completed,
                Ok(false) if started.load(Ordering::Acquire) => VoiceCueStatus::Cancelled,
                Ok(false) => VoiceCueStatus::Skipped,
                Err(_) => VoiceCueStatus::Failed,
            }
        };
        diagnostic.reason_code = match diagnostic.status {
            VoiceCueStatus::Cancelled | VoiceCueStatus::Skipped => {
                Some("cue_preempted_or_state_changed".into())
            }
            VoiceCueStatus::Failed => Some("cue_output_failed".into()),
            _ => None,
        };
        diagnostic.at_ms = chrono::Utc::now().timestamp_millis();
        tracing::info!(kind = ?kind, status = ?diagnostic.status, request_id = %diagnostic.request_id,
            "voice_feedback_finished");
        publish(&diagnostic);
    });
}

fn cue_frame(asset: EarconKind, volume: u8, optional: bool) -> Vec<u8> {
    let (mut pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(asset.as_wav_bytes());
    let gain = volume as f32 / 100.0 * if optional { 0.5 } else { 1.0 };
    for sample in &mut pcm {
        *sample *= gain;
    }
    nomifun_audio::pcm_to_wav(&pcm, rate)
}

pub(crate) fn preview_frame(kind: VoiceCueKind, volume: u8) -> Vec<u8> {
    let asset = match kind {
        VoiceCueKind::Wake | VoiceCueKind::FollowUp | VoiceCueKind::ListeningReminder => {
            EarconKind::Wake
        }
        VoiceCueKind::Recognized | VoiceCueKind::Processing => EarconKind::Guide,
        VoiceCueKind::Sleep | VoiceCueKind::Stopped => EarconKind::Sleep,
        VoiceCueKind::Error => EarconKind::Error,
    };
    let frame = cue_frame(
        asset,
        volume,
        matches!(
            kind,
            VoiceCueKind::Processing | VoiceCueKind::ListeningReminder
        ),
    );
    if kind != VoiceCueKind::Processing {
        return frame;
    }
    let (mut pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&frame);
    let second = pcm.clone();
    pcm.extend(vec![0.0; (rate / 20) as usize]);
    pcm.extend(second);
    nomifun_audio::pcm_to_wav(&pcm, rate)
}

pub(crate) fn schedule(
    playback: Arc<PlaybackPipeline>,
    events: Arc<VoiceEventHub>,
    kind: VoiceCueKind,
    generation: u64,
) {
    tokio::spawn(async move {
        play(&playback, &events, kind, generation).await;
    });
}

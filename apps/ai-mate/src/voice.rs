//! AI Mate Voice Companion Runtime & Pipeline Assembly
//!
//! Ties together:
//! - `nomifun_audio::voice_pipeline::VoiceCapturePipeline`
//! - `nomifun_voice_terminal::playback::PlaybackPipeline`
//! - `nomifun_voice_terminal::driver::VoiceSessionDriver`
//! - `nomifun_voice_terminal::router::QwenVoiceRouter`
//! - `nomifun_voice_models::ModelManager`
//! - `crate::oobe::OobeManager`

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;
use tracing::{info, warn};

use nomifun_audio::EarconKind;
use nomifun_audio::capture::CaptureReadResult;
use nomifun_audio::kws::KwsEngine;
#[cfg(any(test, feature = "voice-skeleton"))]
use nomifun_audio::vad::{EnergyVad, VadConfig};
use nomifun_audio::vad::{SherpaVad, VadBackend};
use nomifun_audio::voice_pipeline::{PipelineConfig, VoiceCapturePipeline};
use nomifun_audio::{frame::AudioChannel, recorder::SttCallback};
use nomifun_voice_models::ModelManager;
use nomifun_voice_terminal::contract::{AcousticEvent, AcousticState, IntentType};
use nomifun_voice_terminal::driver::{VoiceRouter, VoiceSessionDriver};
#[cfg(any(test, feature = "voice-skeleton"))]
use nomifun_voice_terminal::playback::ChannelAudioSink;
use nomifun_voice_terminal::playback::{AudioSinkError, PhysicalAudioSink, PlaybackPipeline};
use nomifun_voice_terminal::router::QwenVoiceRouter;
use nomifun_voice_terminal::tts::{EdgeTtsProvider, TtsProvider};

use crate::oobe::OobeManager;
use crate::voice_events::VoiceEventHub;
use crate::voice_feedback;
mod confirmation;
use confirmation::PendingConfirmation;
pub use nomifun_api_types::voice::VoiceCompanionEvent;
use nomifun_api_types::voice::{
    VoiceAudioDiagnostics, VoiceCueKind, VoiceDecision, VoiceSessionControlRequest,
    VoiceSessionEndReason, VoiceSessionSnapshot,
};

#[derive(Debug, thiserror::Error)]
pub enum VoiceWakeError {
    #[error("pairing_required")]
    PairingRequired,
    #[error("models_unavailable")]
    ModelsUnavailable,
    #[error("microphone_unavailable")]
    MicrophoneUnavailable,
    #[error("session_busy")]
    Busy,
}

#[derive(Clone, Debug)]
struct AnnouncementRecord {
    command_id: String,
    notification_id: String,
    attempt_id: String,
    scope: nomifun_api_types::voice::VoiceSessionControlRequest,
    text: String,
    status: String,
    reason_code: Option<String>,
}

struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

fn detect_task_control(input: &str) -> Option<nomifun_api_types::voice::VoiceTaskControlAction> {
    use nomifun_api_types::voice::VoiceTaskControlAction;
    let s = input.trim();
    if s.starts_with("读取")
        || s.starts_with("创建")
        || s.starts_with("分析")
        || s.starts_with("扫描")
        || s.starts_with("整理")
        || s.starts_with("备份")
    {
        return None;
    }
    let has_negated_cancel = s.contains("不要取消")
        || s.contains("别取消")
        || s.contains("不用取消")
        || s.contains("不能取消")
        || s.contains("不取消")
        || s.contains("请勿取消")
        || s.contains("不要停止")
        || s.contains("别停止")
        || s.contains("不用停止")
        || s.contains("不要结束")
        || s.contains("别结束");

    let is_query = s.contains("任务进度")
        || s.contains("任务状态")
        || s.contains("查询进度")
        || s.contains("查询状态")
        || s.contains("查询任务")
        || s.contains("进度")
        || s.contains("状态")
        || s.contains("重听")
        || s.contains("再播一遍")
        || s.contains("再说一遍")
        || s.contains("刚才说了什么")
        || s.contains("再读一遍");

    if has_negated_cancel {
        if is_query {
            return Some(VoiceTaskControlAction::QueryStatus);
        }
        return None;
    }

    let is_cancel = s.contains("取消任务")
        || s.contains("停止任务")
        || s == "取消"
        || s == "停止"
        || s == "结束任务";

    if is_cancel {
        if is_query {
            return None;
        }
        return Some(VoiceTaskControlAction::CancelTask);
    }

    if is_query {
        Some(VoiceTaskControlAction::QueryStatus)
    } else {
        None
    }
}

/// Production voice companion pipeline assembly and runtime controller.
pub struct VoiceCompanionRuntime {
    pending_confirmation:Arc<std::sync::Mutex<Option<PendingConfirmation>>>,
    announced_tasks: Mutex<std::collections::VecDeque<AnnouncementRecord>>,
    task_client: Option<Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>>,
    model_manager: Arc<ModelManager>,
    oobe_manager: Arc<OobeManager>,
    driver: Arc<Mutex<VoiceSessionDriver>>,
    playback: Arc<PlaybackPipeline>,
    tts: Arc<dyn TtsProvider>,
    router: Arc<dyn VoiceRouter>,
    transcriber: Option<Arc<dyn SttCallback>>,
    capture_pipeline: Arc<Mutex<VoiceCapturePipeline>>,
    acoustic_ready: Arc<AtomicBool>,
    event_tx: Arc<VoiceEventHub>,
    active_turn: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Active RAII guard holding the cpal input stream open. Dropping this closes the mic.
    #[allow(dead_code)]
    mic_source: Option<Arc<nomifun_audio::MicSource>>,
    #[cfg(any(test, feature = "voice-skeleton"))]
    replay_capture: bool,
}

impl Drop for VoiceCompanionRuntime {
    fn drop(&mut self) {
        if let Some(turn) = self
            .active_turn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            turn.abort();
        }
    }
}

impl VoiceCompanionRuntime {
    /// Assembles the complete voice pipeline with capture, playback, session driver, and router.
    pub fn assemble(model_manager: Arc<ModelManager>, oobe_manager: Arc<OobeManager>) -> Self {
        let (capture_pipeline, mic_source) = match nomifun_audio::MicSource::new(16_000) {
            Ok(mic) => {
                info!("Hardware microphone successfully initialized for AI Mate");
                let pipeline = mic.pipeline();
                let mic_arc = Arc::new(mic);
                (pipeline, Some(mic_arc))
            }
            Err(err) => {
                warn!(error = %err, "No hardware microphone available; using standalone capture pipeline");
                let (pipeline, _) = VoiceCapturePipeline::new(PipelineConfig::default());
                (Arc::new(Mutex::new(pipeline)), None)
            }
        };

        let sink = Arc::new(PhysicalAudioSink::open_default());
        let playback = Arc::new(PlaybackPipeline::new(sink));

        let driver = Arc::new(Mutex::new(VoiceSessionDriver::new()));
        let router = Arc::new(QwenVoiceRouter::new());
        let event_tx = Arc::new(VoiceEventHub::new());

        Self {
            pending_confirmation:Arc::new(std::sync::Mutex::new(None)),
            announced_tasks: Mutex::new(std::collections::VecDeque::new()),
            task_client: std::env::var("AI_MATE_TASK_ENDPOINT").ok().and_then(|endpoint| {
                nomifun_voice_terminal::task_client::HostTaskClient::new(endpoint, std::env::var("AI_MATE_CONTROL_TOKEN").unwrap_or_default())
                    .map(|client| Arc::new(client) as Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>).ok()
            }),
            model_manager,
            oobe_manager,
            driver,
            playback,
            tts: Arc::new(EdgeTtsProvider::new()),
            router,
            transcriber: None,
            capture_pipeline,
            acoustic_ready: Arc::new(AtomicBool::new(false)),
            event_tx,
            active_turn: Arc::new(std::sync::Mutex::new(None)),
            mic_source,
            #[cfg(any(test, feature = "voice-skeleton"))]
            replay_capture: false,
        }
    }

    /// Assemble the same worker with an owned capture source and STT/router dependencies.
    /// Replay consumers feed the supplied pipeline rather than a disconnected producer.
    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn from_capture(
        model_manager: Arc<ModelManager>,
        oobe_manager: Arc<OobeManager>,
        capture_pipeline: Arc<Mutex<VoiceCapturePipeline>>,
        transcriber: Arc<dyn SttCallback>,
        router: Arc<dyn VoiceRouter>,
    ) -> (Self, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let event_tx = Arc::new(VoiceEventHub::new());
        (
            Self {
                pending_confirmation:Arc::new(std::sync::Mutex::new(None)),
                announced_tasks: Mutex::new(std::collections::VecDeque::new()),
                task_client: None,
                model_manager,
                oobe_manager,
                driver: Arc::new(Mutex::new(VoiceSessionDriver::new())),
                playback: Arc::new(PlaybackPipeline::new(Arc::new(ChannelAudioSink::new(tx)))),
                tts: Arc::new(nomifun_voice_terminal::tts::NullTts::new()),
                router,
                transcriber: Some(transcriber),
                capture_pipeline,
                event_tx,
                active_turn: Arc::new(std::sync::Mutex::new(None)),
                mic_source: None,
                acoustic_ready: Arc::new(AtomicBool::new(true)),
                replay_capture: true,
            },
            rx,
        )
    }

    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn with_test_playback(
        mut self,
        playback: Arc<PlaybackPipeline>,
        tts: Arc<dyn TtsProvider>,
    ) -> Self {
        self.playback = playback;
        self.tts = tts;
        self
    }

    pub fn event_sender(&self) -> tokio::sync::broadcast::Sender<VoiceCompanionEvent> {
        self.event_tx.sender()
    }

    pub fn with_tasks(mut self, client: Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>) -> Self {
        self.task_client = Some(client);
        self
    }

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
        if driver.current_state()!=AcousticState::Idle || snapshot.ended_reason==Some(VoiceSessionEndReason::UserStop)
            || snapshot.audio.as_ref().is_some_and(|audio|audio.speech_detected) {return Err("voice_busy");}
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
        drop(driver);
        drop(announced);
        voice_feedback::play(&self.playback, &self.event_tx, VoiceCueKind::Recognized, generation).await;
        let play_result = Self::play_response(&request.text,&self.driver,&self.playback,&self.event_tx,&self.tts,generation).await;
        let mut announced = self.announced_tasks.lock().await;
        if let Some(record) = announced.iter_mut().find(|r| r.attempt_id == attempt) {
            match play_result {
                Ok(()) => {
                    record.status = "played".into();
                    record.reason_code = None;
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

    pub fn snapshot(&self) -> VoiceSessionSnapshot {
        self.event_tx.snapshot()
    }

    pub fn feedback(&self) -> nomifun_api_types::voice::VoiceFeedbackConfig {
        self.event_tx.feedback()
    }

    pub fn update_feedback(
        &self,
        patch: nomifun_api_types::voice::VoiceFeedbackPatch,
    ) -> Result<(), &'static str> {
        let config = patch.apply(self.feedback());
        config.validate()?;
        self.event_tx.set_feedback(config);
        Ok(())
    }

    pub async fn preview_feedback(&self, kind: VoiceCueKind) -> Result<(), &'static str> {
        let scope = self.snapshot();
        if scope.phase != nomifun_api_types::voice::VoiceSessionPhase::Idle {
            return Err("voice_busy");
        }
        let config = self.feedback();
        if config.volume == 0 || config.mode == nomifun_api_types::voice::VoiceFeedbackMode::Off {
            return Err("feedback_muted");
        }
        let frame = voice_feedback::preview_frame(kind, config.volume);
        let generation = self.playback.generation_id();
        self.playback
            .play_optional_frame(
                frame,
                generation,
                || {
                    let now = self.snapshot();
                    now.phase == nomifun_api_types::voice::VoiceSessionPhase::Idle
                        && now.session_id == scope.session_id
                },
                || {},
            )
            .await
            .map_err(|_| "cue_output_failed")?
            .then_some(())
            .ok_or("voice_busy")
    }

    pub async fn stop_session(
        &self,
        request: &VoiceSessionControlRequest,
    ) -> Result<(), VoiceWakeError> {
        let mut driver = self.driver.lock().await;
        if !request.matches(&self.snapshot()) {
            return Err(VoiceWakeError::Busy);
        }
        driver.reset();
        self.playback
            .interrupt()
            .await
            .map_err(|_| VoiceWakeError::Busy)?;
        if let Some(turn) = self
            .active_turn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            turn.abort();
        }
        let generation = self.playback.generation_id();
        let _ = self.event_tx.send(VoiceCompanionEvent::SessionEnded {
            reason: VoiceSessionEndReason::UserStop,
        });
        drop(driver);
        voice_feedback::schedule(
            self.playback.clone(),
            self.event_tx.clone(),
            VoiceCueKind::Stopped,
            generation,
        );
        Ok(())
    }

    pub fn driver(&self) -> Arc<Mutex<VoiceSessionDriver>> {
        Arc::clone(&self.driver)
    }

    pub fn playback(&self) -> Arc<PlaybackPipeline> {
        Arc::clone(&self.playback)
    }

    pub fn router(&self) -> Arc<dyn VoiceRouter> {
        Arc::clone(&self.router)
    }

    pub fn capture_pipeline(&self) -> Arc<Mutex<VoiceCapturePipeline>> {
        Arc::clone(&self.capture_pipeline)
    }

    /// Check if device is paired and models are ready.
    pub async fn is_operational(&self) -> bool {
        let paired = self.oobe_manager.is_paired().await;
        let models_ready = self.models_ready().await;
        paired
            && models_ready
            && self.capture_available()
            && self.acoustic_ready.load(Ordering::Acquire)
    }

    fn capture_available(&self) -> bool {
        #[cfg(any(test, feature = "voice-skeleton"))]
        if self.replay_capture {
            return true;
        }
        self.mic_source.is_some()
    }

    async fn models_ready(&self) -> bool {
        #[cfg(any(test, feature = "voice-skeleton"))]
        if self.replay_capture {
            return true;
        }
        self.model_manager.get_status().await.state.is_ready()
    }

    /// Handle far-field wake event ("Hey Flowy").
    pub async fn handle_wake(&self) -> Result<(), VoiceWakeError> {
        self.handle_wake_keyword("Hey Flowy").await
    }

    /// Handle far-field wake event with specific matched keyword.
    pub async fn handle_wake_keyword(&self, keyword: &str) -> Result<(), VoiceWakeError> {
        if !self.oobe_manager.is_paired().await {
            warn!("Wake received but device is not yet paired via OOBE");
            let _ = self.playback.play_earcon(EarconKind::Alert).await;
            return Err(VoiceWakeError::PairingRequired);
        }

        if !self.models_ready().await {
            warn!("Wake received but voice models are not yet installed or verified");
            let _ = self.playback.play_earcon(EarconKind::Alert).await;
            return Err(VoiceWakeError::ModelsUnavailable);
        }

        if !self.capture_available() {
            return Err(VoiceWakeError::MicrophoneUnavailable);
        }
        if !self.acoustic_ready.load(Ordering::Acquire) {
            return Err(VoiceWakeError::ModelsUnavailable);
        }

        Self::accept_wake(&self.driver, &self.playback, &self.event_tx, keyword).await
    }

    async fn accept_wake(
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        events: &Arc<VoiceEventHub>,
        keyword: &str,
    ) -> Result<(), VoiceWakeError> {
        let mut d = driver.lock().await;
        let next = d
            .handle_event(AcousticEvent::WakeHit)
            .map_err(|_| VoiceWakeError::Busy)?;
        d.pause_listening_for_feedback();
        playback
            .interrupt()
            .await
            .map_err(|_| VoiceWakeError::Busy)?;
        info!(state = ?next, "Wake accepted");
        let _ = events.send(VoiceCompanionEvent::WakeHit {
            keyword: keyword.to_string(),
        });
        let _ = events.send(VoiceCompanionEvent::Listening);
        drop(d);
        let driver = driver.clone();
        let playback = playback.clone();
        let events = events.clone();
        let generation = playback.generation_id();
        tokio::spawn(async move {
            voice_feedback::play(&playback, &events, VoiceCueKind::Wake, generation).await;
            let mut d = driver.lock().await;
            if playback.generation_id() == generation && d.begin_listening_window() {
                let _ = events.send(VoiceCompanionEvent::ListeningReady { timeout_secs: 6 });
            }
        });
        Ok(())
    }

    /// One playback/follow-up path for chat, controls and task results.
    async fn play_response(
        reply_text:&str,
        driver:&Arc<Mutex<VoiceSessionDriver>>,
        playback:&Arc<PlaybackPipeline>,
        event_tx:&Arc<VoiceEventHub>,
        tts:&Arc<dyn TtsProvider>,
        generation:u64,
    ) -> Result<(), &'static str> {
        let guard=driver.lock().await;
        if playback.generation_id()!=generation {return Ok(());}
        info!("voice_response_playback_started");
        let _=event_tx.send(VoiceCompanionEvent::ResponseReady {text:reply_text.to_owned()});
        drop(guard);
        if !event_tx.feedback().speak_replies {
            Self::finish_muted_playback(driver, playback, event_tx, generation).await;
            return Ok(());
        }
        let _ = event_tx.send(VoiceCompanionEvent::TtsSynthesizing {
            text: reply_text.to_string(),
        });
        let started = std::time::Instant::now();
        let result = playback
            .speak_text_with_started(tts.as_ref(), reply_text, "", || {
                let _ = event_tx.send(VoiceCompanionEvent::TtsSpeaking {
                    text: reply_text.to_string(),
                });
            })
            .await;
        info!(elapsed_ms=started.elapsed().as_millis() as u64,ok=result.is_ok(),"voice_playback_finished");
        Self::finish_response_playback(result,driver,playback,event_tx,generation).await
    }

    async fn finish_muted_playback(
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        event_tx: &Arc<VoiceEventHub>,
        generation: u64,
    ) {
        let mut d = driver.lock().await;
        if playback.generation_id() != generation {
            return;
        }
        if d.current_state() == AcousticState::Deciding {
            let _ = d.handle_event(AcousticEvent::IntentChat);
        }
        let _ = d.handle_event(AcousticEvent::PlaybackDone);
        drop(d);
        let _ = event_tx.send(VoiceCompanionEvent::FollowUpStarted { text_only: true });
        voice_feedback::play(
            playback,
            event_tx,
            VoiceCueKind::FollowUp,
            generation,
        )
        .await;
        if playback.generation_id() != generation {
            return;
        }
        let mut d = driver.lock().await;
        if d.current_state() == AcousticState::FollowUpListening && d.begin_listening_window() {
            let _ = event_tx.send(VoiceCompanionEvent::TextFollowUpWindow { timeout_secs: 4 });
        } else if d.current_state() != AcousticState::FollowUpListening {
            d.reset();
        }
    }

    async fn finish_response_playback(
        result: Result<(), AudioSinkError>,
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        event_tx: &Arc<VoiceEventHub>,
        generation: u64,
    ) -> Result<(), &'static str> {
        let mut d2 = driver.lock().await;
        if playback.generation_id() != generation {
            return Ok(());
        }
        if d2.current_state() == AcousticState::Deciding {
            let _ = d2.handle_event(AcousticEvent::IntentChat);
        }
        let _ = d2.handle_event(AcousticEvent::PlaybackDone);
        drop(d2);
        if let Err(err) = result {
            let code = match err {
                AudioSinkError::RateLimited { .. } => "tts_rate_limited",
                AudioSinkError::Synthesis(_) => "tts_failed",
                AudioSinkError::NoAudio => "tts_empty_audio",
                _ => "speaker_failed",
            };
            warn!(code, "voice_playback_failed");
            let _ = playback.interrupt().await;
            let fallback_generation = playback.generation_id();
            let _ = event_tx.send(VoiceCompanionEvent::Error {
                stage: "playback".into(),
                code: code.into(),
            });
            let _ =
                event_tx.send(VoiceCompanionEvent::FollowUpStarted { text_only: true });
            voice_feedback::play(
                playback,
                event_tx,
                VoiceCueKind::Error,
                fallback_generation,
            )
            .await;
            voice_feedback::play(
                playback,
                event_tx,
                VoiceCueKind::FollowUp,
                fallback_generation,
            )
            .await;
            if playback.generation_id() != fallback_generation {
                return Err(code);
            }
            let mut d = driver.lock().await;
            if d.current_state() == AcousticState::FollowUpListening
                && d.begin_listening_window()
            {
                let _ = event_tx
                    .send(VoiceCompanionEvent::TextFollowUpWindow { timeout_secs: 4 });
            } else if d.current_state() != AcousticState::FollowUpListening {
                d.reset();
            }
            Err(code)
        } else {
            let _ = event_tx
                .send(VoiceCompanionEvent::FollowUpStarted { text_only: false });
            voice_feedback::play(
                playback,
                event_tx,
                VoiceCueKind::FollowUp,
                generation,
            )
            .await;
            if playback.generation_id() != generation {
                return Ok(());
            }
            let mut d = driver.lock().await;
            if d.current_state() == AcousticState::FollowUpListening
                && d.begin_listening_window()
            {
                let _ = event_tx
                    .send(VoiceCompanionEvent::FollowUpWindow { timeout_secs: 4 });
            } else if d.current_state() != AcousticState::FollowUpListening {
                d.reset();
            }
            Ok(())
        }
    }

    /// Internal helper to run ASR transcription, Qwen intent routing, TTS synthesis and FSM dispatch.
    async fn process_utterance(
        utterance_pcm: &[f32],
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        router: &Arc<dyn VoiceRouter>,
        transcriber: &Option<Arc<dyn SttCallback>>,
        playback: &Arc<PlaybackPipeline>,
        event_tx: &Arc<VoiceEventHub>,
        tts: &Arc<dyn TtsProvider>,
        task_client: &Option<Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>>,
        generation: u64,
    ) {
        if playback.generation_id() != generation {
            return;
        }
        let started = std::time::Instant::now();
        let scope = event_tx.snapshot();
        let mut task_context:Result<Option<nomifun_api_types::voice::VoiceTaskSnapshot>,String>=Ok(None);
        info!(session_id = ?scope.session_id, turn_id = ?scope.turn_id, samples = utterance_pcm.len(), sample_rate = 16000, "voice_asr_started");
        let text_opt = if let Some(stt) = transcriber {
            stt.transcribe(AudioChannel::Mic, utterance_pcm.to_vec(), 16_000)
                .await
        } else {
            None
        }
        .filter(|text| !text.trim().is_empty());
        if playback.generation_id() != generation {
            return;
        }
        info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            text_chars = text_opt.as_ref().map_or(0, |text| text.chars().count()),
            "voice_asr_finished"
        );
        if text_opt.as_ref().is_none_or(|text| text.trim().is_empty()) {
            warn!("voice_asr_empty_result");
            let _ = event_tx.send(VoiceCompanionEvent::Error {
                stage: "asr".into(),
                code: "asr_empty_result".into(),
            });
        }
        let decision = if let Some(ref text) = text_opt {
            info!("ASR transcription produced text");
            let _ = event_tx.send(VoiceCompanionEvent::SpeechRecognized { text: text.clone() });
            let clean_input = nomifun_audio::strip_wake_phrase(text);
            if !clean_input.is_empty() {
                voice_feedback::play(playback, event_tx, VoiceCueKind::Recognized, generation)
                    .await;
                if playback.generation_id() != generation {
                    return;
                }
            }
            if clean_input.is_empty() {
                let _ = event_tx.send(VoiceCompanionEvent::Error {
                    stage: "asr".into(),
                    code: "wake_only".into(),
                });
                None
            } else if let (Some(action), Some(_)) = (detect_task_control(&clean_input), task_client) {
                // Local task control fast-path: query and cancellation remain local,
                // zero dialogue model calls, fully functional even when model is offline.
                let dec = VoiceDecision {
                    intent_type: IntentType::TaskControl,
                    understood_request: Some(if action == nomifun_api_types::voice::VoiceTaskControlAction::CancelTask {
                        "取消当前后台任务".to_string()
                    } else {
                        "查询当前后台任务进度与状态".to_string()
                    }),
                    task_control_action: Some(action),
                    reply_tts: None,
                    spoken_ack: None,
                    task_goal: None,
                    requires_confirmation: None,
                };
                Some(dec)
            } else if !router.is_configured() {
                let _ = event_tx.send(VoiceCompanionEvent::Error {
                    stage: "dialogue".into(),
                    code: "model_unavailable".into(),
                });
                None
            } else {
                let started = std::time::Instant::now();
                let context = event_tx.context();
                if let Some(client) = task_client {
                    task_context = tokio::time::timeout(std::time::Duration::from_millis(250), client.current()).await
                        .unwrap_or_else(|_| Err("task_status_unavailable".into()));
                }
                if playback.generation_id() != generation {
                    return;
                }
                let task_status = task_context.as_ref().ok().and_then(|task| task.as_ref()).map(|task| serde_json::json!({
                    "command_id": task.command_id, "status": task.status,
                }).to_string());
                let (sentence_tx, mut sentence_rx) = tokio::sync::mpsc::channel::<nomifun_api_types::voice::VoiceDialogueSentence>(4);
                let (speech_tx, speech_rx) = tokio::sync::mpsc::channel::<String>(4);
                let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

                let req = nomifun_api_types::voice::VoiceDialogueRequest::scoped(
                    scope.session_id.as_deref().unwrap_or(""),
                    scope.turn_id.as_deref().unwrap_or(""),
                    &clean_input,
                );

                let mut router_guard = AbortOnDrop(Some(tokio::spawn({
                    let router = router.clone();
                    let clean = clean_input.clone();
                    let task_status = task_status.clone();
                    let context = context.clone();
                    async move {
                        router.route_scoped_stream(&req, &clean, task_status.as_deref(), &context, sentence_tx).await
                    }
                })));

                let speak_replies = event_tx.feedback().speak_replies;
                let first_speech_text = Arc::new(std::sync::Mutex::new(None::<String>));
                let mut playback_guard = if speak_replies {
                    let playback = playback.clone();
                    let tts = tts.clone();
                    let expected_generation = generation;
                    let on_started = {
                        let event_tx = event_tx.clone();
                        let first_speech_text = first_speech_text.clone();
                        let stream_start = started;
                        move || {
                            let text = first_speech_text.lock().unwrap().clone().unwrap_or_default();
                            let ttfa_ms = stream_start.elapsed().as_millis() as u64;
                            info!(elapsed_ms = ttfa_ms, text_chars = text.chars().count(), "voice_first_audio_started");
                            let _ = event_tx.send(VoiceCompanionEvent::TtsSpeaking { text });
                        }
                    };
                    AbortOnDrop(Some(tokio::spawn(async move {
                        playback.speak_sentence_stream(tts.as_ref(), speech_rx, expected_generation, cancel_rx, on_started).await
                    })))
                } else {
                    AbortOnDrop(None)
                };

                let mut router_result = None;
                let mut raw_text = None;
                let mut chat_streamed = false;
                let mut sentence_rx_closed = false;

                loop {
                    tokio::select! {
                        sentence_opt = sentence_rx.recv(), if !sentence_rx_closed => {
                            match sentence_opt {
                                Some(sentence) => {
                                    if playback.generation_id() != generation {
                                        let _ = cancel_tx.send(true);
                                        break;
                                    }
                                    if !chat_streamed {
                                        let mut d = driver.lock().await;
                                        if d.current_state() == AcousticState::Deciding {
                                            let _ = d.handle_event(AcousticEvent::IntentChat);
                                        }
                                        chat_streamed = true;
                                    }
                                    {
                                        let mut guard = first_speech_text.lock().unwrap();
                                        if guard.is_none() {
                                            *guard = Some(sentence.text.clone());
                                            if speak_replies {
                                                let _ = event_tx.send(VoiceCompanionEvent::TtsSynthesizing {
                                                    text: sentence.text.clone(),
                                                });
                                            }
                                        }
                                    }
                                    let _ = event_tx.send(VoiceCompanionEvent::ReplySentence {
                                        text: sentence.text.clone(),
                                        understood_request: sentence.understood_request.clone(),
                                    });
                                    if speak_replies {
                                        if speech_tx.send(sentence.text).await.is_err() {
                                            let _ = cancel_tx.send(true);
                                            break;
                                        }
                                    }
                                }
                                None => {
                                    sentence_rx_closed = true;
                                    if router_result.is_some() {
                                        break;
                                    }
                                }
                            }
                        }
                        res = router_guard.0.as_mut().unwrap(), if router_result.is_none() => {
                            let (res, raw) = match res {
                                Ok((res, raw)) => (res, raw),
                                Err(_) => (
                                    Err(nomifun_voice_terminal::driver::RouteError::Generation("model_panicked".into())),
                                    None,
                                ),
                            };
                            raw_text = raw;
                            match res {
                                Ok(dec) => {
                                    router_result = Some(Ok(dec));
                                    if sentence_rx_closed {
                                        break;
                                    }
                                }
                                Err(err) => {
                                    if !matches!(err, nomifun_voice_terminal::driver::RouteError::Record { .. }) {
                                        let _ = cancel_tx.send(true);
                                    }
                                    router_result = Some(Err(err));
                                    break;
                                }
                            }
                        }
                        else => {
                            break;
                        }
                    }
                }
                drop(speech_tx);

                if router_result.is_none() {
                    if let Some(mut handle) = router_guard.0.take() {
                        if let Ok((res, raw)) = (&mut handle).await {
                            raw_text = raw;
                            router_result = Some(res);
                        }
                    }
                } else {
                    router_guard.0 = None;
                }

                if playback.generation_id() != generation {
                    let _ = cancel_tx.send(true);
                    return;
                }

                if let Some(Err(err)) = router_result {
                    use nomifun_voice_terminal::driver::RouteError;
                    match err {
                        RouteError::Record { code, content } => {
                            raw_text = Some(content.clone());
                            let _ = event_tx.send(VoiceCompanionEvent::Error {
                                stage: "history".into(),
                                code: code.clone(),
                            });
                            if let Ok(dec) = nomifun_api_types::voice::parse_voice_decision(&content) {
                                if dec.intent_type == IntentType::Chat {
                                    router_result = Some(Ok(dec));
                                } else {
                                    let _ = playback.interrupt().await;
                                    let _ = playback_guard.0.take();
                                    driver.lock().await.reset();
                                    let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                        reason: VoiceSessionEndReason::Failed,
                                    });
                                    return;
                                }
                            } else {
                                let _ = playback.interrupt().await;
                                let _ = playback_guard.0.take();
                                driver.lock().await.reset();
                                let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                    reason: VoiceSessionEndReason::Failed,
                                });
                                return;
                            }
                        }
                        other_err => {
                            let _ = playback.interrupt().await;
                            let _ = playback_guard.0.take();
                            let code = match other_err {
                                RouteError::Network(_) => "model_network_failed",
                                RouteError::Generation(ref detail)
                                    if matches!(
                                        detail.as_str(),
                                        "model_unavailable"
                                            | "model_auth_failed"
                                            | "model_network_failed"
                                            | "model_rejected"
                                            | "model_response_invalid"
                                            | "model_timeout"
                                    ) =>
                                {
                                    detail.as_str()
                                }
                                RouteError::Validation(_) => "model_response_invalid",
                                RouteError::Generation(ref detail)
                                    if detail.contains("401") || detail.contains("403") =>
                                {
                                    "model_auth_failed"
                                }
                                RouteError::Generation(ref detail)
                                    if detail.starts_with("HTTP status") =>
                                {
                                    "model_rejected"
                                }
                                _ => "model_response_invalid",
                            };
                            warn!(code, "voice_model_failed");
                            let _ = event_tx.send(VoiceCompanionEvent::Error {
                                stage: "dialogue".into(),
                                code: code.into(),
                            });
                            driver.lock().await.reset();
                            let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                reason: VoiceSessionEndReason::Failed,
                            });
                            return;
                        }
                    }
                }

                let dec = match router_result {
                    Some(Ok(d)) => d,
                    _ => {
                        let _ = playback_guard.0.take();
                        return;
                    }
                };

                if let Some(text) = raw_text {
                    let _ = event_tx.send(VoiceCompanionEvent::ModelResponse { text });
                }
                info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    ok = true,
                    "voice_model_finished"
                );

                if chat_streamed {
                    let _ = event_tx.send(VoiceCompanionEvent::IntentDecided {
                        intent: "Chat".into(),
                        reply: dec.reply_tts.clone(),
                        understood_request: dec.understood_request.clone(),
                    });
                    info!(intent = ?dec.intent_type, has_understanding = dec.understood_request.is_some(), "voice_intent_decided");

                    if !speak_replies {
                        Self::finish_muted_playback(driver, playback, event_tx, generation).await;
                        return;
                    }

                    let play_res = if let Some(h) = playback_guard.0.take() {
                        match h.await {
                            Ok(res) => res,
                            Err(_) => Err(nomifun_voice_terminal::playback::AudioSinkError::Synthesis(
                                "tts_task_panicked".into(),
                            )),
                        }
                    } else {
                        Ok(())
                    };
                    if playback.generation_id() != generation {
                        return;
                    }

                    let _ = Self::finish_response_playback(play_res, driver, playback, event_tx, generation).await;
                    return;
                }

                let _ = playback_guard.0.take();
                Some(dec)
            }
        } else {
            None
        };

        if playback.generation_id() != generation {
            return;
        }

        let mut d = driver.lock().await;
        if playback.generation_id() != generation {
            return;
        }
        if let Some(dec) = decision {
            let _ = event_tx.send(VoiceCompanionEvent::IntentDecided {
                intent: format!("{:?}", dec.intent_type),
                reply: dec.reply_tts.clone().filter(|_| dec.intent_type == IntentType::Chat),
                understood_request: dec.understood_request.clone(),
            });
            info!(intent = ?dec.intent_type, has_understanding = dec.understood_request.is_some(), "voice_intent_decided");
            match dec.intent_type {
                IntentType::Ignore => {
                    let _ = d.handle_event(AcousticEvent::IntentIgnore);
                    drop(d);
                    voice_feedback::play(playback, event_tx, VoiceCueKind::Sleep, generation).await;
                    if playback.generation_id() == generation {
                        let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                            reason: VoiceSessionEndReason::NoIntent,
                        });
                    }
                }
                IntentType::Chat => {
                    let _ = d.handle_event(AcousticEvent::IntentChat);
                    drop(d);
                    let Some(reply_text) = dec
                        .reply_tts
                        .as_deref()
                        .filter(|text| !text.trim().is_empty())
                    else {
                        let mut d = driver.lock().await;
                        d.reset();
                        let _ = event_tx.send(VoiceCompanionEvent::Error {
                            stage: "dialogue".into(),
                            code: "model_response_invalid".into(),
                        });
                        let _ = event_tx.send(VoiceCompanionEvent::Idle);
                        return;
                    };
                    let _ = Self::play_response(reply_text,driver,playback,event_tx,tts,generation).await;
                }
                IntentType::TaskControl if task_client.is_some() => {
                    use nomifun_api_types::voice::{VoiceSessionControlRequest,VoiceTaskControlCommand,VoiceTaskReference,VoiceTaskStatus};
                    let snapshot=event_tx.snapshot();
                    if task_context.as_ref().ok().and_then(|t| t.as_ref()).is_none() {
                        if let Some(client) = task_client {
                            task_context = tokio::time::timeout(std::time::Duration::from_millis(250), client.current()).await
                                .unwrap_or_else(|_| Err("task_status_unavailable".into()));
                        }
                    }
                    let request=match (task_context,dec.task_control_action) {
                        (Ok(Some(task)),Some(action)) => Ok(VoiceTaskControlCommand {
                            scope:VoiceSessionControlRequest {instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},
                            task:VoiceTaskReference::from(&task),action,
                        }),
                        (Err(_),_) => Err("task_status_unavailable".to_owned()),
                        (Ok(None),_) => Err("task_not_found".into()),
                        (_,None) => Err("model_response_invalid".into()),
                    };
                    let _=d.handle_event(AcousticEvent::IntentTaskControl);
                    drop(d);
                    let result=match request {
                        Ok(request) => task_client.as_ref().unwrap().control(request).await,
                        Err(code) => Err(code),
                    };
                    if playback.generation_id()!=generation {return;}
                    match result {
                        Ok(task) => {
                            let text=match task.status {
                                VoiceTaskStatus::Delivering => "任务正在提交，尚未核实接单。".into(),
                                VoiceTaskStatus::Unknown => "任务状态尚未核实，正在核对原回执。".into(),
                                VoiceTaskStatus::Accepted => "任务已接受，正在等待执行结果。".into(),
                                VoiceTaskStatus::Cancelling => "正在取消任务，等待执行侧确认。".into(),
                                VoiceTaskStatus::Completed => format!("任务已完成。{}",task.result_text.as_deref().unwrap_or("请查看任务详情。").chars().take(220).collect::<String>()),
                                VoiceTaskStatus::Failed => format!("任务已结束，执行结果未成功。{}",task.result_text.as_deref().unwrap_or("请查看任务详情了解原因。").chars().take(220).collect::<String>()),
                            };
                            let _ = Self::play_response(&text,driver,playback,event_tx,tts,generation).await;
                        },
                        Err(code) => {
                            let mut driver=driver.lock().await;
                            if playback.generation_id()!=generation {return;}
                            driver.reset();
                            let _=event_tx.send(VoiceCompanionEvent::Error {stage:"task".into(),code});
                            drop(driver);
                            voice_feedback::play(playback,event_tx,VoiceCueKind::Error,generation).await;
                            if playback.generation_id()==generation {let _=event_tx.send(VoiceCompanionEvent::SessionEnded {reason:VoiceSessionEndReason::Failed});}
                        },
                    }
                }
                IntentType::AgentTask if task_client.is_some() => {
                    use nomifun_api_types::voice::{VoiceTaskCommand, VoiceTaskStatus, VoiceTaskSubmitRequest};
                    let _ = d.handle_event(AcousticEvent::IntentAgentTask);
                    drop(d);
                    let _ = event_tx.send(VoiceCompanionEvent::TaskDispatching);
                    let scope = event_tx.snapshot();
                    let command = match (scope.session_id, scope.turn_id, dec.task_goal.filter(|goal| !goal.trim().is_empty())) {
                        (Some(session_id),Some(turn_id),Some(goal)) => VoiceTaskCommand {
                            command_id: format!("voice-{turn_id}"),
                            record_id: Some(format!("voice-{turn_id}")),
                            binding_id: Some(format!("voice-binding-{session_id}")),
                            session_id,
                            turn_id,
                            goal,
                        },
                        _ => {
                            driver.lock().await.reset();
                            let _ = event_tx.send(VoiceCompanionEvent::Error {stage:"task".into(),code:"task_goal_invalid".into()});
                            let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {reason:VoiceSessionEndReason::Failed});
                            return;
                        }
                    };
                    let result = task_client.as_ref().unwrap().submit(VoiceTaskSubmitRequest {
                        instance_id:scope.instance_id,command:command.clone(),
                    }).await;
                    if playback.generation_id() != generation { return; }
                    let code = match result {
                        Ok(delivery) if delivery.task.command_id != command.command_id
                            || delivery.task.session_id != command.session_id
                            || delivery.task.acoustic_turn_id != command.turn_id => Some("task_receipt_scope_changed".into()),
                        Ok(delivery) if delivery.task.status == VoiceTaskStatus::Failed => Some(delivery.task.error_code.unwrap_or_else(||"task_failed".into())),
                        Ok(delivery) if delivery.freshly_accepted && delivery.task.status == VoiceTaskStatus::Accepted
                            && delivery.task.conversation_id.is_some() && delivery.task.message_id.is_some() => {
                            let text = "任务已接单，正在执行。";
                            let _ = event_tx.send(VoiceCompanionEvent::ResponseReady { text: text.into() });
                            let _ = event_tx.send(VoiceCompanionEvent::TtsSynthesizing {text:text.into()});
                            if event_tx.feedback().speak_replies {
                                playback.speak_text_with_started(tts.as_ref(),text,"", || {
                                    let _ = event_tx.send(VoiceCompanionEvent::TtsSpeaking {text:text.into()});
                                }).await.err().map(|error| match error {AudioSinkError::Synthesis(_) => "tts_failed", AudioSinkError::NoAudio => "tts_empty_audio", _ => "speaker_failed"}.to_owned())
                            } else {None}
                        }
                        Ok(delivery) if delivery.task.conversation_id.is_none() || delivery.task.message_id.is_none()
                            || matches!(delivery.task.status, VoiceTaskStatus::Delivering | VoiceTaskStatus::Unknown) => Some("task_delivery_unknown".into()),
                        Ok(_) => None,
                        Err(code) => Some(code),
                    };
                    if playback.generation_id() != generation {return;}
                    let mut driver = driver.lock().await;
                    let _ = driver.handle_event(AcousticEvent::TaskAccepted);
                    drop(driver);
                    let failed = code.is_some();
                    if let Some(code) = code {
                        let _ = event_tx.send(VoiceCompanionEvent::Error {stage:"task".into(),code});
                        voice_feedback::play(playback,event_tx,VoiceCueKind::Error,generation).await;
                    }
                    if playback.generation_id() == generation {
                        let _ = event_tx.send(if failed {VoiceCompanionEvent::SessionEnded {reason:VoiceSessionEndReason::Failed}} else {VoiceCompanionEvent::Idle});
                    }
                }
                IntentType::TaskControl | IntentType::AgentTask => {
                    // Only a real runtime receipt may authorize an acceptance announcement.
                    let _ = d.handle_event(AcousticEvent::IntentIgnore);
                    drop(d);
                    let _ = event_tx.send(VoiceCompanionEvent::Error {
                        stage: "task".into(),
                        code: "runtime_unavailable".into(),
                    });
                    voice_feedback::play(playback, event_tx, VoiceCueKind::Error, generation).await;
                    if playback.generation_id() == generation {
                        let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                            reason: VoiceSessionEndReason::Failed,
                        });
                    }
                }
            }
        } else {
            let _ = d.handle_event(AcousticEvent::IntentIgnore);
            drop(d);
            voice_feedback::play(playback, event_tx, VoiceCueKind::Error, generation).await;
            if playback.generation_id() == generation {
                let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                    reason: VoiceSessionEndReason::Failed,
                });
            }
        }
    }

    /// Keeps capture consuming frames while a turn awaits the model or speaker.
    fn start_utterance(
        pcm: Vec<f32>,
        driver: Arc<Mutex<VoiceSessionDriver>>,
        router: Arc<dyn VoiceRouter>,
        transcriber: Option<Arc<dyn SttCallback>>,
        playback: Arc<PlaybackPipeline>,
        events: Arc<VoiceEventHub>,
        tts: Arc<dyn TtsProvider>,
        task_client: Option<Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>>,
        active_turn: &Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
        generation: u64,
    ) {
        let mut active = active_turn.lock().unwrap_or_else(|e| e.into_inner());
        if playback.generation_id() != generation {
            return;
        }
        if let Some(previous) = active.take() {
            previous.abort();
        }
        *active = Some(tokio::spawn(async move {
            use futures_util::FutureExt;
            if playback.generation_id() != generation {
                return;
            }
            let result = std::panic::AssertUnwindSafe(Self::process_utterance(
                &pcm,
                &driver,
                &router,
                &transcriber,
                &playback,
                &events,
                &tts,
                &task_client,
                generation,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let mut d = driver.lock().await;
                if playback.generation_id() != generation {
                    return;
                }
                d.reset();
                let _ = playback.interrupt().await;
                warn!(generation, "voice_turn_panicked");
                let _ = events.send(VoiceCompanionEvent::Error {
                    stage: "turn".into(),
                    code: "voice_turn_failed".into(),
                });
                let _ = events.send(VoiceCompanionEvent::Idle);
            }
        }));
    }

    pub fn start_worker(&self) -> tokio::task::JoinHandle<()> {
        let driver = Arc::clone(&self.driver);
        let capture = Arc::clone(&self.capture_pipeline);
        let playback = Arc::clone(&self.playback);
        let tts = Arc::clone(&self.tts);
        let router = Arc::clone(&self.router);
        let task_client = self.task_client.clone();
        let pending_confirmation=self.pending_confirmation.clone();
        let mut transcriber = self.transcriber.clone();
        let acoustic_ready = self.acoustic_ready.clone();
        let capture_available = self.capture_available();
        let replay = {
            #[cfg(any(test, feature = "voice-skeleton"))]
            {
                self.replay_capture
            }
            #[cfg(not(any(test, feature = "voice-skeleton")))]
            {
                false
            }
        };
        let oobe = Arc::clone(&self.oobe_manager);
        let models = Arc::clone(&self.model_manager);
        let event_tx = self.event_tx.clone();
        let active_turn = self.active_turn.clone();

        tokio::spawn(async move {
            let mut kws = KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
                model_dir: Some(models.models_dir().to_path_buf()),
                ..Default::default()
            })
            .expect("the fixed 16 kHz KWS configuration is valid");
            let mut vad: Box<dyn VadBackend> = match SherpaVad::load(models.models_dir()) {
                Ok(vad) => Box::new(vad),
                // Synthetic worker tests without installed assets retain their test detector.
                #[cfg(any(test, feature = "voice-skeleton"))]
                Err(_) if replay && !models.models_dir().join("vad/silero_vad.onnx").is_file() => {
                    Box::new(EnergyVad::new(VadConfig {
                        threshold: 0.01,
                        ..Default::default()
                    }))
                }
                Err(code) => {
                    acoustic_ready.store(false, Ordering::Release);
                    warn!(code, "voice_vad_load_failed");
                    let _ = event_tx.send(VoiceCompanionEvent::Error {
                        stage: "capture".into(),
                        code,
                    });
                    let _ = event_tx.send(VoiceCompanionEvent::Availability {
                        acoustic_ready: false,
                        dialogue_ready: router.is_configured(),
                        playback_ready: playback.is_available(),
                    });
                    return;
                }
            };
            info!(silence_ms = 800, "voice_vad_loaded");
            let mut utterance_pcm = Vec::<f32>::new();
            let mut speech_started = false;
            let mut listen_start: Option<std::time::Instant> = None;
            let mut last_model_check: Option<std::time::Instant> = None;
            let mut last_availability = None;
            let mut observed_session = None;
            let mut audio = VoiceAudioDiagnostics::default();
            audio.vad_ready = true;
            let mut last_audio_publish = std::time::Instant::now();
            let mut last_audio_log = std::time::Instant::now();
            let mut last_pcm = std::time::Instant::now();
            let mut last_asr_load_warning: Option<std::time::Instant> = None;
            let mut processing_feedback: Option<(String, std::time::Instant, u8)> = None;
            let quiet_input = Arc::new(AtomicBool::new(false));
            let mut quiet_since: Option<std::time::Instant> = None;
            let mut reminded_session: Option<String> = None;

            loop {
                let session = event_tx.snapshot().session_id;
                if session != observed_session {
                    observed_session = session;
                    utterance_pcm.clear();
                    speech_started = false;
                    listen_start = None;
                    audio.utterance_samples = 0;
                    audio.utterance_peak_milli = 0;
                    vad.reset();
                    kws.reset();
                }
                if !replay
                    && capture_available
                    && last_model_check
                        .is_none_or(|last| last.elapsed() >= std::time::Duration::from_secs(1))
                {
                    last_model_check = Some(std::time::Instant::now());
                    if models.get_status().await.state.is_ready() {
                        if !kws.is_initialized() {
                            kws = KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
                                model_dir: Some(models.models_dir().to_path_buf()),
                                ..Default::default()
                            })
                            .expect("fixed KWS configuration");
                        }
                        if transcriber.is_none() {
                            let dir = models.models_dir().to_path_buf();
                            let loaded = tokio::task::spawn_blocking(move || {
                                nomifun_audio::stt::try_load_sherpa(Some(&dir))
                            })
                            .await;
                            let report = last_asr_load_warning.is_none_or(|last| {
                                last.elapsed() >= std::time::Duration::from_secs(5)
                            });
                            match loaded {
                                Ok(Ok(Some(stt))) => transcriber = Some(Arc::new(stt)),
                                Ok(Err(err)) if report => {
                                    warn!(error = %err, "voice_asr_load_failed");
                                    last_asr_load_warning = Some(std::time::Instant::now());
                                }
                                Err(err) if report => {
                                    warn!(error = %err, "voice_asr_loader_failed");
                                    last_asr_load_warning = Some(std::time::Instant::now());
                                }
                                Ok(Ok(None)) if report => {
                                    warn!(models_dir = %models.models_dir().display(), "voice_asr_model_not_found");
                                    last_asr_load_warning = Some(std::time::Instant::now());
                                }
                                _ => {}
                            }
                        }
                        acoustic_ready.store(
                            kws.is_initialized()
                                && transcriber.is_some()
                                && audio.capture_state != "closed",
                            Ordering::Release,
                        );
                    } else {
                        acoustic_ready.store(false, Ordering::Release);
                    }
                }
                let availability = (
                    acoustic_ready.load(Ordering::Acquire),
                    router.is_configured(),
                    playback.is_available(),
                );
                audio.asr_ready = transcriber.is_some();
                audio.kws_ready = kws.is_initialized();
                if last_availability != Some(availability) {
                    info!(
                        microphone_open = capture_available,
                        kws_ready = kws.is_initialized(),
                        asr_ready = transcriber.is_some(),
                        dialogue_ready = availability.1,
                        "voice_engines_ready"
                    );
                    last_availability = Some(availability);
                    let _ = event_tx.send(VoiceCompanionEvent::Availability {
                        acoustic_ready: availability.0,
                        dialogue_ready: availability.1,
                        playback_ready: availability.2,
                    });
                    let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                        diagnostics: audio.clone(),
                    });
                }
                // 1. Check FSM watchdog timers (e.g. 8-second confirmation timeout)
                let snapshot = event_tx.snapshot();
                if matches!(
                    snapshot.phase,
                    nomifun_api_types::voice::VoiceSessionPhase::Transcribing
                        | nomifun_api_types::voice::VoiceSessionPhase::Deciding
                        | nomifun_api_types::voice::VoiceSessionPhase::Synthesizing
                ) {
                    if let Some((turn, start, count)) = &mut processing_feedback {
                        if snapshot.turn_id.as_deref() == Some(turn.as_str())
                            && *count < 3
                            && start.elapsed()
                                >= std::time::Duration::from_secs(3 + u64::from(*count) * 6)
                        {
                            *count += 1;
                            voice_feedback::schedule_optional(
                                playback.clone(),
                                event_tx.clone(),
                                VoiceCueKind::Processing,
                                playback.generation_id(),
                                quiet_input.clone(),
                                None,
                            );
                        }
                    }
                } else {
                    processing_feedback = None;
                }
                {
                    let mut d = driver.lock().await;
                    let timed_state = d.current_state();
                    if timed_state == AcousticState::Listening
                        && quiet_input.load(Ordering::Acquire)
                        && snapshot.session_id.is_some()
                        && reminded_session != snapshot.session_id
                        && d.remaining_listening_time().is_some_and(|remaining| {
                            !remaining.is_zero() && remaining <= std::time::Duration::from_secs(3)
                        })
                    {
                        reminded_session = snapshot.session_id.clone();
                        voice_feedback::schedule_optional(
                            playback.clone(),
                            event_tx.clone(),
                            VoiceCueKind::ListeningReminder,
                            playback.generation_id(),
                            quiet_input.clone(),
                            Some(driver.clone()),
                        );
                    }
                    if let Ok(Some(next)) = d.check_watchdog() {
                        info!(state = ?next, pcm_frames = audio.frames_received, rms_milli = audio.rms_milli, speech_detected = speech_started, "voice_listen_timeout");
                        if next == AcousticState::Idle {
                            let code = if timed_state==AcousticState::ListeningConfirm {
                                "confirmation_silence"
                            } else if timed_state == AcousticState::FollowUpListening {
                                "follow_up_silence"
                            } else if last_pcm.elapsed() > std::time::Duration::from_secs(2) {
                                "capture_no_pcm"
                            } else {
                                "initial_silence"
                            };
                            info!(
                                code,
                                samples = audio.utterance_samples,
                                peak_milli = audio.utterance_peak_milli,
                                "voice_listening_ended"
                            );
                            if code == "capture_no_pcm" {
                                let _ = event_tx.send(VoiceCompanionEvent::Error {
                                    stage: "capture".into(),
                                    code: code.into(),
                                });
                            }
                            utterance_pcm.clear();
                            speech_started = false;
                            listen_start = None;
                            vad.reset();
                            kws.reset();
                            let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                reason: match code {
                                    "follow_up_silence" => VoiceSessionEndReason::FollowUpSilence,
                                    "initial_silence" => VoiceSessionEndReason::InitialSilence,
                                    "confirmation_silence"=>VoiceSessionEndReason::NoIntent,
                                    _ => VoiceSessionEndReason::Failed,
                                },
                            });
                            voice_feedback::schedule(
                                playback.clone(),
                                event_tx.clone(),
                                if code == "capture_no_pcm" {
                                    VoiceCueKind::Error
                                } else {
                                    VoiceCueKind::Sleep
                                },
                                playback.generation_id(),
                            );
                        }
                    }
                }

                // 2. Fetch next PCM audio chunk from capture pipeline
                let chunk_result = {
                    let mut cap = capture.lock().await;
                    cap.try_next_chunk()
                };

                let chunk = match chunk_result {
                    CaptureReadResult::Chunk(samples) => samples,
                    CaptureReadResult::TemporarilyEmpty => {
                        if last_pcm.elapsed() > std::time::Duration::from_secs(2)
                            && last_audio_publish.elapsed() >= std::time::Duration::from_secs(1)
                        {
                            audio.capture_state = "no_pcm".into();
                            audio.rms_milli = 0;
                            audio.peak_milli = 0;
                            audio.speech_detected = false;
                            let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                                diagnostics: audio.clone(),
                            });
                            warn!(frames = audio.frames_received, "voice_capture_no_pcm");
                            last_audio_publish = std::time::Instant::now();
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        continue;
                    }
                    CaptureReadResult::StreamClosed | CaptureReadResult::DeviceError(_) => {
                        if audio.capture_state != "closed" {
                            let mut d = driver.lock().await;
                            d.reset();
                            let _ = playback.interrupt().await;
                            if let Some(turn) =
                                active_turn.lock().unwrap_or_else(|e| e.into_inner()).take()
                            {
                                turn.abort();
                            }
                            drop(d);
                            processing_feedback = None;
                            quiet_input.store(false, Ordering::Release);
                            utterance_pcm.clear();
                            speech_started = false;
                            listen_start = None;
                            vad.reset();
                            kws.reset();
                            audio.capture_state = "closed".into();
                            audio.rms_milli = 0;
                            audio.peak_milli = 0;
                            audio.speech_detected = false;
                            acoustic_ready.store(false, Ordering::Release);
                            let _ = event_tx.send(VoiceCompanionEvent::Availability {
                                acoustic_ready: false,
                                dialogue_ready: router.is_configured(),
                                playback_ready: playback.is_available(),
                            });
                            let _ = event_tx.send(VoiceCompanionEvent::Error {
                                stage: "capture".into(),
                                code: "capture_closed".into(),
                            });
                            let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                reason: VoiceSessionEndReason::Failed,
                            });
                            voice_feedback::schedule(
                                playback.clone(),
                                event_tx.clone(),
                                VoiceCueKind::Error,
                                playback.generation_id(),
                            );
                            warn!("voice_capture_closed_or_failed");
                            let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                                diagnostics: audio.clone(),
                            });
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };

                last_pcm = std::time::Instant::now();
                audio.frames_received += 1;
                audio.last_frame_at_ms = Some(chrono::Utc::now().timestamp_millis());
                audio.capture_state = "receiving".into();
                audio.asr_ready = transcriber.is_some();
                audio.kws_ready = kws.is_initialized();
                let rms = if chunk.is_empty() {
                    0.0
                } else {
                    (chunk.iter().map(|sample| sample * sample).sum::<f32>() / chunk.len() as f32)
                        .sqrt()
                };
                audio.rms_milli = (rms.clamp(0.0, 1.0) * 1000.0).round() as u16;
                audio.peak_milli = (chunk
                    .iter()
                    .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
                    .clamp(0.0, 1.0)
                    * 1000.0)
                    .round() as u16;
                if rms >= 0.001 || vad.is_speech_active() {
                    quiet_since = None;
                    quiet_input.store(false, Ordering::Release);
                } else {
                    let since = quiet_since.get_or_insert_with(std::time::Instant::now);
                    quiet_input.store(
                        since.elapsed() >= std::time::Duration::from_millis(500),
                        Ordering::Release,
                    );
                }
                if last_audio_publish.elapsed() >= std::time::Duration::from_millis(200) {
                    let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                        diagnostics: audio.clone(),
                    });
                    last_audio_publish = std::time::Instant::now();
                }
                if last_audio_log.elapsed() >= std::time::Duration::from_secs(5) {
                    info!(
                        frames = audio.frames_received,
                        rms_milli = audio.rms_milli,
                        peak_milli = audio.peak_milli,
                        vad_active = audio.speech_detected,
                        "voice_capture_heartbeat"
                    );
                    last_audio_log = std::time::Instant::now();
                }

                let current_state = {
                    let d = driver.lock().await;
                    d.current_state()
                };

                match current_state {
                    AcousticState::Idle => {
                        // Standby: feed audio chunk to Keyword Spotter
                        let mut hit = kws.process_pcm_chunk(&chunk).map(|r| r.keyword);

                        // Stage-2: Cascade VAD + SenseVoice check if streaming KWS was borderline
                        if hit.is_none() {
                            let is_active = vad.process_frame(&chunk);
                            if is_active {
                                utterance_pcm.extend_from_slice(&chunk);
                                if utterance_pcm.len() > 16_000 * 4 {
                                    utterance_pcm.drain(0..16_000);
                                }
                            } else if !utterance_pcm.is_empty() {
                                if utterance_pcm.len() >= 4_000 {
                                    let snippet = std::mem::take(&mut utterance_pcm);
                                    if let Some(stt) = &transcriber
                                        && let Some(text) =
                                            stt.transcribe(AudioChannel::Mic, snippet, 16_000).await
                                    {
                                        if nomifun_audio::is_wake_phrase_match(&text) {
                                            info!("Cascade wake hit accepted via SenseVoice");
                                            hit = Some(text);
                                        }
                                    }
                                } else {
                                    utterance_pcm.clear();
                                }
                                vad.reset();
                            }
                        }

                        if let Some(keyword) = hit {
                            info!(keyword = %keyword, "Wake hit detected");
                            if !oobe.is_paired().await {
                                warn!("Wake received but device is not yet paired via OOBE");
                                let _ = playback.play_earcon(EarconKind::Alert).await;
                            } else if !acoustic_ready.load(Ordering::Acquire) {
                                warn!("Wake received but voice models are not yet installed");
                                let _ = playback.play_earcon(EarconKind::Alert).await;
                            } else {
                                if Self::accept_wake(&driver, &playback, &event_tx, &keyword)
                                    .await
                                    .is_ok()
                                {
                                    observed_session = event_tx.snapshot().session_id;
                                    vad.reset();
                                    utterance_pcm.clear();
                                    speech_started = false;
                                    listen_start = Some(std::time::Instant::now());
                                    audio.utterance_samples = 0;
                                    audio.utterance_peak_milli = 0;
                                }
                            }
                        }
                    }

                    AcousticState::ListeningConfirm=>{
                        utterance_pcm.extend_from_slice(&chunk);
                        if !speech_started && utterance_pcm.len()>8000 {let excess=utterance_pcm.len()-8000;utterance_pcm.drain(..excess);}
                        audio.utterance_samples=utterance_pcm.len() as u32;
                        audio.utterance_peak_milli=audio.utterance_peak_milli.max(audio.peak_milli);
                        let active=vad.process_frame(&chunk);speech_started|=active;
                        if speech_started && !active {
                            let response={let mut pending=pending_confirmation.lock().unwrap_or_else(|error|error.into_inner());pending.as_mut().and_then(|pending|pending.response.take().map(|response|(response,pending.generation,pending.deadline_ms)))};
                            if let Some((response,generation,deadline_ms))=response {
                                let pcm=std::mem::take(&mut utterance_pcm);let stt=transcriber.clone();let output=playback.clone();let events=event_tx.clone();let states=driver.clone();
                                let turn=tokio::spawn(async move {
                                    let text=match stt {Some(stt)=>stt.transcribe(AudioChannel::Mic,pcm,16000).await,None=>None};
                                    let guard=states.lock().await;
                                    if output.generation_id()==generation && chrono::Utc::now().timestamp_millis()<deadline_ms && guard.current_state()==AcousticState::ListeningConfirm {
                                        if let Some(text)=&text {let _=events.send(VoiceCompanionEvent::ConfirmationHeard {text:text.clone()});}
                                        let _=response.send(text);
                                    }
                                });
                                let mut active=active_turn.lock().unwrap_or_else(|error|error.into_inner());if let Some(previous)=active.replace(turn){previous.abort();}
                            }
                            speech_started=false;utterance_pcm.clear();vad.reset();
                        }
                    }
                    AcousticState::Listening => {
                        // User speech capture: accumulate PCM samples
                        utterance_pcm.extend_from_slice(&chunk);
                        audio.utterance_samples = utterance_pcm.len() as u32;
                        audio.utterance_peak_milli =
                            audio.utterance_peak_milli.max(audio.peak_milli);

                        // Feed frame to VAD detector
                        let is_active = vad.process_frame(&chunk);
                        if is_active {
                            if !speech_started {
                                info!(session_id = ?event_tx.snapshot().session_id, rms_milli = audio.rms_milli, "voice_speech_started");
                                driver.lock().await.mark_speech_started();
                                let _ = event_tx.send(VoiceCompanionEvent::SpeechStarted);
                            }
                            speech_started = true;
                        }
                        if speech_started && (!is_active || utterance_pcm.len() >= 16_000 * 30) {
                            // User paused / end of utterance detected
                            info!(
                                samples = utterance_pcm.len(),
                                "SpeechEnd detected by VAD, transitioning to Deciding"
                            );
                            speech_started = false;
                            listen_start = None;

                            let mut d = driver.lock().await;
                            if let Ok(AcousticState::Deciding) =
                                d.handle_event(AcousticEvent::SpeechEnd)
                            {
                                let generation = playback.generation_id();
                                drop(d);
                                let _ = event_tx.send(VoiceCompanionEvent::Transcribing);
                                processing_feedback = event_tx
                                    .snapshot()
                                    .turn_id
                                    .map(|id| (id, std::time::Instant::now(), 0));
                                Self::start_utterance(
                                    std::mem::take(&mut utterance_pcm),
                                    driver.clone(),
                                    router.clone(),
                                    transcriber.clone(),
                                    playback.clone(),
                                    event_tx.clone(),
                                    tts.clone(),
                                    task_client.clone(),
                                    &active_turn,
                                    generation,
                                );
                                utterance_pcm.clear();
                                kws.reset();
                                vad.reset();
                            }
                        }
                    }

                    AcousticState::FollowUpListening => {
                        // 4-second follow-up window (seamless continuous dialogue without wake phrase)
                        if listen_start.is_none() {
                            listen_start = Some(std::time::Instant::now());
                            vad.reset();
                            utterance_pcm.clear();
                            speech_started = false;
                        }

                        // Keep a bounded onset window while VAD accumulates its
                        // evidence; speech begins before the active flag does.
                        utterance_pcm.extend_from_slice(&chunk);
                        if !speech_started && utterance_pcm.len() > 8_000 {
                            let excess = utterance_pcm.len() - 8_000;
                            utterance_pcm.drain(..excess);
                        }
                        audio.utterance_samples = utterance_pcm.len() as u32;
                        audio.utterance_peak_milli =
                            audio.utterance_peak_milli.max(audio.peak_milli);
                        let is_active = vad.process_frame(&chunk);
                        if is_active {
                            if !speech_started {
                                driver.lock().await.mark_speech_started();
                                let _ = event_tx.send(VoiceCompanionEvent::SpeechStarted);
                            }
                            speech_started = true;
                        }
                        if speech_started && (!is_active || utterance_pcm.len() >= 16_000 * 30) {
                            // User followed up with another utterance during the 4s window!
                            info!(
                                samples = utterance_pcm.len(),
                                "Follow-up speech ended, transitioning to Deciding"
                            );
                            speech_started = false;
                            listen_start = None;

                            let mut d = driver.lock().await;
                            if let Ok(AcousticState::Deciding) =
                                d.handle_event(AcousticEvent::FollowUpSpeech)
                            {
                                let generation = playback.generation_id();
                                drop(d);
                                let _ = event_tx.send(VoiceCompanionEvent::Transcribing);
                                processing_feedback = event_tx
                                    .snapshot()
                                    .turn_id
                                    .map(|id| (id, std::time::Instant::now(), 0));
                                Self::start_utterance(
                                    std::mem::take(&mut utterance_pcm),
                                    driver.clone(),
                                    router.clone(),
                                    transcriber.clone(),
                                    playback.clone(),
                                    event_tx.clone(),
                                    tts.clone(),
                                    task_client.clone(),
                                    &active_turn,
                                    generation,
                                );
                                utterance_pcm.clear();
                                kws.reset();
                                vad.reset();
                            }
                        }
                    }

                    _ => {}
                }
                audio.speech_detected = vad.is_speech_active();
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_voice_terminal::contract::AcousticState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_voice_runtime_assembly() {
        let dir = tempdir().unwrap();
        let model_manager = Arc::new(ModelManager::new(dir.path()));
        let oobe_manager = Arc::new(OobeManager::new_bypassed());

        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, mut playback_rx) = VoiceCompanionRuntime::from_capture(
            model_manager.clone(),
            oobe_manager.clone(),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );

        // Verify driver state initialized to Idle
        {
            let d = runtime.driver.lock().await;
            assert_eq!(d.current_state(), AcousticState::Idle);
        }

        // Verify audio pipeline playback sends frames into rx
        runtime
            .playback()
            .play_earcon(EarconKind::Wake)
            .await
            .unwrap();
        let frame = playback_rx.recv().await;
        assert!(frame.is_some());
        assert!(!frame.unwrap().is_empty());

        // Verify worker task spawns and pumps cleanly
        let handle = runtime.start_worker();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert!(!handle.is_finished());

        handle.abort();
    }

    struct MockSpeech(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for MockSpeech {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn test_progress_must_not_block_final_result_of_same_task() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceSessionPhase};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let worker = runtime.start_worker();
        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "task-001".into(),
                notification_id: Some(format!("{kind}:task-001")),
                attempt_id: Some(format!("{kind}:task-001:1")),
                goal: None,
                text: text.into(),
            }
        };

        // 1. Progress announcement accepted
        runtime.announce_task_result(notice(&runtime, "progress", "正在调用搜索工具。")).await.unwrap();

        // Wait until idle
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while runtime.snapshot().phase != VoiceSessionPhase::Idle {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        // 2. Final result announcement for same command_id must succeed and not be blocked as conflict
        let result = runtime.announce_task_result(notice(&runtime, "result", "这是最终搜索结果。")).await;
        worker.abort();

        assert_eq!(result, Ok(()), "different notification_id for the same command_id must succeed");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // 3. Status query by attempt_id, notification_id, command_id
        assert!(runtime.notification_status("result:task-001:1").await.is_some());
        assert!(runtime.notification_status("result:task-001").await.is_some());
        assert!(runtime.notification_status("task-001").await.is_some());
    }

    #[tokio::test]
    async fn test_explicit_replay_must_create_a_receipt_and_play_again() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceSessionPhase};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let worker = runtime.start_worker();
        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "same-task".into(),
                notification_id: Some(format!("{kind}:same-task")),
                attempt_id: Some(format!("{kind}:same-task:1")),
                goal: None,
                text: text.into(),
            }
        };

        let first = notice(&runtime, "result", "这是最终结果。");
        runtime.announce_task_result(first).await.unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while runtime.snapshot().phase != VoiceSessionPhase::Idle {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        let mut replay = notice(&runtime, "result", "这是最终结果。");
        replay.attempt_id = Some("result:same-task:2".into());
        runtime.announce_task_result(replay).await.unwrap();
        worker.abort();

        assert!(
            runtime.notification_status("result:same-task:2").await.is_some(),
            "accepted explicit replay must have its own attempt receipt"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_changed_payload_of_same_logical_notification_must_conflict() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::AtomicUsize;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls)),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "same-task".into(),
                notification_id: Some(format!("{kind}:same-task")),
                attempt_id: Some(format!("{kind}:same-task:1")),
                goal: None,
                text: text.into(),
            }
        };

        runtime.announce_task_result(notice(&runtime, "result", "原始结果。")).await.unwrap();
        let mut changed = notice(&runtime, "result", "不同的新结果。");
        changed.attempt_id = Some("changed-attempt".into());
        assert_eq!(runtime.announce_task_result(changed).await, Err("task_result_conflict"));
    }

    struct FailingTts;
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for FailingTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            Err(nomifun_voice_terminal::tts::TtsError::Synthesis("mock_network_down".into()))
        }
    }

    #[tokio::test]
    async fn test_task_result_plays_chime_and_opens_four_second_follow_up_window() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceCueKind};
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "light",
                    "volume": 80,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let mut rx = runtime.event_sender().subscribe();
        let snapshot = runtime.snapshot();
        let notice = VoiceTaskResultAnnouncement {
            scope: VoiceSessionControlRequest {
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                turn_id: snapshot.turn_id,
            },
            command_id: "cmd-step5".into(),
            notification_id: Some("notif-step5".into()),
            attempt_id: Some("attempt-step5:1".into()),
            goal: Some("查询北京天气".into()),
            text: "北京今天晴，气温 15 到 25 度。".into(),
        };

        let result = runtime.announce_task_result(notice).await;
        assert_eq!(result, Ok(()));

        let mut saw_chime = false;
        let mut saw_followup_window = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                VoiceCompanionEvent::Feedback { diagnostic } if diagnostic.kind == VoiceCueKind::Recognized => {
                    saw_chime = true;
                }
                VoiceCompanionEvent::FollowUpWindow { timeout_secs } => {
                    assert_eq!(timeout_secs, 4, "follow-up window must strictly be 4 seconds");
                    saw_followup_window = true;
                }
                _ => {}
            }
        }
        assert!(saw_chime, "task completion chime (Recognized) must play before speaking result");
        assert!(saw_followup_window, "follow-up window of 4 seconds must be announced");
    }

    #[tokio::test]
    async fn test_task_result_tts_failure_plays_error_cue_fallback() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceCueKind};
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(FailingTts),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "light",
                    "volume": 80,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let mut rx = runtime.event_sender().subscribe();
        let snapshot = runtime.snapshot();
        let notice = VoiceTaskResultAnnouncement {
            scope: VoiceSessionControlRequest {
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                turn_id: snapshot.turn_id,
            },
            command_id: "cmd-tts-fail".into(),
            notification_id: Some("notif-tts-fail".into()),
            attempt_id: Some("attempt-tts-fail:1".into()),
            goal: Some("测试TTS失败".into()),
            text: "这是一条无法合成的语音消息。".into(),
        };

        let result = runtime.announce_task_result(notice).await;
        assert_eq!(result, Ok(()));

        let mut saw_error_cue = false;
        let mut saw_error_event = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                VoiceCompanionEvent::Feedback { diagnostic } if diagnostic.kind == VoiceCueKind::Error => {
                    saw_error_cue = true;
                }
                VoiceCompanionEvent::Error { stage, code } => {
                    assert_eq!(stage, "playback");
                    assert_eq!(code, "tts_failed");
                    saw_error_event = true;
                }
                _ => {}
            }
        }
        assert!(saw_error_event, "error event must be emitted on TTS synthesis failure");
        assert!(saw_error_cue, "hardcoded local error cue must play on TTS failure");
    }
}

//! AI Mate Voice Companion Runtime & Pipeline Assembly
//!
//! Ties together:
//! - `nomifun_audio::voice_pipeline::VoiceCapturePipeline`
//! - `nomifun_voice_terminal::playback::PlaybackPipeline`
//! - `nomifun_voice_terminal::driver::VoiceSessionDriver`
//! - `nomifun_voice_terminal::router::QwenVoiceRouter`
//! - `nomifun_voice_models::ModelManager`
//! - `crate::oobe::OobeManager`

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

use nomifun_audio::capture::CaptureReadResult;
use nomifun_audio::kws::KwsEngine;
use nomifun_audio::vad::{EnergyVad, VadBackend, VadConfig};
use nomifun_audio::EarconKind;
use nomifun_audio::voice_pipeline::{PipelineConfig, SpscProducer, VoiceCapturePipeline};
use nomifun_voice_models::ModelManager;
use nomifun_voice_terminal::contract::{AcousticEvent, AcousticState, IntentType};
use nomifun_voice_terminal::driver::{VoiceRouter, VoiceSessionDriver};
use nomifun_voice_terminal::playback::{ChannelAudioSink, PlaybackPipeline};
use nomifun_voice_terminal::router::QwenVoiceRouter;

use crate::oobe::OobeManager;

/// Real-time voice companion event emitted over SSE for desktop dashboard observability.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum VoiceCompanionEvent {
    WakeHit { keyword: String },
    Listening,
    SpeechRecognized { text: String },
    IntentDecided { intent: String, reply: Option<String> },
    TtsSpeaking { text: String },
    FollowUpWindow { timeout_secs: u32 },
    Idle,
}

/// Production voice companion pipeline assembly and runtime controller.
pub struct VoiceCompanionRuntime {
    model_manager: Arc<ModelManager>,
    oobe_manager: Arc<OobeManager>,
    driver: Arc<Mutex<VoiceSessionDriver>>,
    playback: Arc<PlaybackPipeline>,
    router: Arc<QwenVoiceRouter>,
    capture_pipeline: Arc<Mutex<VoiceCapturePipeline>>,
    spsc_producer: SpscProducer,
    event_tx: tokio::sync::broadcast::Sender<VoiceCompanionEvent>,
    /// Active RAII guard holding the cpal input stream open. Dropping this closes the mic.
    #[allow(dead_code)]
    mic_source: Option<Arc<nomifun_audio::MicSource>>,
}

impl VoiceCompanionRuntime {
    /// Assembles the complete voice pipeline with capture, playback, session driver, and router.
    pub fn assemble(
        model_manager: Arc<ModelManager>,
        oobe_manager: Arc<OobeManager>,
    ) -> (Self, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let (capture_pipeline, producer, mic_source) = match nomifun_audio::MicSource::new(16_000) {
            Ok(mic) => {
                info!("Hardware microphone successfully initialized for AI Mate");
                let pipeline = mic.pipeline();
                let mic_arc = Arc::new(mic);
                let (_, prod) = VoiceCapturePipeline::new(PipelineConfig::default());
                (pipeline, prod, Some(mic_arc))
            }
            Err(err) => {
                warn!(error = %err, "No hardware microphone available; using standalone capture pipeline");
                let (pipeline, prod) = VoiceCapturePipeline::new(PipelineConfig::default());
                (Arc::new(Mutex::new(pipeline)), prod, None)
            }
        };

        let (playback_tx, playback_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let sink = Arc::new(ChannelAudioSink::new(playback_tx));
        let playback = Arc::new(PlaybackPipeline::new(sink));

        let driver = Arc::new(Mutex::new(VoiceSessionDriver::new()));
        let router = Arc::new(QwenVoiceRouter::new());
        let (event_tx, _) = tokio::sync::broadcast::channel::<VoiceCompanionEvent>(128);

        (
            Self {
                model_manager,
                oobe_manager,
                driver,
                playback,
                router,
                capture_pipeline,
                spsc_producer: producer,
                event_tx,
                mic_source,
            },
            playback_rx,
        )
    }

    pub fn event_sender(&self) -> tokio::sync::broadcast::Sender<VoiceCompanionEvent> {
        self.event_tx.clone()
    }

    pub fn driver(&self) -> Arc<Mutex<VoiceSessionDriver>> {
        Arc::clone(&self.driver)
    }

    pub fn playback(&self) -> Arc<PlaybackPipeline> {
        Arc::clone(&self.playback)
    }

    pub fn router(&self) -> Arc<QwenVoiceRouter> {
        Arc::clone(&self.router)
    }

    pub fn capture_pipeline(&self) -> Arc<Mutex<VoiceCapturePipeline>> {
        Arc::clone(&self.capture_pipeline)
    }

    pub fn spsc_producer(&self) -> &SpscProducer {
        &self.spsc_producer
    }

    /// Check if device is paired and models are ready.
    pub async fn is_operational(&self) -> bool {
        let paired = self.oobe_manager.is_paired().await;
        let models_ready = self.model_manager.get_status().await.state.is_ready();
        paired && models_ready
    }

    /// Handle far-field wake event ("Hey Flowy").
    pub async fn handle_wake(&self) {
        if !self.oobe_manager.is_paired().await {
            warn!("Wake received but device is not yet paired via OOBE");
            let _ = self.playback.play_earcon(EarconKind::Alert).await;
            return;
        }

        if !self.model_manager.get_status().await.state.is_ready() {
            warn!("Wake received but voice models are not yet installed or verified");
            let _ = self.playback.play_earcon(EarconKind::Alert).await;
            return;
        }

        let mut driver = self.driver.lock().await;
        if let Ok(next) = driver.handle_event(AcousticEvent::WakeHit) {
            info!(state = ?next, "Wake hit accepted, transitioning to listening");
            let _ = self.event_tx.send(VoiceCompanionEvent::WakeHit {
                keyword: "Hey Flowy".to_string(),
            });
            let _ = self.event_tx.send(VoiceCompanionEvent::Listening);
            let _ = self.playback.play_earcon(EarconKind::Wake).await;
        }
    }

    /// Internal helper to run ASR transcription, Qwen intent routing, TTS synthesis and FSM dispatch.
    async fn process_utterance(
        utterance_pcm: &[f32],
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        router: &Arc<QwenVoiceRouter>,
        playback: &Arc<PlaybackPipeline>,
        event_tx: &tokio::sync::broadcast::Sender<VoiceCompanionEvent>,
    ) {
        let text_opt = nomifun_audio::stt::transcribe_test_pcm(utterance_pcm, 16000).await;
        let decision = if let Some(ref text) = text_opt {
            info!(text = %text, "ASR transcription produced text");
            let _ = event_tx.send(VoiceCompanionEvent::SpeechRecognized { text: text.clone() });
            let clean_input = nomifun_audio::strip_wake_phrase(text);
            let route_input = if clean_input.is_empty() {
                "你好"
            } else {
                &clean_input
            };
            match router.route(route_input).await {
                Ok(dec) => Some(dec),
                Err(err) => {
                    warn!(error = %err, "Routing error");
                    None
                }
            }
        } else {
            None
        };

        let mut d = driver.lock().await;
        if let Some(dec) = decision {
            let _ = event_tx.send(VoiceCompanionEvent::IntentDecided {
                intent: format!("{:?}", dec.intent_type),
                reply: dec.reply_tts.clone(),
            });
            match dec.intent_type {
                IntentType::Ignore => {
                    let _ = d.handle_event(AcousticEvent::IntentIgnore);
                    drop(d);
                    let _ = playback.play_earcon(EarconKind::Sleep).await;
                    let _ = event_tx.send(VoiceCompanionEvent::Idle);
                }
                IntentType::Chat => {
                    let _ = d.handle_event(AcousticEvent::IntentChat);
                    drop(d);
                    let reply_text = dec.reply_tts.as_deref().unwrap_or("你好！有什么我可以帮你的吗？");
                    info!(reply = %reply_text, "Speaking Chat reply via Edge-TTS");
                    let _ = event_tx.send(VoiceCompanionEvent::TtsSpeaking {
                        text: reply_text.to_string(),
                    });
                    let tts = nomifun_voice_terminal::tts::EdgeTtsProvider::new();
                    if let Err(err) = playback.speak_text(&tts, reply_text, "").await {
                        warn!(error = %err, "TTS speak_text failed, playing Done earcon fallback");
                        let _ = playback.play_earcon(EarconKind::Done).await;
                    }
                    let mut d2 = driver.lock().await;
                    let _ = d2.handle_event(AcousticEvent::PlaybackDone);
                    let _ = event_tx.send(VoiceCompanionEvent::FollowUpWindow { timeout_secs: 4 });
                }
                IntentType::TaskControl => {
                    let _ = d.handle_event(AcousticEvent::IntentTaskControl);
                    drop(d);
                    if let Some(ref ack) = dec.spoken_ack {
                        let _ = event_tx.send(VoiceCompanionEvent::TtsSpeaking {
                            text: ack.to_string(),
                        });
                        let tts = nomifun_voice_terminal::tts::EdgeTtsProvider::new();
                        let _ = playback.speak_text(&tts, ack, "").await;
                    } else {
                        let _ = playback.play_earcon(EarconKind::Done).await;
                    }
                    let mut d2 = driver.lock().await;
                    let _ = d2.handle_event(AcousticEvent::PlaybackDone);
                    let _ = event_tx.send(VoiceCompanionEvent::FollowUpWindow { timeout_secs: 4 });
                }
                IntentType::AgentTask => {
                    if dec.requires_confirmation == Some(true) {
                        let _ = d.handle_event(AcousticEvent::ConfirmationRequired);
                        drop(d);
                        let prompt = dec.reply_tts.as_deref().unwrap_or("确认要执行该危险操作吗？");
                        let _ = event_tx.send(VoiceCompanionEvent::TtsSpeaking {
                            text: prompt.to_string(),
                        });
                        let tts = nomifun_voice_terminal::tts::EdgeTtsProvider::new();
                        let _ = playback.speak_text(&tts, prompt, "").await;
                        let mut d2 = driver.lock().await;
                        let _ = d2.handle_event(AcousticEvent::PromptDone);
                    } else {
                        let _ = d.handle_event(AcousticEvent::IntentAgentTask);
                        drop(d);
                        if let Some(ref ack) = dec.spoken_ack {
                            let _ = event_tx.send(VoiceCompanionEvent::TtsSpeaking {
                                text: ack.to_string(),
                            });
                            let tts = nomifun_voice_terminal::tts::EdgeTtsProvider::new();
                            let _ = playback.speak_text(&tts, ack, "").await;
                        } else {
                            let _ = playback.play_earcon(EarconKind::Done).await;
                        }
                        let mut d2 = driver.lock().await;
                        let _ = d2.handle_event(AcousticEvent::TaskAccepted);
                        let _ = event_tx.send(VoiceCompanionEvent::Idle);
                    }
                }
            }
        } else {
            let _ = d.handle_event(AcousticEvent::IntentIgnore);
            drop(d);
            let _ = playback.play_earcon(EarconKind::Sleep).await;
            let _ = event_tx.send(VoiceCompanionEvent::Idle);
        }
    }

    /// Starts the background audio event pump and watchdog loop.
    pub fn start_worker(&self) -> tokio::task::JoinHandle<()> {
        let driver = Arc::clone(&self.driver);
        let capture = Arc::clone(&self.capture_pipeline);
        let playback = Arc::clone(&self.playback);
        let router = Arc::clone(&self.router);
        let oobe = Arc::clone(&self.oobe_manager);
        let models = Arc::clone(&self.model_manager);
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut kws = KwsEngine::load_default().unwrap_or_else(|_| {
                KwsEngine::new(nomifun_audio::kws::KwsEngineConfig::default()).unwrap()
            });
            let mut vad = EnergyVad::new(VadConfig::default());
            let mut utterance_pcm = Vec::<f32>::new();
            let mut speech_started = false;
            let mut listen_start: Option<std::time::Instant> = None;

            loop {
                // 1. Check FSM watchdog timers (e.g. 8-second confirmation timeout)
                {
                    let mut d = driver.lock().await;
                    if let Ok(Some(next)) = d.check_watchdog() {
                        info!(state = ?next, "Watchdog timeout transitioned FSM state");
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
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        continue;
                    }
                    CaptureReadResult::StreamClosed | CaptureReadResult::DeviceError(_) => {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };

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
                                if utterance_pcm.len() >= 6_400 {
                                    let snippet = std::mem::take(&mut utterance_pcm);
                                    if let Some(text) = nomifun_audio::stt::transcribe_test_pcm(&snippet, 16_000).await {
                                        if nomifun_audio::is_wake_phrase_match(&text) {
                                            info!(text = %text, "Cascade wake hit accepted via SenseVoice");
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
                            } else if !models.get_status().await.state.is_ready() {
                                warn!("Wake received but voice models are not yet installed");
                                let _ = playback.play_earcon(EarconKind::Alert).await;
                            } else {
                                let mut d = driver.lock().await;
                                if let Ok(next) = d.handle_event(AcousticEvent::WakeHit) {
                                    info!(state = ?next, "Wake accepted, transitioning to Listening");
                                    drop(d);
                                    let _ = event_tx.send(VoiceCompanionEvent::WakeHit { keyword });
                                    let _ = event_tx.send(VoiceCompanionEvent::Listening);
                                    let _ = playback.play_earcon(EarconKind::Wake).await;
                                    vad.reset();
                                    utterance_pcm.clear();
                                    speech_started = false;
                                    listen_start = Some(std::time::Instant::now());
                                }
                            }
                        }
                    }

                    AcousticState::Listening => {
                        // User speech capture: accumulate PCM samples
                        utterance_pcm.extend_from_slice(&chunk);

                        // Enforce 6-second silence timeout if no speech detected
                        if !speech_started
                            && let Some(start) = listen_start
                            && start.elapsed() > std::time::Duration::from_secs(6)
                        {
                            warn!("Listening timed out (6s silence)");
                            let mut d = driver.lock().await;
                            let _ = d.handle_event(AcousticEvent::ListenTimeout);
                            let _ = event_tx.send(VoiceCompanionEvent::Idle);
                            utterance_pcm.clear();
                            kws.reset();
                            vad.reset();
                            continue;
                        }

                        // Feed frame to VAD detector
                        let is_active = vad.process_frame(&chunk);
                        if is_active {
                            speech_started = true;
                        } else if speech_started {
                            // User paused / end of utterance detected
                            info!(samples = utterance_pcm.len(), "SpeechEnd detected by VAD, transitioning to Deciding");
                            speech_started = false;
                            listen_start = None;

                            let mut d = driver.lock().await;
                            if let Ok(AcousticState::Deciding) = d.handle_event(AcousticEvent::SpeechEnd) {
                                drop(d);
                                Self::process_utterance(
                                    &utterance_pcm,
                                    &driver,
                                    &router,
                                    &playback,
                                    &event_tx,
                                )
                                .await;
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
                            let _ = event_tx.send(VoiceCompanionEvent::FollowUpWindow { timeout_secs: 4 });
                        }

                        let is_active = vad.process_frame(&chunk);
                        if is_active {
                            speech_started = true;
                            utterance_pcm.extend_from_slice(&chunk);
                        } else if speech_started {
                            // User followed up with another utterance during the 4s window!
                            info!(samples = utterance_pcm.len(), "Follow-up speech ended, transitioning to Deciding");
                            speech_started = false;
                            listen_start = None;

                            let mut d = driver.lock().await;
                            if let Ok(AcousticState::Deciding) = d.handle_event(AcousticEvent::FollowUpSpeech) {
                                drop(d);
                                Self::process_utterance(
                                    &utterance_pcm,
                                    &driver,
                                    &router,
                                    &playback,
                                    &event_tx,
                                )
                                .await;
                                utterance_pcm.clear();
                                kws.reset();
                                vad.reset();
                            }
                        } else if let Some(start) = listen_start {
                            if start.elapsed() > std::time::Duration::from_secs(4) {
                                info!("FollowUpListening timed out (4s silence), returning to Idle");
                                let mut d = driver.lock().await;
                                let _ = d.handle_event(AcousticEvent::FollowUpTimeout);
                                listen_start = None;
                                vad.reset();
                                let _ = event_tx.send(VoiceCompanionEvent::Idle);
                            }
                        }
                    }

                    _ => {}
                }
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

        let (runtime, mut playback_rx) = VoiceCompanionRuntime::assemble(
            model_manager.clone(),
            oobe_manager.clone(),
        );

        // Verify driver state initialized to Idle
        {
            let d = runtime.driver.lock().await;
            assert_eq!(d.current_state(), AcousticState::Idle);
        }

        // Verify audio pipeline playback sends frames into rx
        runtime.playback().play_earcon(EarconKind::Wake).await.unwrap();
        let frame = playback_rx.recv().await;
        assert!(frame.is_some());
        assert!(!frame.unwrap().is_empty());

        // Verify worker task spawns and pumps cleanly
        let handle = runtime.start_worker();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert!(!handle.is_finished());

        // Push test silent PCM frames through SPSC producer
        let silent_chunk = vec![0.0f32; 320];
        runtime.spsc_producer().push(&silent_chunk);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        handle.abort();
    }
}

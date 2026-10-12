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

/// Production voice companion pipeline assembly and runtime controller.
pub struct VoiceCompanionRuntime {
    model_manager: Arc<ModelManager>,
    oobe_manager: Arc<OobeManager>,
    driver: Arc<Mutex<VoiceSessionDriver>>,
    playback: Arc<PlaybackPipeline>,
    router: Arc<QwenVoiceRouter>,
    capture_pipeline: Arc<Mutex<VoiceCapturePipeline>>,
    spsc_producer: SpscProducer,
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

        (
            Self {
                model_manager,
                oobe_manager,
                driver,
                playback,
                router,
                capture_pipeline,
                spsc_producer: producer,
                mic_source,
            },
            playback_rx,
        )
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
            let _ = self.playback.play_earcon(EarconKind::Wake).await;
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
                        if let Some(wake_res) = kws.process_pcm_chunk(&chunk) {
                            info!(keyword = %wake_res.keyword, score = wake_res.score, "Wake hit detected");
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

                                // Perform ASR transcription and intent routing
                                let text_opt = nomifun_audio::stt::transcribe_test_pcm(&utterance_pcm, 16000).await;
                                utterance_pcm.clear();

                                let decision = if let Some(ref text) = text_opt {
                                    info!(text = %text, "ASR transcription produced text");
                                    match router.route(text).await {
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
                                    match dec.intent_type {
                                        IntentType::Ignore => {
                                            let _ = d.handle_event(AcousticEvent::IntentIgnore);
                                        }
                                        IntentType::Chat => {
                                            let _ = d.handle_event(AcousticEvent::IntentChat);
                                            drop(d);
                                            let _ = playback.play_earcon(EarconKind::Done).await;
                                            let mut d2 = driver.lock().await;
                                            let _ = d2.handle_event(AcousticEvent::PlaybackDone);
                                        }
                                        IntentType::TaskControl => {
                                            let _ = d.handle_event(AcousticEvent::IntentTaskControl);
                                            drop(d);
                                            let _ = playback.play_earcon(EarconKind::Done).await;
                                            let mut d2 = driver.lock().await;
                                            let _ = d2.handle_event(AcousticEvent::PlaybackDone);
                                        }
                                        IntentType::AgentTask => {
                                            if dec.requires_confirmation == Some(true) {
                                                let _ = d.handle_event(AcousticEvent::ConfirmationRequired);
                                                drop(d);
                                                let _ = playback.play_earcon(EarconKind::Alert).await;
                                                let mut d2 = driver.lock().await;
                                                let _ = d2.handle_event(AcousticEvent::PromptDone);
                                            } else {
                                                let _ = d.handle_event(AcousticEvent::IntentAgentTask);
                                                drop(d);
                                                let _ = playback.play_earcon(EarconKind::Done).await;
                                                let mut d2 = driver.lock().await;
                                                let _ = d2.handle_event(AcousticEvent::TaskAccepted);
                                            }
                                        }
                                    }
                                } else {
                                    let _ = d.handle_event(AcousticEvent::IntentIgnore);
                                }
                                kws.reset();
                                vad.reset();
                            }
                        }
                    }

                    AcousticState::FollowUpListening => {
                        // 4-second follow-up window
                        if let Some(start) = listen_start {
                            if start.elapsed() > std::time::Duration::from_secs(4) {
                                let mut d = driver.lock().await;
                                let _ = d.handle_event(AcousticEvent::FollowUpTimeout);
                                listen_start = None;
                            }
                        } else {
                            listen_start = Some(std::time::Instant::now());
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

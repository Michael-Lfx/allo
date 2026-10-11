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

use nomifun_audio::EarconKind;
use nomifun_audio::voice_pipeline::{PipelineConfig, SpscProducer, VoiceCapturePipeline};
use nomifun_voice_models::ModelManager;
use nomifun_voice_terminal::contract::AcousticEvent;
use nomifun_voice_terminal::driver::VoiceSessionDriver;
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
    _capture_pipeline: Arc<Mutex<VoiceCapturePipeline>>,
    _spsc_producer: SpscProducer,
}

impl VoiceCompanionRuntime {
    /// Assembles the complete voice pipeline with capture, playback, session driver, and router.
    pub fn assemble(
        model_manager: Arc<ModelManager>,
        oobe_manager: Arc<OobeManager>,
    ) -> (Self, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let (capture_pipeline, producer) = VoiceCapturePipeline::new(PipelineConfig::default());

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
                _capture_pipeline: Arc::new(Mutex::new(capture_pipeline)),
                _spsc_producer: producer,
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

    /// Starts the background watchdog and maintenance loop.
    pub fn start_worker(&self) -> tokio::task::JoinHandle<()> {
        let driver = Arc::clone(&self.driver);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
            loop {
                interval.tick().await;
                let mut d = driver.lock().await;
                if let Ok(Some(next)) = d.check_watchdog() {
                    info!(state = ?next, "Watchdog timeout transitioned FSM state");
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

        // Verify worker task spawns cleanly
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        worker.abort();
    }
}

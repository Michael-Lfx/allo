//! Real-model diagnostic for the configured 30s idle window. The historical
//! 6.5s replay remains unchanged, so this does not mask its baseline failure.
#![cfg(feature = "voice-skeleton")]
use voice_companion::{oobe::OobeManager, voice::VoiceCompanionRuntime};
use nomifun_audio::voice_pipeline::{PipelineConfig, VoiceCapturePipeline};
use nomifun_voice_terminal::driver::StubVoiceRouter;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
fn models() -> PathBuf { PathBuf::from(std::env::var_os("VOICE_REPLAY_MODEL_DIR").expect("isolated model dir required")) }
#[tokio::test]
#[ignore = "real speech detector silence control through the production worker"]
async fn configured_idle_timeout_ends_silent_input() {
    let asr = Arc::new(
        nomifun_audio::stt::try_load_sherpa(Some(&models()))
            .unwrap()
            .unwrap(),
    );
    let router = Arc::new(StubVoiceRouter::chat("must not reply to silence"));
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(nomifun_voice_models::ModelManager::new(models())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        asr,
        router.clone(),
    );
    let worker = runtime.start_worker();
    tokio::time::timeout(Duration::from_secs(15), async {
        while !runtime
            .snapshot()
            .audio
            .is_some_and(|audio| audio.vad_ready)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("installed engines must be ready before the wake window begins");
    runtime.handle_wake().await.unwrap();
    let started = tokio::time::Instant::now();
    let timeout = runtime.feedback().idle_timeout_secs;
    assert_eq!(timeout, 30, "production default changed; reassess this diagnostic");
    println!("CONFIGURED_IDLE_SECONDS={timeout}");
    for index in 0..(u64::from(timeout) * 50 + 100) {
        producer.push(&[0.0; 320]);
        tokio::time::sleep_until(started + Duration::from_millis((index + 1) * 20)).await;
    }
    let snapshot = runtime.snapshot();
    worker.abort();
    assert_eq!(
        snapshot.phase,
        nomifun_api_types::voice::VoiceSessionPhase::Idle
    );
    assert!(snapshot.error.is_none());
    assert_eq!(
        snapshot.ended_reason,
        Some(nomifun_api_types::voice::VoiceSessionEndReason::InitialSilence)
    );
    assert_eq!(router.call_count(), 0);
    assert!(snapshot.conversation.is_empty());
}

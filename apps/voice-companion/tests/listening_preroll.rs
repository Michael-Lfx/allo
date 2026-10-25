#![cfg(feature = "voice-skeleton")]

use nomifun_audio::{
    AudioChannel, SttCallback,
    voice_pipeline::{PipelineConfig, VoiceCapturePipeline},
};
use nomifun_voice_models::ModelManager;
use nomifun_voice_terminal::driver::StubVoiceRouter;
use std::{
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::sync::Mutex;
use voice_companion::{oobe::OobeManager, voice::VoiceCompanionRuntime};

struct RecordingStt(Arc<StdMutex<Vec<(usize, usize)>>>);
#[async_trait::async_trait]
impl SttCallback for RecordingStt {
    async fn transcribe(&self, _: AudioChannel, samples: Vec<f32>, _: u32) -> Option<String> {
        self.0.lock().unwrap().push((
            samples.len(),
            samples.iter().filter(|x| x.abs() > 0.01).count(),
        ));
        None
    }
}

#[tokio::test]
async fn long_initial_silence_does_not_cut_off_the_first_spoken_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig {
        chunk_size: 1600,
        capacity_samples: 1_100_000,
        backlog_limit_ms: 60_000,
        ..Default::default()
    });
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        Arc::new(ModelManager::new(dir.path())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        Arc::new(RecordingStt(seen.clone())),
        Arc::new(StubVoiceRouter::chat("unused")),
    );
    runtime
        .update_feedback(
            nomifun_voice_models::VoiceFeedbackConfig {
                mode: nomifun_voice_models::VoiceFeedbackMode::Off,
                volume: 0,
                speak_replies: false,
            }
            .into(),
        )
        .unwrap();
    let worker = runtime.start_worker();
    runtime.handle_wake().await.unwrap();
    producer.push(&vec![0.0; 16_000 * 31]);
    // Replay PCM time rapidly; this still traverses the real capture worker.
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            producer.push(&[0.0; 1600]);
            if runtime
                .snapshot()
                .audio
                .as_ref()
                .is_some_and(|d| d.frames_received >= 310)
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    producer.push(&vec![0.1; 16_000]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let premature = seen.lock().unwrap().clone();
    // Supply actual trailing silence so a fixed worker must eventually deliver
    // the entire sentence and return to Listening after an empty ASR result.
    for _ in 0..12 {
        producer.push(&[0.0; 1600]);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let observed = seen.lock().unwrap().clone();
    worker.abort();
    let _ = worker.await;
    producer.close();
    assert!(
        premature.is_empty(),
        "continuous speech was cut at VAD onset: {premature:?}"
    );
    assert_eq!(
        observed.len(),
        1,
        "one sentence must have one ASR invocation"
    );
    assert_eq!(
        observed[0].1, 16_000,
        "VAD onset must not drop the first frames"
    );
    assert!(
        observed[0].0 < 16_000 * 4,
        "leading silence must remain bounded"
    );
    assert_eq!(
        runtime.snapshot().phase,
        nomifun_voice_models::VoiceSessionPhase::Listening
    );
}

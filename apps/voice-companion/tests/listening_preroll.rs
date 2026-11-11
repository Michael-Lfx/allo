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

struct PanickingStt;
#[async_trait::async_trait]
impl SttCallback for PanickingStt {
    async fn transcribe(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Option<String> {
        panic!("controlled ASR failure")
    }
}

struct ReportedFailureStt;
#[async_trait::async_trait]
impl SttCallback for ReportedFailureStt {
    async fn transcribe(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Option<String> { None }
    async fn transcribe_checked(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Result<Option<String>, String> {
        Err("controlled engine failure".into())
    }
}

struct RetryStt(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl SttCallback for RetryStt {
    async fn transcribe(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Option<String> {
        (self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0).then(|| "重试成功".into())
    }
}

#[tokio::test]
async fn worker_reports_empty_recognition_and_asr_failure_without_ending_the_call() {
    for (transcriber, expected) in [
        (Arc::new(nomifun_audio::stt::NullSttCallback) as Arc<dyn SttCallback>, "asr_empty_result"),
        (Arc::new(PanickingStt) as Arc<dyn SttCallback>, "asr_failed"),
        (Arc::new(ReportedFailureStt) as Arc<dyn SttCallback>, "asr_failed"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(dir.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), transcriber, Arc::new(StubVoiceRouter::chat("unused")),
        );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        let worker = runtime.start_worker();
        runtime.handle_wake().await.unwrap();
        let session = runtime.snapshot().session_id;
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let snapshot = runtime.snapshot();
        worker.abort();
        producer.close();
        assert_eq!(snapshot.session_id, session);
        assert_eq!(snapshot.phase, nomifun_voice_models::VoiceSessionPhase::Listening);
        assert_eq!(snapshot.error.as_ref().map(|error| error.code.as_str()), Some(expected));
        assert!(snapshot.conversation.is_empty(), "failed recognition must not create an empty chat row");
    }
}

#[tokio::test]
async fn worker_snapshot_contains_the_recognized_turn_and_chat_reply() {
    let dir = tempfile::tempdir().unwrap();
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(ModelManager::new(dir.path())), Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)), Arc::new(nomifun_audio::stt::FakeSttCallback::always("固定测试问题")),
        Arc::new(StubVoiceRouter::chat("固定测试回复")),
    );
    runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
    let mut events = runtime.event_sender().subscribe();
    let worker = runtime.start_worker();
    runtime.handle_wake().await.unwrap();
    for index in 0..75 {
        assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let snapshot = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = runtime.snapshot();
            if snapshot.assistant_text.as_deref() == Some("固定测试回复") { break snapshot; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    worker.abort();
    producer.close();
    assert_eq!(snapshot.user_text.as_deref(), Some("固定测试问题"));
    assert_eq!(snapshot.conversation.len(), 1);
    assert_eq!(snapshot.conversation[0].session_id, snapshot.session_id);
    assert_eq!(Some(&snapshot.conversation[0].turn_id), snapshot.turn_id.as_ref());
    let recognized = std::iter::from_fn(|| events.try_recv().ok()).filter(|event| matches!(event, voice_companion::voice::VoiceCompanionEvent::SpeechRecognized { text } if text == "固定测试问题")).count();
    assert_eq!(recognized, 1);
    if let Some(path) = std::env::var_os("VOICE_WORKER_REPLAY_SNAPSHOT") {
        std::fs::write(path, serde_json::to_vec_pretty(&snapshot).unwrap()).unwrap();
    }
}

#[tokio::test]
async fn next_speech_clears_a_recoverable_asr_error_in_the_same_session() {
    let dir = tempfile::tempdir().unwrap();
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(ModelManager::new(dir.path())), Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)), Arc::new(RetryStt(std::sync::atomic::AtomicUsize::new(0))),
        Arc::new(StubVoiceRouter::chat("收到")),
    );
    runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
    let worker = runtime.start_worker();
    runtime.handle_wake().await.unwrap();
    let session = runtime.snapshot().session_id;
    for turn in 0..2 {
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if turn == 0 {
            let snapshot = runtime.snapshot();
            assert_eq!(snapshot.error.as_ref().map(|error| error.code.as_str()), Some("asr_empty_result"));
            assert_eq!(snapshot.session_id, session);
        }
    }
    let snapshot = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = runtime.snapshot();
            if snapshot.assistant_text.as_deref() == Some("收到") { break snapshot; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    worker.abort();
    producer.close();
    assert_eq!(snapshot.session_id, session);
    assert_eq!(snapshot.user_text.as_deref(), Some("重试成功"));
    assert!(snapshot.error.is_none());
    assert_eq!(snapshot.conversation.len(), 1);
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
                ..Default::default()
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

//! End-to-end voice regressions, including the actual Edge transport and playback pipeline.
use nomifun_voice_terminal::tts::{EdgeTtsProvider, TtsProvider};
use std::time::Duration;

#[tokio::test]
#[ignore = "explicit real Edge greeting synthesis with the desktop TLS feature graph; no physical output"]
async fn greeting_synthesis_with_desktop_tls_graph() {
    let started = std::time::Instant::now();
    let mut stream = EdgeTtsProvider::new()
        .synthesize_stream("你好呀，很高兴听到你的声音。", "")
        .await
        .unwrap();
    let wav = stream
        .recv()
        .await
        .expect("real Edge must return decoded audio");
    let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
    assert_eq!(rate, 24000);
    assert!(pcm.len() > 12000 && pcm.iter().any(|s| s.abs() > 0.001));
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/architecture/voice-agent/execution/T5-edge-greeting.wav");
    std::fs::write(path, wav).unwrap();
    println!(
        "Real Edge greeting: samples={}, rate={}, elapsed_ms={}",
        pcm.len(),
        rate,
        started.elapsed().as_millis()
    );
}

#[tokio::test]
async fn encrypted_tts_connection_returns_an_error_without_panicking() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("wss://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        drop(socket);
    });
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::spawn(async move {
            EdgeTtsProvider::new()
                .with_endpoint(endpoint)
                .synthesize_stream("你好呀，很高兴听到你的声音。", "")
                .await
        }),
    )
    .await
    .expect("closed TLS peer must not leave synthesis hanging");
    assert!(
        result.is_ok(),
        "the desktop production feature graph must not panic during TLS setup: {result:?}"
    );
    assert!(
        result.unwrap().is_err(),
        "closed TLS peer must be reported as an error"
    );
    server.await.unwrap();
}

#[cfg(feature = "voice-skeleton")]
mod flow {
    use super::*;
    use voice_companion::{oobe::OobeManager, voice::VoiceCompanionRuntime};
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use nomifun_audio::voice_pipeline::{PipelineConfig, SpscProducer, VoiceCapturePipeline};
    use nomifun_voice_terminal::{
        completion::HostDialogueClient,
        playback::{AudioSink, AudioSinkError, PlaybackPipeline},
        router::OpenAiCompatibleVoiceRouter,
        tts::TtsError,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    const REPLY: &str = "你好呀，很高兴听到你的声音。";
    const UNDERSTOOD: &str = "用户向我问好，希望得到友好的聊天回应。";
    #[derive(Clone, Copy)]
    enum TtsMode {
        Audio,
        Empty,
        NetworkFailure,
        Panic,
        Online,
        CancellationAware,
    }
    #[derive(Default)]
    struct DrainSink {
        frames: Mutex<Vec<Vec<u8>>>,
        hold_earcons: AtomicBool,
        draining: Notify,
        consumed: Notify,
    }
    #[async_trait]
    impl AudioSink for DrainSink {
        async fn write_frame(&self, bytes: Vec<u8>) -> Result<(), AudioSinkError> {
            self.frames.lock().unwrap().push(bytes);
            Ok(())
        }
        async fn flush(&self) -> Result<(), AudioSinkError> {
            self.frames.lock().unwrap().clear();
            Ok(())
        }
        async fn drain(&self) -> Result<(), AudioSinkError> {
            // Only synthesized replies need a controlled consumption receipt.
            // Existing built-in earcons are consumed immediately in this harness.
            if !self.hold_earcons.load(Ordering::SeqCst)
                && self.frames.lock().unwrap().last().is_some_and(|frame| {
                    let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(frame);
                    rate == 16000 && pcm.len() <= 4000
                })
            {
                return Ok(());
            }
            self.draining.notify_one();
            self.consumed.notified().await;
            Ok(())
        }
    }
    struct PanicTts;
    struct PendingTts {
        entered: Arc<Notify>,
        cancelled: Arc<AtomicBool>,
    }
    struct CancelNotice(Arc<AtomicBool>);
    impl Drop for CancelNotice {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl TtsProvider for PendingTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, TtsError> {
            let _notice = CancelNotice(self.cancelled.clone());
            self.entered.notify_one();
            std::future::pending().await
        }
    }
    #[async_trait]
    impl TtsProvider for PanicTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, TtsError> {
            panic!("fixture TTS panic");
        }
    }
    struct Flow {
        runtime: VoiceCompanionRuntime,
        producer: SpscProducer,
        sink: Arc<DrainSink>,
        synthesis_requested: Arc<Notify>,
        release_synthesis: Arc<Notify>,
        calls: Arc<AtomicUsize>,
        prompts: Arc<Mutex<Vec<String>>>,
        recordings: Arc<Mutex<Vec<Vec<f32>>>>,
        phases: Arc<Mutex<Vec<nomifun_api_types::voice::VoiceSessionPhase>>>,
        feedback: Arc<Mutex<Vec<nomifun_api_types::voice::VoiceFeedbackDiagnostic>>>,
        cancelled: Arc<AtomicBool>,
        online: bool,
        jobs: Vec<tokio::task::JoinHandle<()>>,
        _dir: tempfile::TempDir,
    }
    impl Drop for Flow {
        fn drop(&mut self) {
            self.producer.close();
            for job in &self.jobs {
                job.abort();
            }
        }
    }
    impl Flow {
        async fn new(mode: TtsMode) -> Self {
            let online = matches!(mode, TtsMode::Online);
            let empty_audio = matches!(mode, TtsMode::Empty);
            let calls = Arc::new(AtomicUsize::new(0));
            let counted = calls.clone();
            let prompts = Arc::new(Mutex::new(Vec::new()));
            let captured_prompts = prompts.clone();
            let app = axum::Router::new().route("/", axum::routing::post(move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<serde_json::Value>| {
                let counted = counted.clone(); let captured_prompts = captured_prompts.clone(); async move {
                    assert_eq!(headers.get("x-ai-mate-control-token").unwrap(), "fixture-control");
                    assert!(body["prompt"].as_str().unwrap().contains("你好"));
                    counted.fetch_add(1, Ordering::SeqCst);
                    captured_prompts.lock().unwrap().push(body["prompt"].as_str().unwrap().to_owned());
                    axum::Json(serde_json::json!({"content": serde_json::json!({"intent_type":"chat", "understood_request":UNDERSTOOD, "reply_tts":REPLY}).to_string()}))
                }
            }));
            let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let model_endpoint = format!("http://{}/", http.local_addr().unwrap());
            let model = tokio::spawn(async move {
                axum::serve(http, app).await.unwrap();
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let tts_endpoint = format!("ws://{}", listener.local_addr().unwrap());
            let synthesis_requested = Arc::new(Notify::new());
            let release_synthesis = Arc::new(Notify::new());
            let requested = synthesis_requested.clone();
            let released = release_synthesis.clone();
            let tts_app = axum::Router::new().route(
                "/",
                axum::routing::get(move |upgrade: axum::extract::ws::WebSocketUpgrade| {
                    let requested = requested.clone();
                    let released = released.clone();
                    async move {
                        if matches!(mode, TtsMode::NetworkFailure) {
                            return axum::response::IntoResponse::into_response(
                                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            );
                        }
                        upgrade.on_upgrade(move |mut ws| async move {
                            use axum::extract::ws::Message;
                            assert!(
                                ws.next()
                                    .await
                                    .unwrap()
                                    .unwrap()
                                    .into_text()
                                    .unwrap()
                                    .contains("Path:speech.config")
                            );
                            let ssml = ws.next().await.unwrap().unwrap().into_text().unwrap();
                            assert!(ssml.contains(REPLY));
                            requested.notify_one();
                            released.notified().await;
                            if !empty_audio {
                                let pcm: Vec<f32> =
                                    (0..4800).map(|i| 0.1 * (i as f32 * 0.13).sin()).collect();
                                let wav = nomifun_audio::pcm_to_wav(&pcm, 24000);
                                let headers = b"Path:audio\r\nContent-Type:audio/mpeg\r\n";
                                let mut frame = (headers.len() as u16).to_be_bytes().to_vec();
                                frame.extend_from_slice(headers);
                                frame.extend(wav);
                                let _ = ws.send(Message::Binary(frame.into())).await;
                            }
                            let _ = ws.send(Message::Text("Path:turn.end\r\n\r\n".into())).await;
                        })
                    }
                }),
            );
            let tts_job = tokio::spawn(async move {
                axum::serve(listener, tts_app).await.unwrap();
            });
            let dir = tempfile::tempdir().unwrap();
            let models = if online {
                std::path::PathBuf::from(
                    std::env::var_os("VOICE_REPLAY_MODEL_DIR")
                        .expect("set installed real model suite"),
                )
            } else {
                dir.path().to_path_buf()
            };
            let recordings = Arc::new(Mutex::new(Vec::new()));
            let stt: Arc<dyn nomifun_audio::recorder::SttCallback> = if online {
                Arc::new(
                    nomifun_audio::stt::try_load_sherpa(Some(&models))
                        .unwrap()
                        .unwrap(),
                )
            } else {
                Arc::new(RecordingStt(recordings.clone()))
            };
            let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
            let (runtime, _) = VoiceCompanionRuntime::from_capture(
                Arc::new(nomifun_voice_models::ModelManager::new(models)),
                Arc::new(OobeManager::new_bypassed()),
                Arc::new(tokio::sync::Mutex::new(capture)),
                stt,
                Arc::new(OpenAiCompatibleVoiceRouter::with_completion(Arc::new(
                    HostDialogueClient::new(model_endpoint, "fixture-control".into()).unwrap(),
                ))),
            );
            let sink = Arc::new(DrainSink::default());
            let cancelled = Arc::new(AtomicBool::new(false));
            let tts: Arc<dyn TtsProvider> = match mode {
                TtsMode::CancellationAware => Arc::new(PendingTts {
                    entered: synthesis_requested.clone(),
                    cancelled: cancelled.clone(),
                }),
                TtsMode::Panic => Arc::new(PanicTts),
                TtsMode::Online => Arc::new(EdgeTtsProvider::new()),
                _ => Arc::new(EdgeTtsProvider::new().with_endpoint(tts_endpoint)),
            };
            let runtime =
                runtime.with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())), tts);
            let phases = Arc::new(Mutex::new(Vec::new()));
            let observed = phases.clone();
            let feedback = Arc::new(Mutex::new(Vec::new()));
            let observed_feedback = feedback.clone();
            let mut events = runtime.event_sender().subscribe();
            let observer = tokio::spawn(async move {
                while let Ok(event) = events.recv().await {
                    if let nomifun_api_types::voice::VoiceCompanionEvent::Feedback { diagnostic } =
                        &event
                    {
                        observed_feedback.lock().unwrap().push(diagnostic.clone());
                    }
                    if let nomifun_api_types::voice::VoiceCompanionEvent::Snapshot { snapshot } =
                        event
                    {
                        let mut phases = observed.lock().unwrap();
                        if phases.last() != Some(&snapshot.phase) {
                            phases.push(snapshot.phase);
                        }
                    }
                }
            });
            let worker = runtime.start_worker();
            Self {
                runtime,
                producer,
                sink,
                synthesis_requested,
                release_synthesis,
                calls,
                prompts,
                recordings,
                phases,
                feedback,
                cancelled,
                online,
                jobs: vec![model, tts_job, observer, worker],
                _dir: dir,
            }
        }
        async fn utterance(&self) {
            // Simulates a KWS wake at its production acceptance entry, followed by PCM.
            self.runtime.handle_wake().await.unwrap();
            self.follow_up_utterance().await;
        }

        async fn follow_up_utterance(&self) {
            let started = tokio::time::Instant::now();
            for frame in 0..90 {
                let pcm: Vec<f32> = (0..320)
                    .map(|i| {
                        if frame < 25 {
                            0.03 * (i as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
                        } else {
                            0.0
                        }
                    })
                    .collect();
                assert_eq!(self.producer.push(&pcm), pcm.len());
                tokio::time::sleep_until(started + Duration::from_millis((frame + 1) * 20)).await;
            }
        }
        async fn phase(&self, expected: &str) -> nomifun_api_types::voice::VoiceSessionSnapshot {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let snapshot = self.runtime.snapshot();
                    if serde_json::to_value(&snapshot).unwrap()["phase"] == expected {
                        break snapshot;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("production worker must reach the expected phase")
        }
        fn assert_text(&self) {
            let snapshot = self.runtime.snapshot();
            if self.online {
                assert!(snapshot.user_text.as_ref().unwrap().contains("你好"));
            } else {
                assert_eq!(snapshot.user_text.as_deref(), Some("你好啊。"));
            }
            assert_eq!(snapshot.understood_request.as_deref(), Some(UNDERSTOOD));
            assert_eq!(snapshot.assistant_text.as_deref(), Some(REPLY));
            assert!(
                snapshot.conversation[0]
                    .model_text
                    .as_ref()
                    .unwrap()
                    .contains(REPLY)
            );
            assert_eq!(self.calls.load(Ordering::SeqCst), 1);
        }
    }

    struct RecordingStt(Arc<Mutex<Vec<Vec<f32>>>>);
    #[async_trait]
    impl nomifun_audio::recorder::SttCallback for RecordingStt {
        async fn transcribe(
            &self,
            _: nomifun_audio::frame::AudioChannel,
            pcm: Vec<f32>,
            _: u32,
        ) -> Option<String> {
            self.0.lock().unwrap().push(pcm);
            Some("你好啊。".into())
        }
    }

    #[tokio::test]
    async fn follow_up_keeps_the_audio_before_vad_becomes_active() {
        let flow = Flow::new(TtsMode::CancellationAware).await;
        flow.runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({"mode":"off","speak_replies":false}))
                    .unwrap(),
            )
            .unwrap();
        flow.utterance().await;
        flow.phase("text_follow_up").await;
        flow.follow_up_utterance().await;
        flow.phase("text_follow_up").await;
        let recordings = flow.recordings.lock().unwrap();
        assert_eq!(recordings.len(), 2);
        let voiced_samples =
            |pcm: &[f32]| pcm.iter().filter(|sample| sample.abs() > 0.00001).count();
        assert_eq!(
            voiced_samples(&recordings[1]),
            voiced_samples(&recordings[0]),
            "identical initial and follow-up input must reach ASR with the same voiced samples, including VAD onset"
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn muted_feedback_and_reply_keep_the_real_text_flow_without_any_output() {
        let flow = Flow::new(TtsMode::CancellationAware).await;
        flow.runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode":"off", "volume":0, "speak_replies":false
                }))
                .unwrap(),
            )
            .unwrap();
        flow.utterance().await;
        flow.phase("text_follow_up").await;
        flow.assert_text();
        assert_eq!(flow.calls.load(Ordering::SeqCst), 1);
        assert!(flow.sink.frames.lock().unwrap().is_empty());
        assert!(
            !flow
                .phases
                .lock()
                .unwrap()
                .contains(&nomifun_api_types::voice::VoiceSessionPhase::Synthesizing)
        );
        assert!(
            !flow
                .phases
                .lock()
                .unwrap()
                .contains(&nomifun_api_types::voice::VoiceSessionPhase::Speaking)
        );
        assert!(flow.runtime.snapshot().follow_up_deadline_ms.is_some());
    }

    #[tokio::test]
    async fn follow_up_requests_receive_only_three_completed_turns_and_stop_clears_context() {
        let flow = Flow::new(TtsMode::CancellationAware).await;
        flow.runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({"mode":"off","speak_replies":false}))
                    .unwrap(),
            )
            .unwrap();
        flow.utterance().await;
        flow.phase("text_follow_up").await;
        for _ in 0..4 {
            flow.follow_up_utterance().await;
            flow.phase("text_follow_up").await;
        }
        let context_counts: Vec<usize> = flow
            .prompts
            .lock()
            .unwrap()
            .iter()
            .map(|prompt| prompt.matches(REPLY).count())
            .collect();
        assert_eq!(
            context_counts,
            [0, 1, 2, 3, 3],
            "each model request must receive up to three completed exchanges"
        );
        let before = flow.runtime.snapshot();
        flow.runtime
            .stop_session(&nomifun_api_types::voice::VoiceSessionControlRequest {
                instance_id: before.instance_id,
                session_id: before.session_id,
                turn_id: before.turn_id,
            })
            .await
            .unwrap();
        assert_eq!(
            flow.runtime.snapshot().conversation.len(),
            3,
            "reading history survives session end"
        );
        flow.utterance().await;
        flow.phase("text_follow_up").await;
        assert_eq!(
            flow.prompts
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .matches(REPLY)
                .count(),
            0,
            "a new session cannot inherit the ended session's inference context"
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 6);
    }

    fn default_cue(kind: nomifun_audio::EarconKind) -> Vec<u8> {
        let (mut pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(kind.as_wav_bytes());
        for sample in &mut pcm {
            *sample *= 0.5;
        }
        nomifun_audio::pcm_to_wav(&pcm, rate)
    }

    #[tokio::test]
    async fn preview_obeys_volume_and_never_starts_capture_or_a_turn() {
        let flow = Flow::new(TtsMode::CancellationAware).await;
        let before = flow.runtime.snapshot();
        for volume in [25, 100] {
            let mut config = flow.runtime.feedback();
            config.volume = volume;
            flow.runtime.update_feedback(config.into()).unwrap();
            flow.runtime
                .preview_feedback(nomifun_api_types::voice::VoiceCueKind::Recognized)
                .await
                .unwrap();
            let frame = flow.sink.frames.lock().unwrap().last().unwrap().clone();
            let (pcm, _) = nomifun_audio::parse_audio_frame_to_pcm(&frame);
            let (original, _) = nomifun_audio::parse_audio_frame_to_pcm(
                nomifun_audio::EarconKind::Guide.as_wav_bytes(),
            );
            let peak = |samples: &[f32]| {
                samples
                    .iter()
                    .fold(0.0_f32, |max, sample| max.max(sample.abs()))
            };
            assert!((peak(&pcm) / peak(&original) - volume as f32 / 100.0).abs() < 0.002);
        }
        let after = flow.runtime.snapshot();
        assert_eq!(after.session_id, before.session_id);
        assert_eq!(after.turn_id, before.turn_id);
        assert_eq!(after.phase, before.phase);
        assert_eq!(
            after.audio.map(|a| a.frames_received),
            before.audio.map(|a| a.frames_received)
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 0);
        assert!(after.conversation.is_empty());
        let original = flow.runtime.feedback();
        let mut invalid = original;
        invalid.volume = 101;
        assert!(flow.runtime.update_feedback(invalid.into()).is_err());
        assert_eq!(flow.runtime.feedback(), original);
        let mut muted = original;
        muted.mode = nomifun_api_types::voice::VoiceFeedbackMode::Off;
        flow.runtime.update_feedback(muted.into()).unwrap();
        assert_eq!(
            flow.runtime
                .preview_feedback(nomifun_api_types::voice::VoiceCueKind::Recognized)
                .await,
            Err("feedback_muted")
        );
        flow.runtime.update_feedback(original.into()).unwrap();
        flow.runtime.handle_wake().await.unwrap();
        assert_eq!(
            flow.runtime
                .preview_feedback(nomifun_api_types::voice::VoiceCueKind::Error)
                .await,
            Err("voice_busy")
        );
    }

    #[tokio::test]
    async fn essential_feedback_keeps_wake_but_omits_the_listening_reminder() {
        let flow = Flow::new(TtsMode::CancellationAware).await;
        let mut config = flow.runtime.feedback();
        config.mode = nomifun_api_types::voice::VoiceFeedbackMode::Essential;
        flow.runtime.update_feedback(config.into()).unwrap();
        flow.runtime.handle_wake().await.unwrap();
        let start = tokio::time::Instant::now();
        for frame in 0..180 {
            assert_eq!(flow.producer.push(&[0.0; 320]), 320);
            tokio::time::sleep_until(start + Duration::from_millis((frame + 1) * 20)).await;
        }
        let feedback = flow.feedback.lock().unwrap();
        assert!(feedback.iter().any(|cue| cue.kind
            == nomifun_api_types::voice::VoiceCueKind::Wake
            && cue.status == nomifun_api_types::voice::VoiceCueStatus::Completed));
        assert!(
            !feedback
                .iter()
                .any(|cue| cue.kind == nomifun_api_types::voice::VoiceCueKind::ListeningReminder)
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn initial_listening_budget_starts_after_the_wake_cue_is_consumed() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.sink.hold_earcons.store(true, Ordering::SeqCst);
        flow.runtime.handle_wake().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), flow.sink.draining.notified())
            .await
            .unwrap();
        assert_eq!(
            flow.runtime.snapshot().listening_deadline_ms,
            None,
            "the six-second speech budget must not run while the wake cue is still playing"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
        flow.sink.hold_earcons.store(false, Ordering::SeqCst);
        flow.sink.consumed.notify_one();
        let deadline = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(deadline) = flow.runtime.snapshot().listening_deadline_ms {
                    break deadline;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(deadline >= chrono::Utc::now().timestamp_millis() + 5900);
        assert_eq!(flow.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn speech_during_the_wake_cue_keeps_capture_active_and_does_not_rearm_a_deadline() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.sink.hold_earcons.store(true, Ordering::SeqCst);
        flow.runtime.handle_wake().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), flow.sink.draining.notified())
            .await
            .unwrap();
        let start = tokio::time::Instant::now();
        for frame in 0..20 {
            let pcm: Vec<f32> = (0..320)
                .map(|i| 0.03 * (i as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin())
                .collect();
            assert_eq!(flow.producer.push(&pcm), pcm.len());
            tokio::time::sleep_until(start + Duration::from_millis((frame + 1) * 20)).await;
        }
        assert!(
            flow.runtime
                .snapshot()
                .audio
                .is_some_and(|audio| audio.speech_detected && audio.frames_received > 0),
            "capture must consume speech while the wake cue is still playing"
        );
        flow.sink.hold_earcons.store(false, Ordering::SeqCst);
        flow.sink.consumed.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if flow.runtime.snapshot().feedback.is_some_and(|feedback| {
                    feedback.kind == nomifun_api_types::voice::VoiceCueKind::Wake
                        && feedback.status == nomifun_api_types::voice::VoiceCueStatus::Completed
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            flow.runtime.snapshot().listening_deadline_ms,
            None,
            "cue completion must never restart the speech deadline after the user began talking"
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn follow_up_cue_completion_cannot_rearm_a_deadline_after_speech_started() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.utterance().await;
        flow.release_synthesis.notify_one();
        tokio::time::timeout(Duration::from_secs(5), flow.sink.draining.notified())
            .await
            .unwrap();
        flow.sink.hold_earcons.store(true, Ordering::SeqCst);
        flow.sink.consumed.notify_one();
        tokio::time::timeout(Duration::from_secs(1), flow.sink.draining.notified())
            .await
            .unwrap();
        assert_eq!(flow.runtime.snapshot().follow_up_deadline_ms, None);
        let start = tokio::time::Instant::now();
        for frame in 0..20 {
            let pcm: Vec<f32> = (0..320)
                .map(|i| 0.03 * (i as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin())
                .collect();
            assert_eq!(flow.producer.push(&pcm), pcm.len());
            tokio::time::sleep_until(start + Duration::from_millis((frame + 1) * 20)).await;
        }
        assert!(
            flow.runtime
                .snapshot()
                .audio
                .is_some_and(|audio| audio.speech_detected)
        );
        flow.sink.hold_earcons.store(false, Ordering::SeqCst);
        flow.sink.consumed.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if flow.runtime.snapshot().feedback.is_some_and(|feedback| {
                    feedback.kind == nomifun_api_types::voice::VoiceCueKind::FollowUp
                        && feedback.status == nomifun_api_types::voice::VoiceCueStatus::Completed
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            flow.runtime.snapshot().follow_up_deadline_ms,
            None,
            "a user already speaking during the follow-up cue must keep the entire utterance"
        );
        assert_eq!(
            flow.runtime.snapshot().phase,
            nomifun_api_types::voice::VoiceSessionPhase::FollowUp
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn initial_pcm_silence_is_a_normal_end_without_a_capture_error() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.runtime.handle_wake().await.unwrap();
        let start = tokio::time::Instant::now();
        for frame in 0..330 {
            assert_eq!(flow.producer.push(&[0.0; 320]), 320);
            tokio::time::sleep_until(start + Duration::from_millis((frame + 1) * 20)).await;
        }
        flow.runtime.expire_listening_deadline().await;
        let snapshot = flow.phase("idle").await;
        assert!(
            snapshot.error.is_none(),
            "healthy PCM with no speech is normal silence, not a capture failure"
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 0);
        let frames = flow.sink.frames.lock().unwrap();
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.as_slice()
                    == default_cue(nomifun_audio::EarconKind::Sleep).as_slice())
                .count(),
            1
        );
        assert!(
            !frames.iter().any(|frame| frame.as_slice()
                == default_cue(nomifun_audio::EarconKind::Error).as_slice())
        );
    }

    #[tokio::test]
    async fn slow_synthesis_has_at_most_three_processing_cues_and_stops_them_before_reply() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.utterance().await;
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .unwrap();
        let count = || {
            flow.feedback
                .lock()
                .unwrap()
                .iter()
                .filter(|cue| {
                    serde_json::to_value(cue).unwrap()["kind"] == "processing"
                        && cue.status == nomifun_api_types::voice::VoiceCueStatus::Started
                })
                .count()
        };
        let start = tokio::time::Instant::now();
        for frame in 0..850 {
            assert_eq!(flow.producer.push(&[0.0; 320]), 320);
            tokio::time::sleep_until(start + Duration::from_millis((frame + 1) * 20)).await;
            if frame == 200 {
                assert_eq!(
                    count(),
                    1,
                    "slow processing needs a lightweight progress cue"
                );
            }
        }
        assert_eq!(
            count(),
            3,
            "processing feedback must stay bounded across all processing stages"
        );
        let output = flow.sink.frames.lock().unwrap().last().unwrap().clone();
        let artifact = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/architecture/voice-agent/execution/R3-processing-cue.wav");
        std::fs::write(artifact, output).unwrap();
        flow.release_synthesis.notify_one();
        tokio::time::timeout(Duration::from_secs(5), flow.sink.draining.notified())
            .await
            .unwrap();
        flow.phase("speaking").await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            count(),
            3,
            "no processing cue may be queued behind the actual reply"
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 1);
        flow.sink.consumed.notify_one();
    }

    #[tokio::test]
    async fn listening_reminder_is_once_and_compensates_actual_output_time() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.runtime.handle_wake().await.unwrap();
        let original_deadline = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(deadline) = flow.runtime.snapshot().listening_deadline_ms {
                    break deadline;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        flow.sink.hold_earcons.store(true, Ordering::SeqCst);
        let start = tokio::time::Instant::now();
        for frame in 0..180 {
            assert_eq!(flow.producer.push(&[0.0; 320]), 320);
            tokio::time::sleep_until(start + Duration::from_millis((frame + 1) * 20)).await;
            if flow.runtime.snapshot().feedback.is_some_and(|cue| {
                cue.kind == nomifun_api_types::voice::VoiceCueKind::ListeningReminder
                    && cue.status == nomifun_api_types::voice::VoiceCueStatus::Started
            }) {
                break;
            }
        }
        tokio::time::timeout(Duration::from_millis(300), flow.sink.draining.notified())
            .await
            .expect("silence needs exactly one midpoint reminder");
        tokio::time::sleep(Duration::from_millis(200)).await;
        flow.sink.hold_earcons.store(false, Ordering::SeqCst);
        flow.sink.consumed.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if flow.runtime.snapshot().feedback.is_some_and(|cue| {
                    cue.kind == nomifun_api_types::voice::VoiceCueKind::ListeningReminder
                        && cue.status == nomifun_api_types::voice::VoiceCueStatus::Completed
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            flow.runtime.snapshot().listening_deadline_ms.unwrap() >= original_deadline + 180,
            "the valid speech window must not be shortened by actual cue consumption"
        );
        let count = flow
            .feedback
            .lock()
            .unwrap()
            .iter()
            .filter(|cue| {
                cue.kind == nomifun_api_types::voice::VoiceCueKind::ListeningReminder
                    && cue.status == nomifun_api_types::voice::VoiceCueStatus::Started
            })
            .count();
        assert_eq!(count, 1);
        assert_eq!(flow.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stopping_cancels_the_pending_turn_future_instead_of_only_hiding_its_result() {
        let flow = Flow::new(TtsMode::CancellationAware).await;
        flow.utterance().await;
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .unwrap();
        let scope = flow.runtime.snapshot();
        flow.runtime
            .stop_session(&nomifun_api_types::voice::VoiceSessionControlRequest {
                instance_id: scope.instance_id,
                session_id: scope.session_id,
                turn_id: scope.turn_id,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_millis(200), async {
            while !flow.cancelled.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("stopping must drop the owned async synthesis future, not leave it running");
        assert_eq!(
            flow.runtime.snapshot().phase,
            nomifun_api_types::voice::VoiceSessionPhase::Idle
        );
        assert_eq!(flow.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn capture_failure_cancels_an_inflight_voice_turn() {
        let flow = Flow::new(TtsMode::CancellationAware).await;
        flow.utterance().await;
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .unwrap();
        flow.producer.close();
        tokio::time::timeout(Duration::from_millis(300), async {
            while !flow.cancelled.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect(
            "capture failure must cancel the owned turn, not let an old reply resume listening",
        );
        let snapshot = flow.runtime.snapshot();
        assert!(!snapshot.acoustic_ready);
        assert_eq!(snapshot.error.unwrap().code, "capture_closed");
        assert_eq!(flow.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn recognized_speech_and_follow_up_end_have_distinct_offline_cues() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.utterance().await;
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .unwrap();
        let count = |kind: nomifun_audio::EarconKind| {
            flow.sink
                .frames
                .lock()
                .unwrap()
                .iter()
                .filter(|frame| frame.as_slice() == default_cue(kind).as_slice())
                .count()
        };
        assert_eq!(
            count(nomifun_audio::EarconKind::Guide),
            1,
            "valid ASR must produce one recognized cue before the model reply"
        );
        flow.release_synthesis.notify_one();
        tokio::time::timeout(Duration::from_secs(5), flow.sink.draining.notified())
            .await
            .unwrap();
        flow.sink.consumed.notify_one();
        flow.phase("follow_up").await;
        flow.runtime.driver().lock().await.expire_listening_deadline();
        flow.phase("idle").await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if flow.runtime.snapshot().feedback.is_some_and(|feedback| {
                    feedback.kind == nomifun_api_types::voice::VoiceCueKind::Sleep
                        && feedback.status == nomifun_api_types::voice::VoiceCueStatus::Completed
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("normal ending must publish an output consumption receipt");
        assert_eq!(
            count(nomifun_audio::EarconKind::Wake),
            2,
            "one initial wake and one follow-up opening cue"
        );
        assert_eq!(
            count(nomifun_audio::EarconKind::Sleep),
            1,
            "follow-up timeout must have one normal ending cue"
        );
        assert_eq!(count(nomifun_audio::EarconKind::Error), 0);
        flow.assert_text();
    }

    #[tokio::test]
    async fn entire_flow_separates_synthesis_playback_and_physical_completion() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.utterance().await;
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(flow.runtime.snapshot()).unwrap()["phase"],
            "synthesizing",
            "requesting TTS is not yet playback"
        );
        flow.assert_text();
        flow.release_synthesis.notify_one();
        tokio::time::timeout(Duration::from_secs(5), flow.sink.draining.notified())
            .await
            .unwrap();
        let snapshot = flow.phase("speaking").await;
        assert_eq!(snapshot.follow_up_deadline_ms, None);
        let frames = flow.sink.frames.lock().unwrap();
        let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(frames.last().unwrap());
        assert_eq!(rate, 24000);
        assert_eq!(pcm.len(), 4800);
        assert!(pcm.iter().any(|s| s.abs() > 0.05));
        drop(frames);
        flow.sink.consumed.notify_one();
        flow.phase("follow_up").await;
        flow.assert_text();
    }
    #[tokio::test]
    async fn empty_tts_keeps_text_and_does_not_claim_successful_audio() {
        let flow = Flow::new(TtsMode::Empty).await;
        flow.utterance().await;
        flow.release_synthesis.notify_one();
        let snapshot = flow.phase("text_follow_up").await;
        assert_eq!(snapshot.error.unwrap().code, "tts_failed");
        flow.assert_text();
    }
    #[tokio::test]
    async fn synthesis_panic_returns_to_idle_and_keeps_the_reply() {
        let flow = Flow::new(TtsMode::Panic).await;
        flow.utterance().await;
        let snapshot = flow.phase("idle").await;
        assert_eq!(snapshot.error.unwrap().code, "voice_turn_failed");
        flow.assert_text();
        flow.runtime.handle_wake().await.unwrap();
        assert_eq!(
            serde_json::to_value(flow.runtime.snapshot()).unwrap()["phase"],
            "listening"
        );
    }
    #[tokio::test]
    async fn tts_network_failure_keeps_text_without_entering_playback() {
        let flow = Flow::new(TtsMode::NetworkFailure).await;
        flow.utterance().await;
        let snapshot = flow.phase("text_follow_up").await;
        assert_eq!(snapshot.error.unwrap().code, "tts_failed");
        flow.assert_text();
        assert!(
            !flow
                .phases
                .lock()
                .unwrap()
                .contains(&nomifun_api_types::voice::VoiceSessionPhase::Speaking)
        );
    }
    #[tokio::test]
    async fn stopping_during_synthesis_suppresses_late_audio() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.utterance().await;
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .unwrap();
        let scope = flow.runtime.snapshot();
        flow.runtime
            .stop_session(&nomifun_api_types::voice::VoiceSessionControlRequest {
                instance_id: scope.instance_id,
                session_id: scope.session_id,
                turn_id: scope.turn_id,
            })
            .await
            .unwrap();
        assert_eq!(
            flow.runtime.snapshot().ended_reason,
            Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
        );
        flow.release_synthesis.notify_one();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            serde_json::to_value(flow.runtime.snapshot()).unwrap()["phase"],
            "idle"
        );
        let frames = flow.sink.frames.lock().unwrap();
        assert_eq!(
            frames.len(),
            1,
            "only the intentional stopped cue may remain"
        );
        assert_eq!(
            frames[0].as_slice(),
            default_cue(nomifun_audio::EarconKind::Sleep).as_slice(),
            "cancelled synthesis must not write late reply audio"
        );
    }

    #[tokio::test]
    async fn test_c05_pending_utterance_drained_across_idle_transition() {
        let flow = Flow::new(TtsMode::Audio).await;
        flow.utterance().await;
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .expect("turn 1 synthesis must be requested");

        let pcm_turn2 = vec![0.05; 3200];
        let session_id = flow.runtime.snapshot().session_id;
        flow.runtime.enqueue_pending_utterance(pcm_turn2, session_id);
        assert_eq!(flow.runtime.pending_utterance_count(), 1);

        // Complete turn 1 synthesis and playback
        flow.release_synthesis.notify_one();
        tokio::time::timeout(Duration::from_secs(5), flow.sink.draining.notified())
            .await
            .expect("turn 1 audio must reach sink");
        flow.sink.consumed.notify_one();

        // Turn 2 must be picked up from the queue and proceed to synthesis
        tokio::time::timeout(Duration::from_secs(5), flow.synthesis_requested.notified())
            .await
            .expect("turn 2 from pending queue must be drained and reach synthesis");

        assert_eq!(flow.runtime.pending_utterance_count(), 0);
        flow.release_synthesis.notify_one();
        tokio::time::timeout(Duration::from_secs(5), flow.sink.draining.notified())
            .await
            .expect("turn 2 audio must reach sink");
        flow.sink.consumed.notify_one();

        tokio::time::timeout(Duration::from_secs(5), async {
            while flow.calls.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both turns must complete model completion");

        assert_eq!(flow.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    #[ignore = "explicit real KWS/VAD/ASR and online Edge through the production worker; controlled model and sink; no microphone or speaker"]
    async fn real_acoustics_and_edge_complete_the_entire_worker_flow() {
        use nomifun_api_types::voice::VoiceSessionPhase as Phase;
        let flow = Flow::new(TtsMode::Online).await;
        tokio::time::timeout(Duration::from_secs(15), async {
            while !flow
                .runtime
                .snapshot()
                .audio
                .is_some_and(|audio| audio.vad_ready)
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut input = EdgeTtsProvider::new()
            .synthesize_stream("你好啊，你能听到我说话吗？", "")
            .await
            .unwrap();
        let input_wav = input.recv().await.unwrap();
        let (speech, rate) = nomifun_audio::parse_audio_frame_to_pcm(&input_wav);
        let (mut normalizer, input_producer) = VoiceCapturePipeline::new(PipelineConfig {
            sample_rate: rate,
            capacity_samples: speech.len(),
            backlog_limit_ms: 60000,
            ..Default::default()
        });
        assert_eq!(input_producer.push(&speech), speech.len());
        input_producer.close();
        let mut greeting = Vec::new();
        loop {
            match normalizer.next_chunk().await {
                nomifun_audio::capture::CaptureReadResult::Chunk(frame) => greeting.extend(frame),
                nomifun_audio::capture::CaptureReadResult::StreamClosed => break,
                other => panic!("unexpected normalized audio: {other:?}"),
            }
        }
        let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/architecture/voice-agent/execution");
        let (wake, rate) = nomifun_audio::parse_audio_frame_to_pcm(
            &std::fs::read(fixtures.join("audio-fixtures/wake-jenny-16k.wav")).unwrap(),
        );
        assert_eq!(rate, 16000);
        let mut pcm = vec![0.0; 8000];
        pcm.extend(wake);
        pcm.extend(vec![0.0; 1280]);
        pcm.extend(greeting);
        pcm.extend(vec![0.0; 24000]);
        std::fs::write(
            fixtures.join("T5-full-flow-input.wav"),
            nomifun_audio::pcm_to_wav(&pcm, 16000),
        )
        .unwrap();
        let started = tokio::time::Instant::now();
        for (index, frame) in pcm.chunks(320).enumerate() {
            assert_eq!(flow.producer.push(frame), frame.len());
            tokio::time::sleep_until(started + Duration::from_millis((index as u64 + 1) * 20))
                .await;
        }
        tokio::time::timeout(Duration::from_secs(35), flow.sink.draining.notified())
            .await
            .expect("real Edge audio must reach the production output pipeline");
        flow.phase("speaking").await;
        flow.assert_text();
        let audio = flow.runtime.snapshot().audio.unwrap();
        assert!(
            audio.kws_ready && audio.vad_ready && audio.asr_ready,
            "actual engines must be loaded; replay intent is not readiness"
        );
        assert_eq!(flow.runtime.snapshot().follow_up_deadline_ms, None);
        let wav = flow.sink.frames.lock().unwrap().last().unwrap().clone();
        let (output, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
        assert_eq!(rate, 24000);
        assert!(output.len() > 12000 && output.iter().any(|s| s.abs() > 0.001));
        std::fs::write(fixtures.join("T5-full-flow-output.wav"), wav).unwrap();
        flow.sink.consumed.notify_one();
        flow.phase("follow_up").await;
        assert_eq!(
            flow.runtime
                .capture_pipeline()
                .lock()
                .await
                .overflow_count(),
            0
        );
        flow.phase("idle").await;
        let phases = flow.phases.lock().unwrap();
        let required = [
            Phase::Listening,
            Phase::Transcribing,
            Phase::Deciding,
            Phase::Synthesizing,
            Phase::Speaking,
            Phase::FollowUp,
        ];
        let positions: Vec<_> = required
            .iter()
            .map(|phase| {
                phases
                    .iter()
                    .position(|actual| actual == phase)
                    .expect("phase must appear in real flow")
            })
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        println!(
            "Real flow phases: {phases:?}; reply_samples={}, rate={rate}; model_requests={}",
            output.len(),
            flow.calls.load(Ordering::SeqCst)
        );
    }
}

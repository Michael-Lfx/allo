//! Voice regressions through the production capture worker and its public event stream.
#![cfg(feature = "voice-skeleton")]
use std::{sync::Arc, time::Duration};

use ai_mate::{
    oobe::OobeManager,
    voice::{VoiceCompanionEvent, VoiceCompanionRuntime},
};
use futures_util::StreamExt;
use nomifun_audio::{
    stt::FakeSttCallback,
    voice_pipeline::{PipelineConfig, VoiceCapturePipeline},
};
use nomifun_voice_models::ModelManager;
use nomifun_voice_terminal::driver::StubVoiceRouter;
use tempfile::tempdir;
use tokio::sync::Mutex;
use tower::ServiceExt;

#[tokio::test]
async fn an_async_task_result_waits_for_actual_input_silence_before_playing(){
    use nomifun_api_types::voice::{VoiceTaskResultAnnouncement,VoiceSessionControlRequest};
    use nomifun_voice_terminal::{playback::{PlaybackPipeline,TestAudioSink},tts::{TtsProvider,TtsError}};
    struct Speech;
    #[async_trait::async_trait]
    impl TtsProvider for Speech {
        async fn synthesize_stream(&self,_:&str,_:&str)->Result<tokio::sync::mpsc::Receiver<Vec<u8>>,TtsError>{
            let(tx,rx)=tokio::sync::mpsc::channel(1);tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05;2400],24000)).unwrap();Ok(rx)
        }
    }
    let root=tempdir().unwrap();let models=Arc::new(ModelManager::new(root.path()));let oobe=Arc::new(OobeManager::new_bypassed());
    let(capture,producer)=VoiceCapturePipeline::new(PipelineConfig::default());
    let(runtime,_)=VoiceCompanionRuntime::from_capture(models.clone(),oobe.clone(),Arc::new(Mutex::new(capture)),Arc::new(FakeSttCallback::always("普通背景讲话")),Arc::new(StubVoiceRouter::chat("unused")));
    let sink=Arc::new(TestAudioSink::new());let runtime=Arc::new(runtime.with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())),Arc::new(Speech)));
    runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
    let app=ai_mate::server::build_router_full(&ai_mate::config::Config {control_token:Some("result-fixture".into()),..Default::default()},models,(*oobe).clone(),Some(runtime.event_sender()),Some(runtime.clone()));
    let worker=runtime.start_worker();
    for _ in 0..20 {
        let pcm:Vec<f32>=(0..320).map(|sample|0.03*(sample as f32*std::f32::consts::TAU*200.0/16000.0).sin()).collect();
        producer.push(&pcm);tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::timeout(Duration::from_secs(2),async {while !runtime.snapshot().audio.as_ref().is_some_and(|audio|audio.speech_detected){tokio::time::sleep(Duration::from_millis(10)).await;}}).await.unwrap();
    let request=||{
        let snapshot=runtime.snapshot();
        let body=VoiceTaskResultAnnouncement {scope:VoiceSessionControlRequest {instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},command_id:"actual-result".into(),notification_id:None,attempt_id:None,goal:None,text:"任务的真实结果。".into()};
        axum::http::Request::builder().method("POST").uri("/api/voice/task-result").header("content-type","application/json").header("x-ai-mate-control-token","result-fixture")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap())).unwrap()
    };
    assert_eq!(runtime.snapshot().phase,nomifun_api_types::voice::VoiceSessionPhase::Idle);
    let busy=app.clone().oneshot(request()).await.unwrap();
    assert_eq!(busy.status(),axum::http::StatusCode::CONFLICT,"Idle alone is not acoustic silence");assert_eq!(sink.frame_count(),0);
    for _ in 0..60 {producer.push(&[0.0;320]);tokio::time::sleep(Duration::from_millis(20)).await;}
    assert!(!runtime.snapshot().audio.unwrap().speech_detected);
    assert_eq!(app.oneshot(request()).await.unwrap().status(),axum::http::StatusCode::OK);
    assert!(sink.frame_count()>0,"a busy rejection must not consume the result's one-time announcement");
    worker.abort();
}

#[tokio::test]
async fn feedback_controls_use_the_supervisor_credential_and_preserve_session_state() {
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let oobe = Arc::new(OobeManager::new_bypassed());
    let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        models.clone(),
        oobe.clone(),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("你好")),
        Arc::new(StubVoiceRouter::chat("你好")),
    );
    let runtime = Arc::new(runtime);
    let config = ai_mate::config::Config {
        control_token: Some("test-supervisor".into()),
        ..Default::default()
    };
    let app = ai_mate::server::build_router_full(
        &config,
        models,
        (*oobe).clone(),
        Some(runtime.event_sender()),
        Some(runtime.clone()),
    );
    let request = |token: Option<&str>, body: &'static str| {
        let mut builder = http::Request::builder()
            .method("POST")
            .uri("/api/voice/feedback")
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("x-ai-mate-control-token", token);
        }
        builder.body(axum::body::Body::from(body)).unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request(None, r#"{"volume":0}"#))
            .await
            .unwrap()
            .status(),
        http::StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(request(Some("wrong"), r#"{"volume":0}"#))
            .await
            .unwrap()
            .status(),
        http::StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                Some("test-supervisor"),
                r#"{"mode":"off","volume":0,"speak_replies":false}"#
            ))
            .await
            .unwrap()
            .status(),
        http::StatusCode::OK
    );
    assert_eq!(runtime.feedback().volume, 0);
    assert!(!runtime.feedback().speak_replies);
    assert_eq!(
        app.oneshot(request(Some("test-supervisor"), r#"{"volume":101}"#))
            .await
            .unwrap()
            .status(),
        http::StatusCode::BAD_REQUEST
    );
    assert_eq!(runtime.feedback().volume, 0);
    assert_eq!(
        runtime.snapshot().phase,
        nomifun_api_types::voice::VoiceSessionPhase::Idle
    );
    assert!(runtime.snapshot().session_id.is_none());
}

#[tokio::test]
async fn ordinary_volume_reaches_asr_without_an_unconfigured_dialogue_fallback() {
    let dir = tempdir().unwrap();
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let router = Arc::new(StubVoiceRouter::chat("fake-success-must-never-play").unavailable());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        Arc::new(ModelManager::new(dir.path())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("现在几点")),
        router.clone(),
    );
    let mut events = runtime.event_sender().subscribe();
    runtime.handle_wake().await.unwrap();
    let worker = runtime.start_worker();
    for frame in 0..75 {
        let pcm: Vec<f32> = (0..320)
            .map(|sample| {
                if frame < 25 {
                    0.03 * (sample as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
                } else {
                    0.0
                }
            })
            .collect();
        producer.push(&pcm);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    worker.abort();
    let mut unavailable = false;
    let mut measured_speech = false;
    let mut asr_results = 0;
    let mut transcribing = 0;
    while let Ok(event) = events.try_recv() {
        if let VoiceCompanionEvent::AudioDiagnostics { diagnostics } = &event {
            measured_speech |= diagnostics.rms_milli > 10
                && diagnostics.speech_detected
                && diagnostics.asr_ready
                && diagnostics.frames_received > 0;
        }
        asr_results += usize::from(matches!(
            event,
            VoiceCompanionEvent::SpeechRecognized { .. }
        ));
        transcribing += usize::from(matches!(event, VoiceCompanionEvent::Transcribing));
        if let VoiceCompanionEvent::Error { code, .. } = &event {
            unavailable |= code == "model_unavailable";
        }
        assert!(!matches!(event, VoiceCompanionEvent::TtsSpeaking { .. }));
    }
    assert!(
        unavailable,
        "PCM replay must reach the actual unavailable dialogue branch"
    );
    assert_eq!(router.call_count(), 0);
    assert!(
        measured_speech,
        "meter must report real replay PCM and VAD activity"
    );
    assert_eq!(asr_results, 1);
    assert_eq!(transcribing, 1);
}

#[tokio::test]
async fn real_model_message_survives_tts_failure_as_a_text_reply() {
    for through_host in [false, true] {
        use nomifun_voice_terminal::{
            playback::{PlaybackPipeline, TestAudioSink},
            router::OpenAiCompatibleVoiceRouter,
            tts::FailingTts,
        };
        let answer = "这是模型返回的完整回答。".repeat(12);
        let raw = serde_json::json!({"intent_type":"chat","understood_request":"用户想知道今天的天气状况。","reply_tts":answer}).to_string();
        let content = raw.clone();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let called = calls.clone();
        let app = axum::Router::new().route(
            "/",
            axum::routing::post(
                move |headers: axum::http::HeaderMap,
                      axum::Json(body): axum::Json<serde_json::Value>| {
                    let content = content.clone();
                    let called = called.clone();
                    async move {
                        if through_host {
                            assert_eq!(
                                headers.get("x-ai-mate-control-token").unwrap(),
                                "fixture-control"
                            );
                            assert!(body["prompt"].as_str().unwrap().contains("今天的天气如何"));
                            assert_eq!(body["raw_user_text"], "今天的天气如何");
                            let turn = body["turn_id"].as_str().expect("production turn identity");
                            let session = body["session_id"].as_str().expect("production session identity");
                            assert_eq!(body["voice_record_id"], format!("voice-{turn}"));
                            assert_eq!(body["binding_id"], format!("voice-binding-{session}"));
                        }
                        called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        axum::Json(if through_host {
                            serde_json::json!({"content":content})
                        } else {
                            serde_json::json!({"choices":[{"message":{"content":content}}]})
                        })
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(dir.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(FakeSttCallback::always("今天的天气如何")),
            Arc::new(if through_host {
                OpenAiCompatibleVoiceRouter::with_completion(Arc::new(
                    nomifun_voice_terminal::completion::HostDialogueClient::new(
                        format!("http://{address}/"),
                        "fixture-control".into(),
                    )
                    .unwrap(),
                ))
            } else {
                OpenAiCompatibleVoiceRouter::from_custom_endpoint(
                    format!("http://{address}/"),
                    "",
                    "fixture",
                )
            }),
        );
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(FailingTts),
        );
        runtime.handle_wake().await.unwrap();
        let worker = runtime.start_worker();
        for frame in 0..75 {
            let pcm: Vec<f32> = (0..320)
                .map(|sample| {
                    if frame < 25 {
                        0.03 * (sample as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
                    } else {
                        0.0
                    }
                })
                .collect();
            producer.push(&pcm);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let snapshot = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let snapshot = runtime.snapshot();
                if snapshot.phase == nomifun_api_types::voice::VoiceSessionPhase::TextFollowUp {
                    break snapshot;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("model reply must reach explicit text follow-up after synthesis fails");
        assert_eq!(snapshot.error.as_ref().unwrap().code, "tts_failed");
        assert_eq!(snapshot.assistant_text.as_deref(), Some(answer.as_str()));
        assert_eq!(
            serde_json::to_value(&snapshot)
                .unwrap()
                .pointer("/conversation/0/understood_request")
                .and_then(|value| value.as_str()),
            Some("用户想知道今天的天气状况。")
        );
        assert_eq!(
            snapshot.conversation[0].model_text.as_deref(),
            Some(raw.as_str())
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        worker.abort();
        server.abort();
    }
}

#[tokio::test]
async fn failed_capture_revokes_readiness_and_never_shows_a_live_meter() {
    let dir = tempdir().unwrap();
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        Arc::new(ModelManager::new(dir.path())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("你好")),
        Arc::new(StubVoiceRouter::chat("你好")),
    );
    let worker = runtime.start_worker();
    producer.signal_device_error("fixture microphone disconnected");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = runtime.snapshot();
            if snapshot
                .audio
                .as_ref()
                .is_some_and(|audio| audio.capture_state == "closed")
            {
                assert!(!snapshot.acoustic_ready);
                assert_eq!(snapshot.audio.unwrap().rms_milli, 0);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
}

#[tokio::test]
async fn supervisor_credential_is_required_and_never_published() {
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let oobe = Arc::new(OobeManager::new_bypassed());
    let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        models.clone(),
        oobe.clone(),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("你好")),
        Arc::new(StubVoiceRouter::chat("你好")),
    );
    let mut config = ai_mate::config::Config::default();
    config.control_token = Some("fixture-private-credential".into());
    assert!(!format!("{config:?}").contains("fixture-private-credential"));
    assert!(
        !toml::to_string(&config)
            .unwrap()
            .contains("fixture-private-credential")
    );
    let app = ai_mate::server::build_router_full(
        &config,
        models,
        (*oobe).clone(),
        Some(runtime.event_sender()),
        Some(Arc::new(runtime)),
    );
    let response = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/api/voice/snapshot")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), http::StatusCode::FORBIDDEN);
    let response = app
        .oneshot(
            http::Request::builder()
                .uri("/api/voice/snapshot")
                .header("x-ai-mate-control-token", "fixture-private-credential")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("fixture-private-credential"));
}

#[tokio::test]
async fn old_window_stop_cannot_interrupt_a_new_wake() {
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        models,
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("你好")),
        Arc::new(StubVoiceRouter::chat("你好")),
    );
    runtime.handle_wake().await.unwrap();
    let snapshot = runtime.snapshot();
    let request = nomifun_api_types::voice::VoiceSessionControlRequest {
        instance_id: snapshot.instance_id,
        session_id: snapshot.session_id,
        turn_id: snapshot.turn_id,
    };
    runtime.stop_session(&request).await.unwrap();
    runtime.handle_wake().await.unwrap();
    assert!(runtime.stop_session(&request).await.is_err());
    assert_eq!(
        runtime.snapshot().phase,
        nomifun_api_types::voice::VoiceSessionPhase::Listening
    );
}

#[tokio::test]
async fn wake_http_rejects_unpaired_runtime_instead_of_claiming_success() {
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let oobe = Arc::new(OobeManager::new());
    let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        models.clone(),
        oobe.clone(),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("你好")),
        Arc::new(StubVoiceRouter::chat("你好")),
    );
    let app = ai_mate::server::build_router_full(
        &ai_mate::config::Config::default(),
        models,
        (*oobe).clone(),
        Some(runtime.event_sender()),
        Some(Arc::new(runtime)),
    );
    let response = app
        .oneshot(
            http::Request::builder()
                .method("POST")
                .uri("/api/voice/wake")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"keyword":"Hey Flowy"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn task_results_require_current_scope_preserve_text_on_tts_failure_and_do_not_replay() {
    use nomifun_api_types::voice::{VoiceTaskResultAnnouncement,VoiceSessionControlRequest,VoiceSessionPhase};
    use nomifun_voice_terminal::{playback::{PlaybackPipeline,TestAudioSink},tts::{TtsProvider,TtsError}};
    use std::sync::atomic::{AtomicUsize,Ordering};
    struct FailingSpeech(AtomicUsize);
    #[async_trait::async_trait]
    impl TtsProvider for FailingSpeech {
        async fn synthesize_stream(&self,_text:&str,_voice:&str) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>,TtsError> {
            self.0.fetch_add(1,Ordering::SeqCst);
            Err(TtsError::Network("controlled failure".into()))
        }
    }
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let oobe = Arc::new(OobeManager::new_bypassed());
    let (capture,_) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime,_) = VoiceCompanionRuntime::from_capture(models.clone(),oobe.clone(),Arc::new(Mutex::new(capture)),Arc::new(FakeSttCallback::always("unused")),Arc::new(StubVoiceRouter::chat("unused")));
    let tts = Arc::new(FailingSpeech(AtomicUsize::new(0)));
    let sink = Arc::new(TestAudioSink::new());
    let runtime = Arc::new(runtime.with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())),tts.clone()));
    runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
    let config = ai_mate::config::Config {control_token:Some("host-owned-fixture".into()),..Default::default()};
    let app = ai_mate::server::build_router_full(&config,models,(*oobe).clone(),Some(runtime.event_sender()),Some(runtime.clone()));
    let scope = |snapshot:&nomifun_api_types::voice::VoiceSessionSnapshot| VoiceSessionControlRequest {instance_id:snapshot.instance_id.clone(),session_id:snapshot.session_id.clone(),turn_id:snapshot.turn_id.clone()};
    let body = VoiceTaskResultAnnouncement {scope:scope(&runtime.snapshot()),command_id:"actual-command".into(),notification_id:None,attempt_id:None,goal:None,text:"真实任务的文件读取结果。".into()};
    let request = |body:&VoiceTaskResultAnnouncement, token:bool| {
        let mut builder = http::Request::builder().method("POST").uri("/api/voice/task-result").header("content-type","application/json");
        if token {builder=builder.header("x-ai-mate-control-token","host-owned-fixture");}
        builder.body(axum::body::Body::from(serde_json::to_vec(body).unwrap())).unwrap()
    };
    assert_eq!(app.clone().oneshot(request(&body,false)).await.unwrap().status(),http::StatusCode::FORBIDDEN);
    let mut old = body.clone(); old.scope.instance_id="previous-daemon".into();
    assert_eq!(app.clone().oneshot(request(&old,true)).await.unwrap().status(),http::StatusCode::CONFLICT);
    assert_eq!(tts.0.load(Ordering::SeqCst),0);
    let response=app.clone().oneshot(request(&body,true)).await.unwrap();
    assert_eq!(response.status(),http::StatusCode::OK);
    let receipt:nomifun_api_types::voice::VoicePlaybackReceipt=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),4096).await.unwrap()).unwrap();
    assert_eq!(receipt.status,"failed","task success cannot imply audio playback success");
    assert_eq!(receipt.reason_code.as_deref(),Some("tts_failed"));
    let query=app.clone().oneshot(http::Request::builder().uri("/api/voice/notifications/actual-command").header("x-ai-mate-control-token","host-owned-fixture").body(axum::body::Body::empty()).unwrap()).await.unwrap();
    let queried:nomifun_api_types::voice::VoicePlaybackReceipt=serde_json::from_slice(&axum::body::to_bytes(query.into_body(),4096).await.unwrap()).unwrap();
    assert_eq!(queried,receipt);
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.phase,VoiceSessionPhase::TextFollowUp);
    assert_eq!(snapshot.assistant_text.as_deref(),Some(body.text.as_str()));
    assert_eq!(snapshot.error.as_ref().unwrap().code,"tts_failed");
    assert!(snapshot.follow_up_deadline_ms.is_some());
    assert_eq!(sink.frame_count(),0);
    let duplicate = VoiceTaskResultAnnouncement {scope:scope(&snapshot),..body.clone()};
    assert_eq!(app.clone().oneshot(request(&duplicate,true)).await.unwrap().status(),http::StatusCode::CONFLICT);
    assert_eq!(tts.0.load(Ordering::SeqCst),1,"a repeated task result must not re-synthesize");
    assert_eq!(app.oneshot(request(&body,true)).await.unwrap().status(),http::StatusCode::OK,"an exact retry only reads its cached playback receipt");
}

#[tokio::test]
async fn task_dispatch_requires_a_fresh_scoped_runtime_receipt_before_announcing_acceptance() {
    use nomifun_api_types::voice::{VoiceTaskDelivery, VoiceTaskSnapshot, VoiceTaskStatus, VoiceTaskSubmitRequest};
    use nomifun_voice_terminal::task_client::VoiceTaskClient;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct TaskClient { status: VoiceTaskStatus, fresh: bool, calls: AtomicUsize }
    #[async_trait::async_trait]
    impl VoiceTaskClient for TaskClient {
        async fn current(&self) -> Result<Option<VoiceTaskSnapshot>,String> {Ok(None)}
        async fn control(&self,_request:nomifun_api_types::voice::VoiceTaskControlCommand) -> Result<VoiceTaskSnapshot,String> {Err("runtime_unavailable".into())}
        async fn submit(&self, request: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.command.goal, "读取工作区文件");
            assert!(!request.instance_id.is_empty());
            assert_ne!(request.command.command_id, request.command.turn_id);
            Ok(VoiceTaskDelivery { freshly_accepted: self.fresh, task: VoiceTaskSnapshot {
                command_id: request.command.command_id, session_id: request.command.session_id,
                acoustic_turn_id: request.command.turn_id, conversation_id: Some("actual-conversation".into()),
                message_id: Some("actual-message".into()), runtime_turn_id: Some("actual-runtime-turn".into()),
                status: self.status, result_ok: None, result_text: None, error_code: None,
            }})
        }
    }
    for (status, fresh, announced) in [(VoiceTaskStatus::Accepted,true,true), (VoiceTaskStatus::Accepted,false,false), (VoiceTaskStatus::Unknown,true,false)] {
        let dir = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let client = Arc::new(TaskClient {status, fresh, calls: AtomicUsize::new(0)});
        let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(dir.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), Arc::new(FakeSttCallback::always("读取工作区文件")),
            Arc::new(StubVoiceRouter::new(serde_json::from_value(serde_json::json!({
                "intent_type":"agent_task", "task_goal":"读取工作区文件", "spoken_ack":"模型声称已经完成", "requires_confirmation":false
            })).unwrap())),
        );
        let runtime = runtime.with_tasks(client.clone());
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        let worker = runtime.start_worker();
        for index in 0..75 {
            producer.push(&vec![if index<25 {0.7} else {0.0};320]);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        worker.abort();
        assert_eq!(client.calls.load(Ordering::SeqCst),1,"production intent dispatcher must deliver once");
        assert_eq!(runtime.snapshot().assistant_text.as_deref(), announced.then_some("任务已接单，正在执行。"));
    }
}

#[tokio::test]
async fn task_without_runtime_never_announces_acceptance() {
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        models,
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("备份目录")),
        Arc::new(StubVoiceRouter::new(serde_json::from_value(serde_json::json!({
            "intent_type": "agent_task",
            "task_goal": "备份目录",
            "spoken_ack": "已接单",
            "reply_tts": "已接单",
            "requires_confirmation": false
        })).unwrap())),
    );
    let mut events = runtime.event_sender().subscribe();
    runtime.handle_wake_keyword("Hey Flowy").await.unwrap();
    let worker = runtime.start_worker();
    for _ in 0..25 {
        producer.push(&vec![0.7; 320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for _ in 0..50 {
        producer.push(&vec![0.0; 320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut decided = false;
    let mut announced = false;
    while let Ok(event) = events.try_recv() {
        match event {
            VoiceCompanionEvent::IntentDecided { intent, .. } if intent == "AgentTask" => {
                decided = true
            }
            VoiceCompanionEvent::TtsSpeaking { text } if text == "已接单" => announced = true,
            _ => {}
        }
    }
    worker.abort();
    assert!(
        decided,
        "the replay must reach the production intent dispatcher"
    );
    assert!(
        !announced,
        "a routing decision is not a real task acceptance receipt"
    );
    assert!(runtime.snapshot().assistant_text.is_none(), "unaccepted task text cannot appear as an assistant acceptance either");
}

#[tokio::test]
async fn external_wake_expires_even_without_any_pcm() {
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        models,
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("你好")),
        Arc::new(StubVoiceRouter::chat("你好")),
    );
    let mut events = runtime.event_sender().subscribe();
    let worker = runtime.start_worker();
    runtime.handle_wake_keyword("Hey Flowy").await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if matches!(events.recv().await.unwrap(), VoiceCompanionEvent::Idle) {
                break;
            }
        }
    })
    .await;
    worker.abort();
    assert!(
        result.is_ok(),
        "an external wake must time out without waiting for another PCM chunk"
    );
    assert_eq!(runtime.snapshot().error.unwrap().code, "capture_no_pcm");
    assert_eq!(runtime.snapshot().listening_deadline_ms, None);
}

#[tokio::test]
async fn late_event_subscriber_receives_current_session_immediately() {
    let dir = tempdir().unwrap();
    let models = Arc::new(ModelManager::new(dir.path()));
    let oobe = Arc::new(OobeManager::new_bypassed());
    let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _audio) = VoiceCompanionRuntime::from_capture(
        models.clone(),
        oobe.clone(),
        Arc::new(Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("你好")),
        Arc::new(StubVoiceRouter::chat("你好")),
    );
    let runtime = Arc::new(runtime);
    runtime.handle_wake_keyword("Hey Flowy").await.unwrap();
    let app = ai_mate::server::build_router_full(
        &ai_mate::config::Config::default(),
        models,
        (*oobe).clone(),
        Some(runtime.event_sender()),
        Some(runtime),
    );
    let response = app
        .oneshot(
            http::Request::builder()
                .uri("/api/voice/events")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut stream = response.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_millis(200), stream.next()).await;
    assert!(
        first.is_ok(),
        "a newly created window cannot wait for the next wake to discover the active session"
    );
    let bytes = first.unwrap().unwrap().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("listening"));
    let frame = String::from_utf8_lossy(&bytes);
    let data = frame
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap();
    let event: serde_json::Value = serde_json::from_str(data).unwrap();
    let snapshot = &event["payload"]["snapshot"];
    assert!(snapshot["instance_id"].as_str().unwrap().len() > 10);
    assert!(snapshot["seq"].as_u64().unwrap() >= 2);
    assert!(snapshot["session_id"].is_string());
}


#[tokio::test]
async fn notification_is_not_played_until_the_sink_drains_and_an_exact_retry_only_reads_the_receipt() {
    use nomifun_api_types::voice::{VoicePlaybackReceipt,VoiceTaskResultAnnouncement,VoiceSessionControlRequest};
    use nomifun_voice_terminal::{playback::{AudioSink,AudioSinkError,PlaybackPipeline},tts::{TtsProvider,TtsError}};
    use std::sync::atomic::{AtomicUsize,Ordering};
    struct Sink {entered:tokio::sync::Notify,release:tokio::sync::Notify,frames:AtomicUsize}
    #[async_trait::async_trait]
    impl AudioSink for Sink {
        async fn write_frame(&self,_:Vec<u8>)->Result<(),AudioSinkError>{self.frames.fetch_add(1,Ordering::SeqCst);Ok(())}
        async fn drain(&self)->Result<(),AudioSinkError>{self.entered.notify_one();self.release.notified().await;Ok(())}
        async fn flush(&self)->Result<(),AudioSinkError>{Ok(())}
    }
    struct Speech;
    #[async_trait::async_trait]
    impl TtsProvider for Speech {
        async fn synthesize_stream(&self,_:&str,_:&str)->Result<tokio::sync::mpsc::Receiver<Vec<u8>>,TtsError>{
            let(tx,rx)=tokio::sync::mpsc::channel(1);tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05;2400],24000)).unwrap();Ok(rx)
        }
    }
    let dir=tempdir().unwrap();let models=Arc::new(ModelManager::new(dir.path()));let oobe=Arc::new(OobeManager::new_bypassed());
    let(capture,_)=VoiceCapturePipeline::new(PipelineConfig::default());
    let(runtime,_)=VoiceCompanionRuntime::from_capture(models.clone(),oobe.clone(),Arc::new(Mutex::new(capture)),Arc::new(FakeSttCallback::always("unused")),Arc::new(StubVoiceRouter::chat("unused")));
    let sink=Arc::new(Sink{entered:Default::default(),release:Default::default(),frames:Default::default()});
    let runtime=Arc::new(runtime.with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())),Arc::new(Speech)));
    runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
    let app=ai_mate::server::build_router_full(&ai_mate::config::Config{control_token:Some("playback-proof".into()),..Default::default()},models,(*oobe).clone(),Some(runtime.event_sender()),Some(runtime.clone()));
    let snapshot=runtime.snapshot();let payload=VoiceTaskResultAnnouncement{scope:VoiceSessionControlRequest{instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},command_id:"result".into(),notification_id:Some("notification".into()),attempt_id:Some("attempt".into()),goal:Some("读取文件".into()),text:"文件读取的真实结果。".into()};
    let request=|payload:&VoiceTaskResultAnnouncement|http::Request::builder().method("POST").uri("/api/voice/task-result").header("content-type","application/json").header("x-ai-mate-control-token","playback-proof").body(axum::body::Body::from(serde_json::to_vec(payload).unwrap())).unwrap();
    let owned=app.clone();let req=request(&payload);let posting=tokio::spawn(async move{owned.oneshot(req).await.unwrap()});
    tokio::time::timeout(Duration::from_secs(2),sink.entered.notified()).await.unwrap();
    let response=app.clone().oneshot(http::Request::builder().uri("/api/voice/notifications/attempt").header("x-ai-mate-control-token","playback-proof").body(axum::body::Body::empty()).unwrap()).await.unwrap();
    let pending:VoicePlaybackReceipt=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),4096).await.unwrap()).unwrap();
    assert_eq!(pending.status,"started");assert!(runtime.snapshot().follow_up_deadline_ms.is_none());
    sink.release.notify_one();let response=posting.await.unwrap();
    let played:VoicePlaybackReceipt=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),4096).await.unwrap()).unwrap();
    assert_eq!(played.status,"played");assert!(runtime.snapshot().follow_up_deadline_ms.is_some());
    let count=sink.frames.load(Ordering::SeqCst);
    assert_eq!(app.clone().oneshot(request(&payload)).await.unwrap().status(),http::StatusCode::OK);
    assert_eq!(sink.frames.load(Ordering::SeqCst),count);
    let mut changed=payload;changed.text="same identity, changed content".into();
    assert_eq!(app.oneshot(request(&changed)).await.unwrap().status(),http::StatusCode::CONFLICT);
}

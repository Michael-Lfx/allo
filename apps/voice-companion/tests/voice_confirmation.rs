//! Approval listening through the daemon HTTP surface and production capture worker.
#![cfg(feature="voice-skeleton")]
use std::{sync::{Arc,atomic::{AtomicBool,Ordering}},time::Duration};
use async_trait::async_trait;
use voice_companion::{oobe::OobeManager,voice::VoiceCompanionRuntime};
use nomifun_api_types::voice::*;
use nomifun_audio::{stt::FakeSttCallback,voice_pipeline::{PipelineConfig,VoiceCapturePipeline}};
use nomifun_voice_terminal::{driver::StubVoiceRouter,playback::{AudioSink,AudioSinkError,PlaybackPipeline},tts::{TtsProvider,TtsError}};
use tower::ServiceExt;

#[derive(Default)]
struct PromptSink {written:AtomicBool,released:AtomicBool,release:tokio::sync::Notify}
#[async_trait]
impl AudioSink for PromptSink {
    fn has_pending_audio(&self)->bool{self.written.load(Ordering::SeqCst)&&!self.released.load(Ordering::SeqCst)}
    async fn write_frame(&self,_:Vec<u8>)->Result<(),AudioSinkError>{self.written.store(true,Ordering::SeqCst);Ok(())}
    async fn flush(&self)->Result<(),AudioSinkError>{self.released.store(true,Ordering::SeqCst);self.release.notify_waiters();Ok(())}
    async fn drain(&self)->Result<(),AudioSinkError>{while !self.released.load(Ordering::SeqCst){self.release.notified().await;}Ok(())}
}
struct PromptTts;
#[async_trait]
impl TtsProvider for PromptTts {
    async fn synthesize_stream(&self,_:&str,_:&str)->Result<tokio::sync::mpsc::Receiver<Vec<u8>>,TtsError>{
        let (tx,rx)=tokio::sync::mpsc::channel(1);tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05;2400],24000)).unwrap();Ok(rx)
    }
}

async fn scenario(heard:Option<&str>,ttl:Duration,stop_during_prompt:bool,prompt_delay:Duration)->VoiceToolConfirmationReply {
    let root=tempfile::tempdir().unwrap();
    let models=Arc::new(nomifun_voice_models::ModelManager::new(root.path()));
    let oobe=Arc::new(OobeManager::new_bypassed());
    let (capture,producer)=VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime,_)=VoiceCompanionRuntime::from_capture(models.clone(),oobe.clone(),Arc::new(tokio::sync::Mutex::new(capture)),
        Arc::new(FakeSttCallback::always(heard.unwrap_or("unused"))),Arc::new(StubVoiceRouter::chat("confirmation must not call the dialogue router")));
    let sink=Arc::new(PromptSink::default());
    let runtime=Arc::new(runtime.with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())),Arc::new(PromptTts)));
    runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
    let worker=runtime.start_worker();
    let app=voice_companion::server::build_router_full(&voice_companion::config::Config{control_token:Some("approval-fixture".into()),..Default::default()},models,(*oobe).clone(),Some(runtime.event_sender()),Some(runtime.clone()));
    let snapshot=runtime.snapshot();
    let prompt=VoiceToolConfirmationPrompt {
        scope:VoiceSessionControlRequest {instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},
        task:VoiceTaskReference {command_id:"actual-command".into(),conversation_id:Some("actual-conversation".into()),message_id:Some("accepted-message".into())},
        approval_id:"actual-approval".into(),msg_id:"actual-tool-message".into(),call_id:"actual-call".into(),
        expires_at_ms:chrono::Utc::now().timestamp_millis()+ttl.as_millis() as i64,
        text:"准备删除 owned.txt。请说确认或取消。".into(),
    };
    let authority_deadline=prompt.expires_at_ms;
    let request=tokio::spawn(async move {app.oneshot(axum::http::Request::builder().method("POST").uri("/api/voice/confirmation")
        .header("content-type","application/json").header("x-ai-mate-control-token","approval-fixture")
        .body(axum::body::Body::from(serde_json::to_vec(&prompt).unwrap())).unwrap()).await.unwrap()});
    tokio::time::timeout(Duration::from_secs(3),async{
        while !sink.written.load(Ordering::SeqCst){
            assert!(!request.is_finished(),"the confirmation route must start real prompt playback before responding");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    assert_eq!(runtime.snapshot().phase,VoiceSessionPhase::AwaitingConfirmation);
    assert!(runtime.snapshot().listening_deadline_ms.is_none(),"listening cannot start while prompt audio is still queued");
    tokio::time::sleep(prompt_delay).await;
    assert_eq!(runtime.snapshot().phase,VoiceSessionPhase::AwaitingConfirmation);
    if stop_during_prompt {
        let snapshot=runtime.snapshot();
        runtime.stop_session(&VoiceSessionControlRequest {instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id}).await.unwrap();
        let response=tokio::time::timeout(Duration::from_secs(2),request).await.unwrap().unwrap();
        let reply:VoiceToolConfirmationReply=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),8192).await.unwrap()).unwrap();
        assert!(!reply.approved);assert!(sink.released.load(Ordering::SeqCst));
        assert_eq!(runtime.snapshot().ended_reason,Some(VoiceSessionEndReason::UserStop));
        worker.abort();return reply;
    }
    sink.released.store(true,Ordering::SeqCst);sink.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2),async{
        while runtime.snapshot().phase!=VoiceSessionPhase::ListeningConfirm {tokio::time::sleep(Duration::from_millis(5)).await;}
    }).await.unwrap();
    let deadline=runtime.snapshot().listening_deadline_ms.unwrap();
    assert!(deadline<=chrono::Utc::now().timestamp_millis()+8000);
    assert!(deadline<=authority_deadline);
    if heard.is_some(){
        for frame in 0..75 {
            let pcm:Vec<f32>=(0..320).map(|sample|if frame<25 {0.03*(sample as f32*std::f32::consts::TAU*200.0/16000.0).sin()}else{0.0}).collect();
            assert_eq!(producer.push(&pcm),pcm.len());tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    let response=tokio::time::timeout(Duration::from_secs(9),request).await.unwrap().unwrap();
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let reply:VoiceToolConfirmationReply=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),8192).await.unwrap()).unwrap();
    assert_eq!((&*reply.approval_id,&*reply.msg_id,&*reply.call_id),("actual-approval","actual-tool-message","actual-call"));
    assert_eq!(runtime.snapshot().phase,VoiceSessionPhase::Idle);
    if let Some(text)=heard {assert_eq!(runtime.snapshot().user_text.as_deref(),Some(text));}
    worker.abort();reply
}

#[tokio::test]
async fn oral_approval_starts_after_actual_prompt_consumption_and_accepts_only_an_exact_answer(){
    for (text,approved) in [("确认",true),("取消",false),("我还没有确认",false)]{
        assert_eq!(scenario(Some(text),Duration::from_secs(30),false,Duration::from_millis(150)).await.approved,approved);
    }
}

#[tokio::test]
async fn silence_never_extends_the_execution_authority_deadline(){
    assert!(!scenario(None,Duration::from_millis(1400),false,Duration::from_millis(150)).await.approved);
}

#[tokio::test]
async fn stopping_during_the_prompt_flushes_audio_and_cannot_authorize_the_tool(){
    assert!(!scenario(None,Duration::from_secs(30),true,Duration::from_millis(150)).await.approved);
}

#[tokio::test]
async fn a_long_prompt_does_not_consume_the_confirmation_listening_window(){
    assert!(scenario(Some("确认"),Duration::from_secs(30),false,Duration::from_secs(9)).await.approved);
}

#[tokio::test]
async fn barge_in_during_prompt_stops_prompt_and_authorizes_tool() {
    let root = tempfile::tempdir().unwrap();
    let models = Arc::new(nomifun_voice_models::ModelManager::new(root.path()));
    let oobe = Arc::new(OobeManager::new_bypassed());
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _) = VoiceCompanionRuntime::from_capture(
        models.clone(),
        oobe.clone(),
        Arc::new(tokio::sync::Mutex::new(capture)),
        Arc::new(FakeSttCallback::always("确认")),
        Arc::new(StubVoiceRouter::chat("confirmation must not call dialogue")),
    );
    let sink = Arc::new(PromptSink::default());
    let runtime = Arc::new(runtime.with_test_barge_in().with_test_playback(
        Arc::new(PlaybackPipeline::new(sink.clone())),
        Arc::new(PromptTts),
    ));
    runtime
        .update_feedback(
            serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true}))
                .unwrap(),
        )
        .unwrap();
    let worker = runtime.start_worker();
    let app = voice_companion::server::build_router_full(
        &voice_companion::config::Config {
            control_token: Some("approval-fixture".into()),
            ..Default::default()
        },
        models,
        (*oobe).clone(),
        Some(runtime.event_sender()),
        Some(runtime.clone()),
    );
    let snapshot = runtime.snapshot();
    let prompt = VoiceToolConfirmationPrompt {
        scope: VoiceSessionControlRequest {
            instance_id: snapshot.instance_id,
            session_id: snapshot.session_id,
            turn_id: snapshot.turn_id,
        },
        task: VoiceTaskReference {
            command_id: "actual-command".into(),
            conversation_id: Some("actual-conversation".into()),
            message_id: Some("accepted-message".into()),
        },
        approval_id: "actual-approval".into(),
        msg_id: "actual-tool-message".into(),
        call_id: "actual-call".into(),
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 30_000,
        text: "准备删除 owned.txt。请说确认或取消。".into(),
    };
    let request = tokio::spawn(async move {
        app.oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/voice/confirmation")
                .header("content-type", "application/json")
                .header("x-ai-mate-control-token", "approval-fixture")
                .body(axum::body::Body::from(serde_json::to_vec(&prompt).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
    });

    tokio::time::timeout(Duration::from_secs(3), async {
        while !sink.written.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(runtime.snapshot().phase, VoiceSessionPhase::AwaitingConfirmation);

    // Speak during prompt playback (barge-in)
    for _frame in 0..25 {
        let pcm: Vec<f32> = (0..320)
            .map(|sample| {
                0.03 * (sample as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
            })
            .collect();
        assert_eq!(producer.push(&pcm), pcm.len());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Barge-in must flush prompt sink and transition to ListeningConfirm
    tokio::time::timeout(Duration::from_secs(2), async {
        while runtime.snapshot().phase != VoiceSessionPhase::ListeningConfirm {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(sink.released.load(Ordering::SeqCst), "prompt audio sink must be flushed upon barge-in");

    // Send silence to complete the utterance ("确认")
    for _ in 0..50 {
        let pcm = vec![0.0_f32; 320];
        assert_eq!(producer.push(&pcm), pcm.len());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let response = tokio::time::timeout(Duration::from_secs(9), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let reply: VoiceToolConfirmationReply = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply.approval_id, "actual-approval");
    assert!(reply.approved, "oral approval must succeed even when user barged in during prompt");
    worker.abort();
}
#[tokio::test]
async fn stop_during_prompt_does_not_restart_a_previous_live_session() {
    for restart in [false, true] {
    let root=tempfile::tempdir().unwrap();
    let models=Arc::new(nomifun_voice_models::ModelManager::new(root.path()));
    let(capture,_producer)=VoiceCapturePipeline::new(PipelineConfig::default());
    let(runtime,_)=VoiceCompanionRuntime::from_capture(models,Arc::new(OobeManager::new_bypassed()),Arc::new(tokio::sync::Mutex::new(capture)),Arc::new(FakeSttCallback::always("确认")),Arc::new(StubVoiceRouter::chat("unused")));
    let sink=Arc::new(PromptSink::default());
    let runtime=Arc::new(runtime.with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())),Arc::new(PromptTts)));
    runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
    let worker=runtime.start_worker();
    runtime.handle_wake().await.unwrap();
    let snapshot=runtime.snapshot();
    let prompt=VoiceToolConfirmationPrompt {
        scope:VoiceSessionControlRequest{instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},
        task:VoiceTaskReference{command_id:"command".into(),conversation_id:Some("conversation".into()),message_id:Some("message".into())},
        approval_id:"approval".into(),msg_id:"tool-message".into(),call_id:"call".into(),expires_at_ms:chrono::Utc::now().timestamp_millis()+30_000,text:"请确认删除。".into(),
    };
    let job={let runtime=runtime.clone();tokio::spawn(async move{runtime.request_tool_confirmation(prompt).await})};
    tokio::time::timeout(Duration::from_secs(2),async{while !sink.written.load(Ordering::SeqCst){tokio::time::sleep(Duration::from_millis(5)).await;}}).await.unwrap();
    let snapshot=runtime.snapshot();
    runtime.stop_session(&VoiceSessionControlRequest{instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id}).await.unwrap();
    if restart { runtime.handle_wake().await.unwrap(); }
    let new_session = runtime.snapshot().session_id;
    assert!(!tokio::time::timeout(Duration::from_secs(2),job).await.unwrap().unwrap().unwrap().approved);
    let state=runtime.driver().lock().await.current_state();
    worker.abort();
    assert_eq!(state,if restart {nomifun_voice_terminal::contract::AcousticState::Listening} else {nomifun_voice_terminal::contract::AcousticState::Idle},"old confirmation cleanup must not resume or reset listening after explicit user stop/new wake");
    assert_eq!(runtime.snapshot().session_id, new_session);
    }
}

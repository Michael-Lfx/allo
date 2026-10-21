#![cfg(feature="voice-skeleton")]
use std::sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}};
use std::time::Duration;
use nomifun_voice_terminal::{completion::HostDialogueClient,router::OpenAiCompatibleVoiceRouter,driver::VoiceRouter};
use nomifun_api_types::voice::VoiceDialogueRequest;

struct ModelStream {
    endpoint:String,
    release:Arc<tokio::sync::Notify>,
    completed:Arc<AtomicBool>,
    calls:Arc<AtomicUsize>,
    job:tokio::task::JoinHandle<()>,
}
impl Drop for ModelStream {fn drop(&mut self){self.job.abort();}}
impl ModelStream {
    async fn new()->Self {Self::with_error(false).await}
    async fn with_error(fail:bool)->Self {
        let release=Arc::new(tokio::sync::Notify::new());let completed=Arc::new(AtomicBool::new(false));let calls=Arc::new(AtomicUsize::new(0));
        let gate=release.clone();let done=completed.clone();let count=calls.clone();
        let app=axum::Router::new().route("/complete",axum::routing::post(move |headers:axum::http::HeaderMap,axum::Json(body):axum::Json<serde_json::Value>| {
            let gate=gate.clone();let done=done.clone();let count=count.clone();
            async move {
                assert_eq!(headers.get("x-ai-mate-control-token").unwrap(),"stream-fixture");
                assert_eq!(body["stream"],true,"the production worker must request incremental completion");
                count.fetch_add(1,Ordering::SeqCst);
                let (tx,rx)=tokio::sync::mpsc::channel(4);
                tokio::spawn(async move {
                    let first=serde_json::json!({"type":"sentence","sentence":{"text":"你好。","understood_request":"用户问好"}}).to_string()+"\n";
                    if tx.send(Ok::<_,std::convert::Infallible>(first)).await.is_err(){return;}
                    tokio::select!{_ = gate.notified()=>{},_ = tx.closed()=>return}
                    if fail {let _=tx.send(Ok(serde_json::json!({"type":"error","code":"model_timeout"}).to_string()+"\n")).await;return;}
                    let next=serde_json::json!({"type":"sentence","sentence":{"text":"我在听。","understood_request":null}}).to_string()+"\n";
                    if tx.send(Ok(next)).await.is_err(){return;}
                    let raw=serde_json::json!({"intent_type":"chat","understood_request":"用户问好","reply_tts":"你好。我在听。"}).to_string();
                    done.store(true,Ordering::SeqCst);
                    let _=tx.send(Ok(serde_json::json!({"type":"complete","response":{"content":raw}}).to_string()+"\n")).await;
                });
                axum::response::Response::builder().header("content-type","application/x-ndjson").body(axum::body::Body::from_stream(futures_util::stream::unfold(rx,|mut rx|async {rx.recv().await.map(|item|(item,rx))}))).unwrap()
            }
        }));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let endpoint=format!("http://{}/complete",listener.local_addr().unwrap());
        let job=tokio::spawn(async move {axum::serve(listener,app).await.unwrap();});
        Self{endpoint,release,completed,calls,job}
    }
    fn router(&self)->Arc<OpenAiCompatibleVoiceRouter> {
        Arc::new(OpenAiCompatibleVoiceRouter::with_completion(Arc::new(HostDialogueClient::new(self.endpoint.clone(),"stream-fixture".into()).unwrap())))
    }
}

#[tokio::test]
async fn scoped_client_delivers_validated_sentences_before_completion() {
    let server=ModelStream::new().await;let router=server.router();let (tx,mut rx)=tokio::sync::mpsc::channel(4);
    let route=tokio::spawn(async move {router.route_scoped_stream(&VoiceDialogueRequest::scoped("stream-session","stream-turn","你好"),"你好",None,&[],tx).await});
    let first=tokio::time::timeout(Duration::from_secs(2),rx.recv()).await.unwrap().unwrap();
    assert_eq!(first.text,"你好。");assert!(!server.completed.load(Ordering::SeqCst));assert!(!route.is_finished());
    server.release.notify_one();
    assert_eq!(rx.recv().await.unwrap().text,"我在听。");
    let (decision,_)=route.await.unwrap();assert_eq!(decision.unwrap().reply_tts.as_deref(),Some("你好。我在听。"));
    assert_eq!(server.calls.load(Ordering::SeqCst),1);
}

#[derive(Default)]
struct SpeechSink {
    frames:std::sync::Mutex<Vec<Vec<u8>>>,
    wrote:tokio::sync::Notify,
    drains:AtomicUsize,
    draining:tokio::sync::Notify,
    consumed:tokio::sync::Notify,
}
#[async_trait::async_trait]
impl nomifun_voice_terminal::playback::AudioSink for SpeechSink {
    async fn write_frame(&self,frame:Vec<u8>)->Result<(),nomifun_voice_terminal::playback::AudioSinkError>{self.frames.lock().unwrap().push(frame);self.wrote.notify_one();Ok(())}
    async fn flush(&self)->Result<(),nomifun_voice_terminal::playback::AudioSinkError>{self.frames.lock().unwrap().clear();Ok(())}
    async fn drain(&self)->Result<(),nomifun_voice_terminal::playback::AudioSinkError>{self.drains.fetch_add(1,Ordering::SeqCst);self.draining.notify_one();self.consumed.notified().await;Ok(())}
}
#[derive(Default)]
struct SentenceSpeech(std::sync::Mutex<Vec<String>>,AtomicBool);
#[async_trait::async_trait]
impl nomifun_voice_terminal::tts::TtsProvider for SentenceSpeech {
    async fn synthesize_stream(&self,text:&str,_:&str)->Result<tokio::sync::mpsc::Receiver<Vec<u8>>,nomifun_voice_terminal::tts::TtsError>{
        let count={let mut calls=self.0.lock().unwrap();calls.push(text.into());calls.len()};
        if self.1.load(Ordering::SeqCst)&&count>1{return Err(nomifun_voice_terminal::tts::TtsError::Synthesis("controlled second sentence failure".into()));}
        let(tx,rx)=tokio::sync::mpsc::channel(1);
        tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05;2400],24000)).unwrap();Ok(rx)
    }
}
struct StreamingWorker {
    server:ModelStream,
    runtime:voice_companion::voice::VoiceCompanionRuntime,
    producer:nomifun_audio::voice_pipeline::SpscProducer,
    sink:Arc<SpeechSink>,
    speech:Arc<SentenceSpeech>,
    worker:tokio::task::JoinHandle<()>,
    _models:tempfile::TempDir,
}
impl Drop for StreamingWorker {fn drop(&mut self){self.producer.close();self.worker.abort();}}
impl StreamingWorker {
    async fn new()->Self {Self::with_error(false).await}
    async fn with_error(fail:bool)->Self {
        let server=ModelStream::with_error(fail).await;let models=tempfile::tempdir().unwrap();
        let(capture,producer)=nomifun_audio::voice_pipeline::VoiceCapturePipeline::new(Default::default());
        let(runtime,_)=voice_companion::voice::VoiceCompanionRuntime::from_capture(
            Arc::new(nomifun_voice_models::ModelManager::new(models.path())),Arc::new(voice_companion::oobe::OobeManager::new_bypassed()),Arc::new(tokio::sync::Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("你好")),server.router());
        let sink=Arc::new(SpeechSink::default());let speech=Arc::new(SentenceSpeech::default());
        let runtime=runtime.with_test_playback(Arc::new(nomifun_voice_terminal::playback::PlaybackPipeline::new(sink.clone())),speech.clone());
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
        let worker=runtime.start_worker();Self{server,runtime,producer,sink,speech,worker,_models:models}
    }
    async fn speak(&self){
        self.runtime.handle_wake().await.unwrap();let began=tokio::time::Instant::now();
        for frame in 0..90 {
            let pcm:Vec<f32>=(0..320).map(|i|if frame<25{0.03*(i as f32*std::f32::consts::TAU*200.0/16000.0).sin()}else{0.0}).collect();
            assert_eq!(self.producer.push(&pcm),pcm.len());tokio::time::sleep_until(began+Duration::from_millis((frame+1)*20)).await;
        }
    }
}

#[tokio::test]
async fn production_worker_plays_first_sentence_before_model_end_and_follows_up_only_after_final_drain(){
    let flow=StreamingWorker::new().await;flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2),flow.sink.wrote.notified()).await.expect("first sentence must reach the real playback pipeline before model completion");
    assert!(!flow.server.completed.load(Ordering::SeqCst));assert_eq!(flow.sink.drains.load(Ordering::SeqCst),0);
    assert_eq!(*flow.speech.0.lock().unwrap(),vec!["你好。"]);
    assert!(flow.runtime.snapshot().follow_up_deadline_ms.is_none());
    flow.server.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2),flow.sink.draining.notified()).await.unwrap();
    assert_eq!(flow.sink.drains.load(Ordering::SeqCst),1);
    assert_eq!(flow.runtime.snapshot().conversation.last().unwrap().reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Complete),"model completion is distinct from audio consumption");
    assert_eq!(*flow.speech.0.lock().unwrap(),vec!["你好。","我在听。"]);
    assert_eq!(flow.runtime.snapshot().assistant_text.as_deref(),Some("你好。我在听。"));
    assert!(flow.runtime.snapshot().follow_up_deadline_ms.is_none());
    flow.sink.consumed.notify_one();
    tokio::time::timeout(Duration::from_secs(2),async {while flow.runtime.snapshot().follow_up_deadline_ms.is_none(){tokio::time::sleep(Duration::from_millis(5)).await;}}).await.unwrap();
    assert_eq!(flow.server.calls.load(Ordering::SeqCst),1);
}

#[tokio::test]
async fn stopping_partial_speech_flushes_audio_and_ignores_late_model_tail(){
    let flow=StreamingWorker::new().await;flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2),flow.sink.wrote.notified()).await.unwrap();
    let snap=flow.runtime.snapshot();
    flow.runtime.stop_session(&nomifun_api_types::voice::VoiceSessionControlRequest{instance_id:snap.instance_id,session_id:snap.session_id,turn_id:snap.turn_id}).await.unwrap();
    flow.server.release.notify_one();tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(flow.sink.frames.lock().unwrap().is_empty());
    assert_eq!(*flow.speech.0.lock().unwrap(),vec!["你好。"]);
    assert_eq!(flow.runtime.snapshot().assistant_text.as_deref(),Some("你好。"));
    assert!(flow.runtime.snapshot().follow_up_deadline_ms.is_none());
    flow.runtime.handle_wake().await.expect("stop must leave wake usable");
}
#[tokio::test]
async fn partial_model_failure_keeps_text_and_allows_a_fresh_wake(){
    let flow=StreamingWorker::with_error(true).await;flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2),flow.sink.wrote.notified()).await.unwrap();
    flow.server.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2),async{loop{let snap=flow.runtime.snapshot();if snap.phase==nomifun_api_types::voice::VoiceSessionPhase::Idle && snap.error.is_some(){break;}tokio::time::sleep(Duration::from_millis(5)).await;}}).await.unwrap();
    assert!(flow.sink.frames.lock().unwrap().is_empty());
    assert_eq!(flow.runtime.snapshot().assistant_text.as_deref(),Some("你好。"));
    assert_eq!(flow.runtime.snapshot().error.unwrap().code,"model_timeout");
    assert_eq!(*flow.speech.0.lock().unwrap(),vec!["你好。"]);
    flow.runtime.handle_wake().await.expect("partial stream failure must reset the acoustic driver");
}

#[tokio::test]
async fn a_later_synthesis_failure_keeps_the_complete_answer_and_enters_text_follow_up(){
    let flow=StreamingWorker::new().await;flow.speech.1.store(true,Ordering::SeqCst);flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2),flow.sink.wrote.notified()).await.unwrap();flow.server.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2),async {while flow.runtime.snapshot().phase!=nomifun_api_types::voice::VoiceSessionPhase::TextFollowUp{tokio::time::sleep(Duration::from_millis(5)).await;}}).await.unwrap();
    assert!(flow.runtime.snapshot().follow_up_deadline_ms.is_some(),"TTS failure must arm the actual follow-up window, not only change the displayed phase");
    let snap=flow.runtime.snapshot();assert_eq!(snap.assistant_text.as_deref(),Some("你好。我在听。"));assert_eq!(snap.error.unwrap().code,"tts_failed");
    assert!(flow.sink.frames.lock().unwrap().is_empty(),"text follow-up must not begin over queued speech");
    assert_eq!(snap.conversation.last().unwrap().reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Complete));
    assert_eq!(flow.server.calls.load(Ordering::SeqCst),1);
}

#[tokio::test]
async fn muted_stream_preserves_text_without_synthesis_and_opens_text_follow_up(){
    let flow=StreamingWorker::new().await;
    flow.runtime.update_feedback(serde_json::from_value(serde_json::json!({"speak_replies":false})).unwrap()).unwrap();
    flow.speak().await;flow.server.release.notify_one();flow.sink.consumed.notify_one();
    tokio::time::timeout(Duration::from_secs(2),async{while flow.runtime.snapshot().follow_up_deadline_ms.is_none(){tokio::time::sleep(Duration::from_millis(5)).await;}}).await.unwrap();
    assert!(flow.speech.0.lock().unwrap().is_empty(),"muting replies must prevent all streamed TTS requests");
    assert!(flow.sink.frames.lock().unwrap().is_empty());
    assert_eq!(flow.runtime.snapshot().phase,nomifun_api_types::voice::VoiceSessionPhase::TextFollowUp);
    assert_eq!(flow.runtime.snapshot().assistant_text.as_deref(),Some("你好。我在听。"));
    assert_eq!(flow.server.calls.load(Ordering::SeqCst),1);
}

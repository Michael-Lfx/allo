//! Host-owned model resolution. No task runtime, tools, or provider keys in the daemon.
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use nomifun_api_types::voice::{VoiceDialogueRequest, VoiceDialogueResponse, VoiceModelRef};
use nomifun_db::{IProviderModelRepository, IProviderRepository};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

pub type VoiceSentenceCallback=Arc<dyn Fn(nomifun_api_types::voice::VoiceDialogueSentence)->Result<(),String>+Send+Sync>;
#[async_trait]
pub trait VoiceDialogueCompleter: Send + Sync {
    async fn resolve_model(&self, choice: Option<&VoiceModelRef>) -> Result<VoiceModelRef, String>;
    async fn complete_stream(&self,prompt:&str,choice:Option<&VoiceModelRef>,_on_sentence:VoiceSentenceCallback)->Result<String,String> {
        self.complete(prompt,choice).await
    }
    async fn complete(
        &self,
        prompt: &str,
        choice: Option<&VoiceModelRef>,
    ) -> Result<String, String>;
}

pub struct ProviderVoiceDialogue {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

#[async_trait]
impl VoiceDialogueCompleter for ProviderVoiceDialogue {
    async fn resolve_model(&self, choice: Option<&VoiceModelRef>) -> Result<VoiceModelRef, String> {
        let selected = match choice {
            Some(reference) => reference.clone(),
            None => {
                // Voice defaults favor quick replies; explicit selections remain authoritative.
                // Resolve catalog references through the same capability checks as manual choices.
                for model in ["AIPC-qwen3.8-flash", "AIPC-deepseek-v4.1-flash"] {
                    let preferred = VoiceModelRef {
                        provider_id: nomifun_common::FLOWY_BUILTIN_PROVIDER_ID.into(),
                        model: model.into(),
                    };
                    if let Ok(selected) = self.resolve_model(Some(&preferred)).await {
                        return Ok(selected);
                    }
                }
                let (provider_id, model) = nomifun_ai_agent::resolve_flowy_cloud_model(
                    &self.provider_repo,
                    &self.provider_model_repo,
                )
                .await
                .ok_or_else(|| "model_unavailable".to_string())?;
                VoiceModelRef { provider_id, model }
            }
        };
        let provider = self
            .provider_repo
            .find_by_id(&selected.provider_id)
            .await
            .map_err(|_| "model_unavailable")?
            .ok_or("model_unavailable")?;
        if !provider.enabled || provider.platform != "openai" {
            return Err("model_unavailable".into());
        }
        let rows = self
            .provider_model_repo
            .list_for_provider(&selected.provider_id)
            .await
            .map_err(|_| "model_unavailable")?;
        let row = rows
            .iter()
            .find(|row| row.model == selected.model && row.enabled)
            .ok_or("model_unavailable")?;
        let tasks: Vec<nomifun_api_types::ModelTask> =
            serde_json::from_str(&row.tasks).unwrap_or_default();
        let tasks = if tasks.is_empty() {
            nomifun_api_types::derive_tasks_and_traits(&provider.platform, &row.model).0
        } else {
            tasks
        };
        if !tasks.contains(&nomifun_api_types::ModelTask::Chat)
            || row.protocol.as_deref().is_some_and(|p| p != "openai")
        {
            return Err("model_unavailable".into());
        }
        Ok(selected)
    }

    async fn complete(&self,prompt:&str,choice:Option<&VoiceModelRef>)->Result<String,String> {
        self.complete_mode(prompt,choice,None).await
    }
    async fn complete_stream(&self,prompt:&str,choice:Option<&VoiceModelRef>,on_sentence:VoiceSentenceCallback)->Result<String,String> {
        self.complete_mode(prompt,choice,Some(on_sentence)).await
    }

}

impl ProviderVoiceDialogue {
    async fn complete_mode(&self,prompt:&str,choice:Option<&VoiceModelRef>,on_sentence:Option<VoiceSentenceCallback>)->Result<String,String> {
        let selected=self.resolve_model(choice).await?;
        let config=nomifun_ai_agent::resolve_provider_config(&self.provider_repo,&self.provider_model_repo,&self.encryption_key,&selected.provider_id,&selected.model,&self.workspace)
            .await.map_err(|_|"model_unavailable")?;
        tracing::info!(provider_id=%selected.provider_id,model=%selected.model,"voice_dialogue_model_selected");
        let system=if on_sentence.is_some() {
            "请遵循用户提供的语音决策 JSON 契约。这是低延迟 JSONL 模式，只输出连续 JSON 对象，不加 Markdown。第一对象必须遵循用户的 VoiceDecision 契约，立即结束这个对象：chat 时 reply_tts 只能给首个完整短句（8至16字，禁止把多句合并），understood_request 只用20字内概括；如果需要第二句，必须另起对象，每个后续对象只能是 {\"reply_delta\":\"下一完整短句\"}。chat 合计最多60字、最多4句。非chat只有第一对象，禁止后续句子。后续对象不能改变意图、任务或理解。不要输出思考过程。对于天气、文件读写、系统操作等多步或需要外部工具的任务，必须输出 agent_task 委派执行，严禁编造实时数据。"
        } else {"请遵循用户提供的语音决策 JSON 契约，仅输出 JSON。对于天气、文件读写、系统操作等需要外部工具的任务，必须输出 agent_task 委派执行，严禁编造实时数据。"};
        let started=std::time::Instant::now();let mut first=true;
        let mut parser=nomifun_api_types::voice::VoiceDecisionStream::default();
        let invalid=tokio::sync::Notify::new();let mut stream_error=None;
        let result={
            let completion=nomifun_ai_agent::streaming_completion_no_thinking(&config,system,vec![nomifun_ai_agent::user_message(prompt)],768,|delta| {
                if first && !delta.is_empty() {first=false;tracing::info!(elapsed_ms=started.elapsed().as_millis() as u64,"voice_dialogue_first_text");}
                if let Some(callback)=&on_sentence {
                    if stream_error.is_some(){return;}
                    match parser.push(delta) {
                        Ok(sentences)=>for sentence in sentences {
                            if let Err(error)=callback(sentence) {stream_error=Some(error);invalid.notify_one();break;}
                        },
                        Err(code)=>{stream_error=Some(code.into());invalid.notify_one();},
                    }
                }
            });
            tokio::pin!(completion);
            tokio::select! {
                biased;
                result=&mut completion=>result.map_err(|error| {
                    let detail=error.to_string().to_lowercase();
                    let code=if detail.contains("401")||detail.contains("403") {"model_auth_failed"}
                        else if detail.contains("timeout")||detail.contains("connect") {"model_network_failed"}else{"model_rejected"};
                    tracing::warn!(code,"voice_dialogue_provider_failed");code.to_string()
                }),
                _=invalid.notified()=>Err("model_response_invalid".into()),
            }
        };
        tracing::info!(elapsed_ms=started.elapsed().as_millis() as u64,ok=result.is_ok(),"voice_dialogue_provider_complete");
        if let Some(error)=stream_error {return Err(error);}
        let output=result?;
        if on_sentence.is_some() {
            let decision=parser.finish().map_err(str::to_owned)?;
            serde_json::to_string(&decision).map_err(|_|"model_response_invalid".into())
        }else{Ok(output)}
    }
}

#[derive(Clone,serde::Serialize,serde::Deserialize)]
pub struct FrozenVoiceExecution {
    pub config:nomifun_api_types::voice::VoiceExecutionConfig,
    pub preset:Option<nomifun_api_types::ResolvedPresetSnapshot>,
    #[serde(default)]
    pub handoff_content:Option<String>,
    #[serde(default)]
    pub display_text:Option<String>,
    #[serde(default)]
    pub conversation_id:Option<String>,
    #[serde(default)]
    pub message_id:Option<String>,
    #[serde(default)]
    pub voice_context_digest:Option<String>,
}

#[async_trait]
pub trait VoiceDialogueRecorder: Send + Sync {
    async fn freeze_session(&self,_session_id:&str)->Result<Option<nomifun_api_types::voice::VoiceBindingSnapshot>,VoiceDialogueRecorderError> {Ok(None)}

    async fn task_configuration(&self, _command:&nomifun_api_types::voice::VoiceTaskCommand) -> Result<Option<FrozenVoiceExecution>,String> {
        Err("binding_invalid".into())
    }

    async fn fallback_execution(&self) -> Result<Option<nomifun_api_types::voice::VoiceExecutionConfig>, String> {
        Ok(None)
    }
    async fn preflight(
        &self,
        request: &VoiceDialogueRequest,
    ) -> Result<VoiceDialoguePreflight, VoiceDialogueRecorderError>;

    async fn record_pair(
        &self,
        conversation_id: &str,
        request: &VoiceDialogueRequest,
        assistant_content: &str,
    ) -> Result<(), VoiceDialogueRecorderError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceDialoguePreflight {
    Ready { conversation_id: String },
    Generated { conversation_id: String, content: String },
    Cached { content: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceDialogueRecorderError {
    DecisionInvalid,
    GenerationUnknown,
    Conflict,
    BindingInvalid(String),
    BindingBusy(String),
    WriteFailed(String),
}

pub const DEFAULT_VOICE_DIALOGUE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
pub const VOICE_FIRST_RESPONSE_TIMEOUT: std::time::Duration = DEFAULT_VOICE_DIALOGUE_TIMEOUT;
pub const VOICE_STREAM_TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Clone)]
struct DialogueState {
    completer: Arc<dyn VoiceDialogueCompleter>,
    selection: Arc<RwLock<Option<VoiceModelRef>>>,
    credential: String,
    timeout: std::time::Duration,
    recorder: Option<Arc<dyn VoiceDialogueRecorder>>,
}

pub struct VoiceDialogueBridge {
    pub endpoint: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for VoiceDialogueBridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl VoiceDialogueBridge {
    pub async fn start(
        completer: Arc<dyn VoiceDialogueCompleter>,
        selection: Arc<RwLock<Option<VoiceModelRef>>>,
        credential: String,
        task_routes: Router,
    ) -> Result<Self, String> {
        Self::start_with_recorder(
            completer,
            selection,
            credential,
            task_routes,
            None,
        )
        .await
    }

    pub async fn start_with_recorder(
        completer: Arc<dyn VoiceDialogueCompleter>,
        selection: Arc<RwLock<Option<VoiceModelRef>>>,
        credential: String,
        task_routes: Router,
        recorder: Option<Arc<dyn VoiceDialogueRecorder>>,
    ) -> Result<Self, String> {
        Self::start_internal(
            completer,
            selection,
            credential,
            task_routes,
            recorder,
            VOICE_STREAM_TOTAL_TIMEOUT,
        )
        .await
    }

    pub async fn start_internal(
        completer: Arc<dyn VoiceDialogueCompleter>,
        selection: Arc<RwLock<Option<VoiceModelRef>>>,
        credential: String,
        task_routes: Router,
        recorder: Option<Arc<dyn VoiceDialogueRecorder>>,
        timeout: std::time::Duration,
    ) -> Result<Self, String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| "dialogue_bridge_start_failed")?;
        let address = listener
            .local_addr()
            .map_err(|_| "dialogue_bridge_start_failed")?;
        let app = Router::new()
            .route("/complete", post(complete))
            .with_state(DialogueState {
                completer,
                selection,
                credential,
                timeout,
                recorder,
            })
            .merge(task_routes)
            .layer(axum::extract::DefaultBodyLimit::max(64 * 1024));
        let task = tokio::spawn(async move {
            if axum::serve(listener, app).await.is_err() {
                tracing::warn!("voice_dialogue_bridge_stopped");
            }
        });
        Ok(Self {
            endpoint: format!("http://{address}/complete"),
            task,
        })
    }
}

async fn complete(
    State(state): State<DialogueState>,
    headers: HeaderMap,
    Json(request): Json<VoiceDialogueRequest>,
) -> Response {
    if headers
        .get("x-ai-mate-control-token")
        .and_then(|v| v.to_str().ok())
        != Some(state.credential.as_str())
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"code":"model_auth_failed"})),
        )
            .into_response();
    }
    if request.prompt.trim().is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let mut generated = None;
    let ready_conversation_id = if let Some(recorder) = &state.recorder {
        match recorder.preflight(&request).await {
            Ok(VoiceDialoguePreflight::Cached { content }) => {
                return Json(VoiceDialogueResponse { content, record_error:None }).into_response();
            }
            Ok(VoiceDialoguePreflight::Ready { conversation_id }) => Some(conversation_id),
            Ok(VoiceDialoguePreflight::Generated { conversation_id, content }) => {
                generated=Some(content); Some(conversation_id)
            },
            Err(VoiceDialogueRecorderError::DecisionInvalid) => {return (StatusCode::BAD_GATEWAY,Json(serde_json::json!({"code":"model_response_invalid"}))).into_response();},
            Err(VoiceDialogueRecorderError::GenerationUnknown) => {
                return (StatusCode::CONFLICT,Json(serde_json::json!({"code":"voice_generation_unknown"}))).into_response();
            },
            Err(VoiceDialogueRecorderError::Conflict) => {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"code":"voice_record_conflict"})),
                )
                    .into_response();
            }
            Err(VoiceDialogueRecorderError::BindingInvalid(reason)) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"code":"binding_invalid", "reason": reason})),
                )
                    .into_response();
            }
            Err(VoiceDialogueRecorderError::BindingBusy(reason)) => {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"code":"binding_busy", "reason": reason})),
                )
                    .into_response();
            }
            Err(VoiceDialogueRecorderError::WriteFailed(_)) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"code":"voice_record_write_failed"})),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    let choice = state
        .selection
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if request.stream {return stream_response(state,request,choice,ready_conversation_id,generated);}
    let result = if let Some(content)=generated {Ok(Ok(content))} else {tokio::time::timeout(
        state.timeout.min(DEFAULT_VOICE_DIALOGUE_TIMEOUT),
        {use tracing::Instrument;state.completer.complete(&request.prompt, choice.as_ref()).instrument(tracing::info_span!("voice_dialogue",session_id=?request.session_id,turn_id=?request.turn_id,record_id=?request.voice_record_id))},
    )
    .await};
    match result {
        Ok(Ok(content)) if !content.trim().is_empty() => {
            if let (Some(recorder), Some(conv_id)) = (&state.recorder, ready_conversation_id) {
                if let Err(error)=recorder.record_pair(&conv_id, &request, &content).await {
                    let code=record_error_code(&error);
                    return Json(VoiceDialogueResponse {content,record_error:Some(code.into())}).into_response();
                }
            }
            Json(VoiceDialogueResponse { content, record_error:None }).into_response()
        }
        Ok(Ok(_)) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"code":"model_response_invalid"})),
        )
            .into_response(),
        Ok(Err(code)) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"code":code})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(serde_json::json!({"code":"model_timeout"})),
        )
            .into_response(),
    }
}
fn record_error_code(error:&VoiceDialogueRecorderError)->&'static str {
    match error {
        VoiceDialogueRecorderError::DecisionInvalid=>"model_response_invalid",
        VoiceDialogueRecorderError::Conflict=>"voice_record_conflict",
        VoiceDialogueRecorderError::BindingInvalid(_)=>"binding_invalid",
        VoiceDialogueRecorderError::BindingBusy(_)=>"binding_busy",
        VoiceDialogueRecorderError::WriteFailed(_)=>"voice_projection_write_failed",
        VoiceDialogueRecorderError::GenerationUnknown=>"voice_generation_unknown",
    }
}

fn stream_response(state:DialogueState,request:VoiceDialogueRequest,choice:Option<VoiceModelRef>,conversation_id:Option<String>,generated:Option<String>)->Response {
    use nomifun_api_types::voice::VoiceDialogueFrame;
    let (tx,rx)=tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let work=async {
            let result=if let Some(content)=generated {Ok(content)}else{
                let (activity,mut changed)=tokio::sync::watch::channel(0usize);
                let pulse=activity.clone();let output=tx.clone();let started=std::time::Instant::now();
                let callback:VoiceSentenceCallback=Arc::new(move |sentence| {
                    output.try_send(VoiceDialogueFrame::Sentence{sentence}).map_err(|_|"voice_stream_closed".to_string())?;
                    pulse.send_modify(|n|*n+=1);
                    Ok(())
                });
                use tracing::Instrument;
                let completion=state.completer.complete_stream(&request.prompt,choice.as_ref(),callback)
                    .instrument(tracing::info_span!("voice_dialogue",session_id=?request.session_id,turn_id=?request.turn_id,record_id=?request.voice_record_id));
                tokio::pin!(completion);
                let idle=tokio::time::sleep(state.timeout.min(VOICE_FIRST_RESPONSE_TIMEOUT));tokio::pin!(idle);
                let total=tokio::time::sleep(state.timeout);tokio::pin!(total);
                loop {tokio::select! {
                    biased;
                    result=&mut completion=>break result,
                    _=changed.changed()=>{
                        tracing::info!(session_id=?request.session_id,turn_id=?request.turn_id,elapsed_ms=started.elapsed().as_millis() as u64,sentences=*changed.borrow(),"voice_dialogue_sentence");
                        idle.as_mut().reset(tokio::time::Instant::now()+state.timeout.min(VOICE_FIRST_RESPONSE_TIMEOUT));
                    },
                    _=&mut idle=>{
                        tracing::warn!(session_id=?request.session_id,turn_id=?request.turn_id,has_sentence=*changed.borrow()>0,"voice_dialogue_idle_timeout");
                        break Err("model_timeout".into());
                    },
                    _=&mut total=>{tracing::warn!(session_id=?request.session_id,turn_id=?request.turn_id,"voice_dialogue_total_timeout");break Err("model_timeout".into());},
                }}
            };
            let terminal=match result {
                Ok(content) if !content.trim().is_empty()=>{
                    let record_error=if let (Some(recorder),Some(id))=(&state.recorder,conversation_id.as_ref()) {
                        recorder.record_pair(id,&request,&content).await.err().map(|error|record_error_code(&error).to_owned())
                    }else{None};
                    VoiceDialogueFrame::Complete{response:VoiceDialogueResponse{content,record_error}}
                },
                Ok(_)=>VoiceDialogueFrame::Error{code:"model_response_invalid".into()},
                Err(code)=>VoiceDialogueFrame::Error{code},
            };
            let _=tx.send(terminal).await;
        };
        // Disconnecting a stopped turn drops the provider future; it cannot publish a late result.
        tokio::select!{_ = tx.closed()=>{},_ = work=>{}}
    });
    let body=axum::body::Body::from_stream(futures_util::stream::unfold(rx,|mut rx|async {
        rx.recv().await.map(|frame|{
            let line=serde_json::to_string(&frame).expect("voice frames contain only serializable fields")+"\n";
            (Ok::<_,std::convert::Infallible>(line),rx)
        })
    }));
    ([ (axum::http::header::CONTENT_TYPE,"application/x-ndjson") ],body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    #[ignore = "explicit single cloud model probe using existing provider configuration; no microphone or speaker"]
    async fn configured_flowy_provider_online_probe() {
        let (live,pool)=configured_probe_provider().await;
        let selected = live.resolve_model(None).await.unwrap();
        println!("Configured model: {}", selected.model);
        let prompt = r#"固定连接测试，没有用户录音。请仅返回 JSON：{"intent_type":"chat","understood_request":"测试语音对话模型是否可用","reply_tts":"已收到语音连接测试。"}"#;
        let started = std::time::Instant::now();
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            live.complete(prompt, Some(&selected)),
        )
        .await
        .expect("configured provider timeout")
        .expect("configured provider must return real output");
        let value: serde_json::Value =
            serde_json::from_str(&output).expect("real model must return JSON");
        assert_eq!(value["intent_type"], "chat");
        assert!(
            value["reply_tts"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
        println!(
            "Real provider reply validated; elapsed_ms={}",
            started.elapsed().as_millis()
        );
        pool.close().await;
    }

    async fn configured_probe_provider()->(ProviderVoiceDialogue,sqlx::SqlitePool) {
        let data = PathBuf::from(
            std::env::var_os("VOICE_HOST_DATA_DIR").expect("set existing host data directory"),
        );
        assert!(
            data.join("encryption_key").is_file(),
            "probe cannot create keys or data"
        );
        let key = crate::config::load_or_create_data_encryption_key(&data, "").unwrap();
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(data.join("flowy-backend.db"))
            .read_only(true)
            .create_if_missing(false);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        let live = ProviderVoiceDialogue {
            provider_repo: Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
            provider_model_repo: Arc::new(nomifun_db::SqliteProviderModelRepository::new(
                pool.clone(),
            )),
            encryption_key: key,
            workspace: data,
        };
        (live,pool)
    }

    #[tokio::test]
    #[ignore="explicit cloud dialogue and Edge synthesis latency sampling; fixed text, no real audio device"]
    async fn configured_streaming_worker_latency_probe() {
        use voice_companion::replay_terminal::{completion::HostDialogueClient,router::OpenAiCompatibleVoiceRouter,playback::{AudioSink,AudioSinkError,PlaybackPipeline},tts::EdgeTtsProvider};
        struct ProbeSink {first:std::sync::Mutex<Option<std::time::Instant>>,frames:AtomicUsize}
        #[async_trait]
        impl AudioSink for ProbeSink {
            async fn write_frame(&self,frame:Vec<u8>)->Result<(),AudioSinkError>{
                assert!(frame.starts_with(b"RIFF"),"real Edge synthesis must deliver decoded WAV");
                self.first.lock().unwrap().get_or_insert_with(std::time::Instant::now);self.frames.fetch_add(1,Ordering::SeqCst);Ok(())
            }
            async fn flush(&self)->Result<(),AudioSinkError>{Ok(())}
            async fn drain(&self)->Result<(),AudioSinkError>{Ok(())}
        }
        let _=tracing_subscriber::fmt().with_env_filter("nomifun_app::voice_dialogue=info,voice_companion::voice=info,nomifun_voice_terminal::playback=info,nomifun_voice_terminal::tts=info").without_time().try_init();
        let (live,pool)=configured_probe_provider().await;let selected=live.resolve_model(None).await.unwrap();
        let bridge=VoiceDialogueBridge::start(Arc::new(live),Arc::new(RwLock::new(Some(selected.clone()))),"fixed-latency-probe".into(),Router::new()).await.unwrap();
        let models=tempfile::tempdir().unwrap();
        let(capture,producer)=nomifun_audio::voice_pipeline::VoiceCapturePipeline::new(Default::default());
        let(runtime,_)=voice_companion::voice::VoiceCompanionRuntime::from_capture(
            Arc::new(nomifun_voice_models::ModelManager::new(models.path())),Arc::new(voice_companion::oobe::OobeManager::new_bypassed()),Arc::new(tokio::sync::Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("你好，请用两个很短的句子介绍你自己。")),
            Arc::new(OpenAiCompatibleVoiceRouter::with_completion(Arc::new(HostDialogueClient::new(bridge.endpoint.clone(),"fixed-latency-probe".into()).unwrap()))));
        let sink=Arc::new(ProbeSink{first:std::sync::Mutex::new(None),frames:AtomicUsize::new(0)});
        let runtime=runtime.with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())),Arc::new(EdgeTtsProvider::new()));
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
        let worker=runtime.start_worker();let mut reports=Vec::new();
        for sample in 0..3 {
            if sample==2 {tokio::time::timeout(std::time::Duration::from_secs(6),async {while runtime.snapshot().phase!=nomifun_api_types::voice::VoiceSessionPhase::Idle{tokio::time::sleep(std::time::Duration::from_millis(10)).await;}}).await.unwrap();}
            if sample!=1 {runtime.handle_wake().await.unwrap();}
            let mut events=runtime.event_sender().subscribe();*sink.first.lock().unwrap()=None;sink.frames.store(0,Ordering::SeqCst);
            let began=std::time::Instant::now();let feed=async {
                let began=tokio::time::Instant::now();
                for frame in 0..90 {let pcm:Vec<f32>=(0..320).map(|i|if frame<25{0.03*(i as f32*std::f32::consts::TAU*200.0/16000.0).sin()}else{0.0}).collect();producer.push(&pcm);tokio::time::sleep_until(began+std::time::Duration::from_millis((frame+1)*20)).await;}
            };
            let observe=async {
                let mut ready=None;let mut sentence=None;let mut failed=None;let mut segment_chars=Vec::new();
                tokio::time::timeout(std::time::Duration::from_secs(45),async {loop {
                    match events.recv().await.unwrap() {
                        nomifun_api_types::voice::VoiceCompanionEvent::Transcribing=>{ready=Some(std::time::Instant::now());},
                        nomifun_api_types::voice::VoiceCompanionEvent::ReplySentence{text,..}=>{sentence.get_or_insert_with(std::time::Instant::now);segment_chars.push(text.chars().count());},
                        nomifun_api_types::voice::VoiceCompanionEvent::Error{code,..}=>{failed=Some(code);},
                        nomifun_api_types::voice::VoiceCompanionEvent::FollowUpWindow{..}|nomifun_api_types::voice::VoiceCompanionEvent::TextFollowUpWindow{..}|nomifun_api_types::voice::VoiceCompanionEvent::SessionEnded{..}=>break,
                        _=>{}
                    }
                }}).await.expect("probe must terminate without a physical audio device");
                let first=*sink.first.lock().unwrap();
                let report=serde_json::json!({"sample":sample,"scenario":if sample==0{"first"}else if sample==1{"follow_up"}else{"rewake"},"model":selected.model,"from_utterance_ready_first_sentence_ms":ready.zip(sentence).map(|(r,s)|s.saturating_duration_since(r).as_millis()),"from_utterance_ready_first_sink_ms":ready.zip(first).map(|(r,s)|s.saturating_duration_since(r).as_millis()),"feed_to_done_ms":began.elapsed().as_millis(),"frames":sink.frames.load(Ordering::SeqCst),"segment_chars":segment_chars,"error":failed});
                println!("VOICE_LATENCY_SAMPLE {report}");report
            };
            let((),report)=tokio::join!(feed,observe);reports.push(report);
            if sample!=1 && runtime.snapshot().phase!=nomifun_api_types::voice::VoiceSessionPhase::FollowUp {break;}
        }
        producer.close();worker.abort();drop(bridge);pool.close().await;
        assert_eq!(reports.len(),3,"all three scenarios must run");
        assert!(reports.iter().all(|r|r["error"].is_null()&&r["frames"].as_u64().unwrap()>0),"real cloud/Edge sample failure must be reported");
    }


    struct PendingStream {dropped:Arc<AtomicUsize>,emit:bool}
    struct Dropped(Arc<AtomicUsize>);
    impl Drop for Dropped {fn drop(&mut self){self.0.fetch_add(1,Ordering::SeqCst);}}
    #[async_trait]
    impl VoiceDialogueCompleter for PendingStream {
        async fn resolve_model(&self,_:Option<&VoiceModelRef>)->Result<VoiceModelRef,String>{Ok(VoiceModelRef{provider_id:"fixture".into(),model:"fixture".into()})}
        async fn complete(&self,_:&str,_:Option<&VoiceModelRef>)->Result<String,String>{std::future::pending().await}
        async fn complete_stream(&self,_:&str,_:Option<&VoiceModelRef>,callback:VoiceSentenceCallback)->Result<String,String>{
            let _guard=Dropped(self.dropped.clone());
            if self.emit {callback(nomifun_api_types::voice::VoiceDialogueSentence{text:"第一句。".into(),understood_request:None})?;}
            std::future::pending().await
        }
    }
    #[tokio::test]
    async fn streaming_timeout_and_disconnect_drop_the_model_future(){
        use futures_util::StreamExt;
        for (emit,disconnect) in [(false,false),(true,false),(true,true)] {
            let dropped=Arc::new(AtomicUsize::new(0));
            let bridge=VoiceDialogueBridge::start_internal(Arc::new(PendingStream{dropped:dropped.clone(),emit}),Arc::new(RwLock::new(None)),"stream-boundary".into(),Router::new(),None,std::time::Duration::from_millis(if disconnect{1000}else{50})).await.unwrap();
            let mut req=VoiceDialogueRequest::scoped("timeout-session","timeout-turn","固定测试");req.stream=true;req.prompt="固定测试".into();
            let response=reqwest::Client::builder().no_proxy().build().unwrap().post(&bridge.endpoint).header("x-ai-mate-control-token","stream-boundary").json(&req).send().await.unwrap();
            if disconnect {
                let mut chunks=response.bytes_stream();assert!(chunks.next().await.unwrap().unwrap().len()>0);drop(chunks);
            }else{let body=response.text().await.unwrap();assert!(body.contains("model_timeout"));assert!(!body.contains("\"type\":\"complete\""));}
            tokio::time::timeout(std::time::Duration::from_secs(2),async {while dropped.load(Ordering::SeqCst)==0{tokio::time::sleep(std::time::Duration::from_millis(5)).await;}}).await.unwrap();
            assert_eq!(dropped.load(Ordering::SeqCst),1);
        }
    }

    struct FixtureCompleter(AtomicUsize);
    #[async_trait]
    impl VoiceDialogueCompleter for FixtureCompleter {
        async fn resolve_model(
            &self,
            choice: Option<&VoiceModelRef>,
        ) -> Result<VoiceModelRef, String> {
            choice.cloned().ok_or_else(|| "model_unavailable".into())
        }
        async fn complete(
            &self,
            prompt: &str,
            choice: Option<&VoiceModelRef>,
        ) -> Result<String, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(format!(
                "{}:{prompt}",
                choice.ok_or("model_unavailable")?.model
            ))
        }
    }

    #[tokio::test]
    async fn private_bridge_authenticates_and_reads_changed_model_without_restarting() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let selection = Arc::new(RwLock::new(Some(VoiceModelRef {
            provider_id: "fixture-provider".into(),
            model: "first".into(),
        })));
        let bridge = VoiceDialogueBridge::start(
            completer.clone(),
            selection.clone(),
            "private-fixture".into(),
            Router::new(),
        )
        .await
        .unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let request = VoiceDialogueRequest::new("question");
        let denied = client
            .post(&bridge.endpoint)
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(completer.0.load(Ordering::SeqCst), 0);
        for model in ["first", "local-model"] {
            *selection.write().unwrap() = Some(VoiceModelRef {
                provider_id: "fixture-provider".into(),
                model: model.into(),
            });
            let reply = client
                .post(&bridge.endpoint)
                .header("x-ai-mate-control-token", "private-fixture")
                .json(&request)
                .send()
                .await
                .unwrap()
                .json::<VoiceDialogueResponse>()
                .await
                .unwrap();
            assert_eq!(reply.content, format!("{model}:question"));
        }
        assert_eq!(completer.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn provider_catalog_identity_and_existing_streaming_transport_are_used() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let raw =
            r#"{"intent_type":"chat","understood_request":"询问时间","reply_tts":"测试回答"}"#
                .to_string();
        let expected = raw.clone();
        let streaming=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let release=Arc::new(tokio::sync::Notify::new());
        let mode=streaming.clone();let gate=release.clone();
        let upstream=Router::new().route("/v1/chat/completions",post(move |headers:HeaderMap,Json(body):Json<serde_json::Value>| {
            let seen=seen.clone();let raw=raw.clone();let gated=mode.load(Ordering::SeqCst);let gate=gate.clone();
            async move {
                assert_eq!(body["model"],"AIPC-fixture-chat");
                assert_eq!(body["thinking"]["type"],"disabled","fast voice must explicitly disable provider reasoning");
                assert_eq!(headers.get("authorization").unwrap(),"Bearer fixture-user-token");
                assert_eq!(headers.get("token").unwrap(),"fixture-user-token");
                seen.fetch_add(1,Ordering::SeqCst);
                let (tx,rx)=tokio::sync::mpsc::channel(4);
                tokio::spawn(async move {
                    let delta=serde_json::json!({"choices":[{"delta":{"content":format!("{raw}{}",if gated {"\n"}else{""})},"finish_reason":null}]}).to_string();
                    if tx.send(Ok::<_,std::convert::Infallible>(axum::response::sse::Event::default().data(delta))).await.is_err(){return;}
                    if gated {
                        gate.notified().await;
                        let delta=serde_json::json!({"choices":[{"delta":{"content":"{\"reply_delta\":\"继续回答。\"}\n"},"finish_reason":null}]}).to_string();
                        if tx.send(Ok(axum::response::sse::Event::default().data(delta))).await.is_err(){return;}
                    }
                    let _=tx.send(Ok(axum::response::sse::Event::default().data("[DONE]"))).await;
                });
                axum::response::Sse::new(futures_util::stream::unfold(rx,|mut rx|async {rx.recv().await.map(|item|(item,rx))}))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });
        let db = nomifun_db::init_database_memory().await.unwrap();
        let provider_repo: Arc<dyn IProviderRepository> =
            Arc::new(nomifun_db::SqliteProviderRepository::new(db.pool().clone()));
        let model_repo: Arc<dyn IProviderModelRepository> = Arc::new(
            nomifun_db::SqliteProviderModelRepository::new(db.pool().clone()),
        );
        let key = [7; 32];
        let encrypted = nomifun_common::encrypt_string("fixture-user-token", &key).unwrap();
        let base = format!("http://{addr}/v1");
        provider_repo
            .create(nomifun_db::CreateProviderParams {
                provider_id: Some(nomifun_common::FLOWY_BUILTIN_PROVIDER_ID),
                platform: "openai",
                name: "Flowy Cloud",
                base_url: &base,
                api_key_encrypted: &encrypted,
                models: r#"["AIPC-fixture-chat"]"#,
                enabled: true,
                model_context_limits: None,
                model_protocols: None,
                model_descriptions: None,
                model_enabled: None,
                bedrock_config: None,
                is_full_url: false,
                sort_order: None,
            })
            .await
            .unwrap();
        model_repo
            .update(
                nomifun_common::FLOWY_BUILTIN_PROVIDER_ID,
                "AIPC-fixture-chat",
                &nomifun_db::ProviderModelUpdate {
                    tasks: Some(r#"["chat"]"#),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let live = Arc::new(ProviderVoiceDialogue {
            provider_repo,
            provider_model_repo: model_repo.clone(),
            encryption_key: key,
            workspace: std::env::temp_dir(),
        });
        let resolved = live.resolve_model(None).await.unwrap();
        assert_eq!(resolved.model, "AIPC-fixture-chat");
        let stale = VoiceModelRef {
            provider_id: resolved.provider_id.clone(),
            model: "qwen3.8-flash".into(),
        };
        assert_eq!(
            live.resolve_model(Some(&stale)).await.unwrap_err(),
            "model_unavailable"
        );
        let bridge = VoiceDialogueBridge::start(
            live.clone(),
            Arc::new(RwLock::new(None)),
            "private-fixture".into(),
            Router::new(),
        )
        .await
        .unwrap();
        let reply = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(&bridge.endpoint)
            .header("x-ai-mate-control-token", "private-fixture")
            .json(&VoiceDialogueRequest::new("请返回语音决策 JSON"))
            .send()
            .await
            .unwrap()
            .json::<VoiceDialogueResponse>()
            .await
            .unwrap();
        assert_eq!(reply.content, expected);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        streaming.store(true,Ordering::SeqCst);
        let mut response=tokio::time::timeout(std::time::Duration::from_secs(2),reqwest::Client::builder().no_proxy().build().unwrap()
            .post(&bridge.endpoint).header("x-ai-mate-control-token","private-fixture")
            .json(&serde_json::json!({"prompt":"请返回语音决策 JSON","stream":true})).send()).await
            .expect("stream headers must not wait for the whole model answer").unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        assert!(response.headers()["content-type"].to_str().unwrap().contains("ndjson"));
        let mut first=Vec::new();
        while !first.contains(&b'\n') {first.extend(response.chunk().await.unwrap().unwrap());}
        let sentence:serde_json::Value=serde_json::from_slice(&first).unwrap();
        assert_eq!(sentence["type"],"sentence");assert_eq!(sentence["sentence"]["text"],"测试回答");
        release.notify_one();
        let remaining=response.text().await.unwrap();
        let terminal:serde_json::Value=serde_json::from_str(remaining.lines().last().unwrap()).unwrap();
        assert_eq!(terminal["type"],"complete");
        let decision:serde_json::Value=serde_json::from_str(terminal["response"]["content"].as_str().unwrap()).unwrap();
        assert_eq!(decision["reply_tts"],"测试回答继续回答。");
        assert_eq!(calls.load(Ordering::SeqCst),2);
        // Catalog order must not override the voice-specific preference.
        for model in ["AIPC-deepseek-v4.1-flash", "AIPC-qwen3.8-flash"] {
            model_repo
                .create(
                    &resolved.provider_id,
                    &nomifun_db::NewProviderModel {
                        model,
                        enabled: true,
                        sort_order: 10,
                        tasks: r#"["chat"]"#,
                        traits: "[]",
                        params: "{}",
                        source: "user",
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(
            live.resolve_model(None).await.unwrap().model,
            "AIPC-qwen3.8-flash"
        );
        assert_eq!(live.resolve_model(Some(&resolved)).await.unwrap(), resolved);
        let qwen = VoiceModelRef {
            provider_id: resolved.provider_id.clone(),
            model: "AIPC-qwen3.8-flash".into(),
        };
        for (model, next) in [
            ("AIPC-qwen3.8-flash", "AIPC-deepseek-v4.1-flash"),
            ("AIPC-deepseek-v4.1-flash", "AIPC-fixture-chat"),
        ] {
            model_repo
                .update(
                    &resolved.provider_id,
                    model,
                    &nomifun_db::ProviderModelUpdate {
                        enabled: Some(false),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(live.resolve_model(None).await.unwrap().model, next);
        }
        assert_eq!(
            live.resolve_model(Some(&qwen)).await.unwrap_err(),
            "model_unavailable"
        );
        model_repo
            .update(
                &resolved.provider_id,
                &resolved.model,
                &nomifun_db::ProviderModelUpdate {
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            live.resolve_model(Some(&resolved)).await.unwrap_err(),
            "model_unavailable"
        );
        server.abort();
    }

    struct SlowCompleter;
    #[async_trait]
    impl VoiceDialogueCompleter for SlowCompleter {
        async fn resolve_model(
            &self,
            choice: Option<&VoiceModelRef>,
        ) -> Result<VoiceModelRef, String> {
            choice.cloned().ok_or_else(|| "model_unavailable".into())
        }
        async fn complete(
            &self,
            _prompt: &str,
            _choice: Option<&VoiceModelRef>,
        ) -> Result<String, String> {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            Ok("done".into())
        }
    }

    #[tokio::test]
    async fn fast_voice_dialogue_times_out_and_reports_model_timeout() {
        assert_eq!(
            DEFAULT_VOICE_DIALOGUE_TIMEOUT,
            std::time::Duration::from_secs(8)
        );
        let completer = Arc::new(SlowCompleter);
        let selection = Arc::new(RwLock::new(Some(VoiceModelRef {
            provider_id: "test-prov".into(),
            model: "test-model".into(),
        })));
        let state = DialogueState {
            completer,
            selection,
            credential: "test-token".into(),
            timeout: std::time::Duration::from_millis(30),
            recorder: None,
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "test-token".parse().unwrap());
        let request = VoiceDialogueRequest::new("hello");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::GATEWAY_TIMEOUT);
        let bytes = axum::body::to_bytes(body, 1024).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["code"], "model_timeout");
    }

    struct MockRecorder {
        preflight_outcome: Result<VoiceDialoguePreflight, VoiceDialogueRecorderError>,
        recorded_pairs: Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
        record_error: Option<VoiceDialogueRecorderError>,
    }

    #[async_trait]
    impl VoiceDialogueRecorder for MockRecorder {
        async fn preflight(
            &self,
            _request: &VoiceDialogueRequest,
        ) -> Result<VoiceDialoguePreflight, VoiceDialogueRecorderError> {
            self.preflight_outcome.clone()
        }

        async fn record_pair(
            &self,
            conversation_id: &str,
            request: &VoiceDialogueRequest,
            assistant_content: &str,
        ) -> Result<(), VoiceDialogueRecorderError> {
            if let Some(err) = &self.record_error {
                return Err(err.clone());
            }
            self.recorded_pairs.lock().unwrap().push((
                conversation_id.to_string(),
                request.prompt.clone(),
                assistant_content.to_string(),
            ));
            Ok(())
        }
    }

    #[tokio::test]
    async fn bv_05_preflight_storage_failure_aborts_without_model_call() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let recorder = Arc::new(MockRecorder {
            preflight_outcome: Err(VoiceDialogueRecorderError::WriteFailed("disk_err".into())),
            recorded_pairs: Arc::new(std::sync::Mutex::new(Vec::new())),
            record_error: None,
        });
        let state = DialogueState {
            completer: completer.clone(),
            selection: Arc::new(RwLock::new(Some(VoiceModelRef {
                provider_id: "prov".into(),
                model: "mod".into(),
            }))),
            credential: "token".into(),
            timeout: std::time::Duration::from_secs(1),
            recorder: Some(recorder),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "token".parse().unwrap());
        let request = VoiceDialogueRequest::new("test prompt");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["code"], "voice_record_write_failed");
        assert_eq!(completer.0.load(Ordering::SeqCst), 0, "must make 0 model calls on write failure");
    }

    #[tokio::test]
    async fn bv_06_idempotent_replay_returns_cached_output_without_model_call() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let recorder = Arc::new(MockRecorder {
            preflight_outcome: Ok(VoiceDialoguePreflight::Cached {
                content: "cached-assistant-reply".into(),
            }),
            recorded_pairs: Arc::new(std::sync::Mutex::new(Vec::new())),
            record_error: None,
        });
        let state = DialogueState {
            completer: completer.clone(),
            selection: Arc::new(RwLock::new(Some(VoiceModelRef {
                provider_id: "prov".into(),
                model: "mod".into(),
            }))),
            credential: "token".into(),
            timeout: std::time::Duration::from_secs(1),
            recorder: Some(recorder),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "token".parse().unwrap());
        let request = VoiceDialogueRequest::new("test prompt");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let reply: VoiceDialogueResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reply.content, "cached-assistant-reply");
        assert_eq!(completer.0.load(Ordering::SeqCst), 0, "must make 0 model calls on cached replay");
    }

    #[tokio::test]
    async fn bv_07_parameter_conflict_returns_409_without_model_call() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let recorder = Arc::new(MockRecorder {
            preflight_outcome: Err(VoiceDialogueRecorderError::Conflict),
            recorded_pairs: Arc::new(std::sync::Mutex::new(Vec::new())),
            record_error: None,
        });
        let state = DialogueState {
            completer: completer.clone(),
            selection: Arc::new(RwLock::new(Some(VoiceModelRef {
                provider_id: "prov".into(),
                model: "mod".into(),
            }))),
            credential: "token".into(),
            timeout: std::time::Duration::from_secs(1),
            recorder: Some(recorder),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "token".parse().unwrap());
        let request = VoiceDialogueRequest::new("test prompt");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["code"], "voice_record_conflict");
        assert_eq!(completer.0.load(Ordering::SeqCst), 0, "must make 0 model calls on conflict");
    }

    #[tokio::test]
    async fn bv_02_binding_invalid_returns_400_without_model_call() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let recorder = Arc::new(MockRecorder {
            preflight_outcome: Err(VoiceDialogueRecorderError::BindingInvalid("companion_not_found".into())),
            recorded_pairs: Arc::new(std::sync::Mutex::new(Vec::new())),
            record_error: None,
        });
        let state = DialogueState {
            completer: completer.clone(),
            selection: Arc::new(RwLock::new(Some(VoiceModelRef {
                provider_id: "prov".into(),
                model: "mod".into(),
            }))),
            credential: "token".into(),
            timeout: std::time::Duration::from_secs(1),
            recorder: Some(recorder),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "token".parse().unwrap());
        let request = VoiceDialogueRequest::new("test prompt");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["code"], "binding_invalid");
        assert_eq!(payload["reason"], "companion_not_found");
        assert_eq!(completer.0.load(Ordering::SeqCst), 0, "must make 0 model calls on invalid binding");
    }

    #[tokio::test]
    async fn bv_03_binding_busy_returns_409_without_model_call() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let recorder = Arc::new(MockRecorder {
            preflight_outcome: Err(VoiceDialogueRecorderError::BindingBusy("target_conversation_busy".into())),
            recorded_pairs: Arc::new(std::sync::Mutex::new(Vec::new())),
            record_error: None,
        });
        let state = DialogueState {
            completer: completer.clone(),
            selection: Arc::new(RwLock::new(Some(VoiceModelRef {
                provider_id: "prov".into(),
                model: "mod".into(),
            }))),
            credential: "token".into(),
            timeout: std::time::Duration::from_secs(1),
            recorder: Some(recorder),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "token".parse().unwrap());
        let request = VoiceDialogueRequest::new("test prompt");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["code"], "binding_busy");
        assert_eq!(payload["reason"], "target_conversation_busy");
        assert_eq!(completer.0.load(Ordering::SeqCst), 0, "must make 0 model calls on busy binding");
    }

    #[tokio::test]
    async fn bv_04_target_became_busy_preserves_generated_content_with_projection_warning() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let recorder = Arc::new(MockRecorder {
            preflight_outcome: Ok(VoiceDialoguePreflight::Ready {
                conversation_id: "conv-frozen-1".into(),
            }),
            recorded_pairs: Arc::new(std::sync::Mutex::new(Vec::new())),
            record_error: Some(VoiceDialogueRecorderError::BindingBusy("target_became_busy_during_generation".into())),
        });
        let state = DialogueState {
            completer: completer.clone(),
            selection: Arc::new(RwLock::new(Some(VoiceModelRef {
                provider_id: "prov".into(),
                model: "mod".into(),
            }))),
            credential: "token".into(),
            timeout: std::time::Duration::from_secs(1),
            recorder: Some(recorder),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "token".parse().unwrap());
        let request = VoiceDialogueRequest::new("test prompt");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["record_error"], "binding_busy");
        assert!(!payload["content"].as_str().unwrap().is_empty(),"already generated output must remain visible");
        assert_eq!(completer.0.load(Ordering::SeqCst), 1, "model was called once, but post-generation write detected busy");
    }

    #[tokio::test]
    async fn normal_turn_records_pair_atomically_after_generation() {
        let completer = Arc::new(FixtureCompleter(AtomicUsize::new(0)));
        let pairs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = Arc::new(MockRecorder {
            preflight_outcome: Ok(VoiceDialoguePreflight::Ready {
                conversation_id: "voice-conv-1".into(),
            }),
            recorded_pairs: pairs.clone(),
            record_error: None,
        });
        let state = DialogueState {
            completer: completer.clone(),
            selection: Arc::new(RwLock::new(Some(VoiceModelRef {
                provider_id: "prov".into(),
                model: "mod".into(),
            }))),
            credential: "token".into(),
            timeout: std::time::Duration::from_secs(1),
            recorder: Some(recorder),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-mate-control-token", "token".parse().unwrap());
        let request = VoiceDialogueRequest::new("hello assistant");

        let response = complete(State(state), headers, Json(request)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let reply: VoiceDialogueResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reply.content, "mod:hello assistant");
        assert_eq!(completer.0.load(Ordering::SeqCst), 1);

        let saved = pairs.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0, "voice-conv-1");
        assert_eq!(saved[0].1, "hello assistant");
        assert_eq!(saved[0].2, "mod:hello assistant");
    }
}

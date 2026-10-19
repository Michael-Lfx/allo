//! Integration test for Standalone Voice Conversation Pair Persistence (S1, BV-01, BV-05, BV-06, BV-07).
//!
//! Verifies:
//! - Without any companion/pet, preflight automatically ensures/creates the default "语音助手" Conversation.
//! - Normal dialogue turn records user prompt (if present) and assistant reply atomically into conversation history.
//! - Durable delivery receipt is persisted in `conversation_delivery_receipts` with status "completed" and result_ok = true.
//! - Subsequent replay with identical `voice_record_id` and parameters returns cached assistant response with 0 model calls.
//! - Subsequent replay with same `voice_record_id` but altered parameters returns Conflict with 0 model calls.

use std::sync::Arc;
use async_trait::async_trait;
use nomifun_ai_agent::runtime_handle::AgentRuntimeHandle;
use nomifun_ai_agent::types::AgentRuntimeBuildOptions;
use nomifun_ai_agent::AgentRuntimeRegistry;
use nomifun_api_types::CreateConversationRequest;
use nomifun_api_types::voice::VoiceDialogueRequest;
use nomifun_app::voice_companion_manager::ConversationVoiceDialogueRecorder;
use nomifun_app::voice_dialogue::{
    VoiceDialoguePreflight, VoiceDialogueRecorder, VoiceDialogueRecorderError,
};
use nomifun_common::{AgentKillReason, AppError, TimestampMs};
use nomifun_conversation::ConversationService;
use nomifun_conversation::skill_resolver::{ResolvedAgentSkill, SkillResolver};
use nomifun_db::{
    IConversationRepository, SortOrder, SqliteAcpSessionRepository,
    SqliteAgentMetadataRepository, SqliteConversationRepository,
};
use nomifun_realtime::UserEventSink;

struct NullSink;

impl UserEventSink for NullSink {
    fn send_to_user(&self, _user_id: &str, _event: nomifun_api_types::WebSocketMessage<serde_json::Value>) {}
}

struct NoopSkillResolver;

#[async_trait]
impl SkillResolver for NoopSkillResolver {
    async fn auto_inject_names(&self) -> Vec<String> {
        Vec::new()
    }
    async fn resolve_skills(&self, _names: &[String]) -> Vec<ResolvedAgentSkill> {
        Vec::new()
    }
    async fn link_workspace_skills(
        &self,
        _workspace: &std::path::Path,
        _rel_dirs: &[&str],
        _skills: &[ResolvedAgentSkill],
    ) -> usize {
        0
    }
}

struct EmptyRegistry;

#[async_trait]
impl AgentRuntimeRegistry for EmptyRegistry {
    fn get_runtime(&self, _conversation_id: &str) -> Option<AgentRuntimeHandle> {
        None
    }
    async fn get_or_create_runtime(
        &self,
        _conversation_id: &str,
        _options: AgentRuntimeBuildOptions,
    ) -> Result<AgentRuntimeHandle, AppError> {
        Err(AppError::Internal("test mock has no runtimes".into()))
    }
    fn terminate(&self, _conversation_id: &str, _reason: Option<AgentKillReason>) -> Result<(), AppError> {
        Ok(())
    }
    fn terminate_all(&self) {}
    fn active_runtime_count(&self) -> usize {
        0
    }
    fn collect_idle_runtimes(&self, _idle_threshold_ms: TimestampMs) -> Vec<String> {
        Vec::new()
    }
}

const USER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000001";

#[tokio::test]
async fn standalone_voice_conversation_pair_persistence_lifecycle() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).expect("canonical fixture owner"),
    )
    .await
    .unwrap();
    let conv_repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let user_id = USER_ID;

    let conversation_svc = Arc::new(ConversationService::new(
        Arc::<str>::from(user_id),
        std::env::temp_dir(),
        Arc::new(NullSink),
        Arc::new(NoopSkillResolver),
        Arc::new(EmptyRegistry),
        conv_repo.clone(),
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));

    let journal_dir = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(
        conversation_svc.clone(),
        user_id.to_string(),
        journal_dir.path(),
    );

    let record_id = "voice-turn-s1";
    let mut request = VoiceDialogueRequest::scoped("session-s1", "turn-s1", "今天天气怎么样？");
    request.voice_record_id = Some(record_id.to_string());
    request.raw_user_text = Some("今天天气怎么样？".to_string());

    // 1. First preflight: no conversation exists yet, ensures default "语音助手" conversation
    let preflight_result = recorder.preflight(&request).await.expect("preflight must succeed");
    let conv_id = match preflight_result {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!("first turn must not be cached"),
    };

    // Verify conversation created in DB with title "语音助手" and extra.is_default_voice = true
    let conv_row = conv_repo.get(&conv_id).await.unwrap().expect("conversation must exist");
    assert_eq!(conv_row.name, "语音助手");
    let extra: serde_json::Value = serde_json::from_str(&conv_row.extra).unwrap();
    assert_eq!(extra["is_default_voice"], true);

    // 2. Record pair: user message + assistant message + delivery receipt
    let assistant_reply = "今天北京晴天，温度 22 度，适宜出行。";
    let assistant_output=serde_json::json!({"intent_type":"chat","reply_tts":assistant_reply}).to_string();
    recorder
        .record_pair(&conv_id, &request, &assistant_output)
        .await
        .expect("pair recording must succeed");

    // Verify messages in DB
    let messages = conv_repo
        .get_messages(&conv_id, 1, 10, SortOrder::Asc)
        .await
        .unwrap();
    assert_eq!(messages.items.len(), 2, "pair must contain user and assistant messages");

    // User message
    assert_eq!(messages.items[0].position.as_deref(), Some("right"));
    let user_content: serde_json::Value = serde_json::from_str(&messages.items[0].content).unwrap();
    assert_eq!(user_content["content"], "今天天气怎么样？");

    // Assistant message
    assert_eq!(messages.items[1].position.as_deref(), Some("left"));
    let asst_content: serde_json::Value = serde_json::from_str(&messages.items[1].content).unwrap();
    assert_eq!(asst_content["content"], assistant_reply);

    // Verify delivery receipt in DB
    let receipt = conv_repo
        .get_delivery_receipt(user_id, &conv_id, record_id)
        .await
        .unwrap()
        .expect("receipt must exist");
    assert_eq!(receipt.status, "completed");
    assert_eq!(receipt.result_ok, Some(true));

    // 3. BV-06: Same record_id replay with identical payload returns cached output
    let replay_preflight = recorder.preflight(&request).await.expect("replay preflight must succeed");
    match replay_preflight {
        VoiceDialoguePreflight::Cached { content } => {
            assert_eq!(content, assistant_output, "cached content must preserve the original decision");
        }
        _ => panic!("replay with same record_id must return Cached"),
    }

    // 4. BV-07: Same record_id replay with different payload returns Conflict
    let mut altered_request = VoiceDialogueRequest::scoped("session-s1", "turn-s1", "今天天气怎么样？");
    altered_request.voice_record_id = Some(record_id.to_string());
    altered_request.raw_user_text = Some("明天天气怎么样？".to_string()); // altered!

    let conflict_err = recorder.preflight(&altered_request).await.expect_err("altered request must fail with conflict");
    assert_eq!(conflict_err, VoiceDialogueRecorderError::Conflict);

    // 5. Subsequent dialogue turn in same conversation reuses default conversation
    let second_request = VoiceDialogueRequest::scoped("session-s1","turn-s2","明天呢？");

    let second_preflight = recorder.preflight(&second_request).await.expect("second preflight must succeed");
    let second_conv_id = match second_preflight {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!("new turn must be Ready"),
    };
    assert_eq!(conv_id, second_conv_id, "default conversation must be reused");
}

#[tokio::test]
async fn bv_02_explicit_binding_resolution_and_validation() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).expect("canonical fixture owner"),
    )
    .await
    .unwrap();
    let conv_repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let user_id = USER_ID;

    let conversation_svc = Arc::new(ConversationService::new(
        Arc::<str>::from(user_id),
        std::env::temp_dir(),
        Arc::new(NullSink),
        Arc::new(NoopSkillResolver),
        Arc::new(EmptyRegistry),
        conv_repo.clone(),
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));

    let journal_dir = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(
        conversation_svc.clone(),
        user_id.to_string(),
        journal_dir.path(),
    );

    // 1. Explicit binding to invalid conversation id returns BindingInvalid
    let mut req_invalid = VoiceDialogueRequest::new("你好");
    req_invalid.binding_id = Some("non-existent-conv-id".to_string());
    let err = recorder.preflight(&req_invalid).await.unwrap_err();
    assert!(matches!(err, VoiceDialogueRecorderError::BindingInvalid(_)));

    // 2. Explicit binding to valid conversation resolves to that conversation
    let req: CreateConversationRequest = serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "项目对话",
        "extra": {},
    })).unwrap();
    let existing = conversation_svc.create(user_id, req).await.unwrap();
    let existing_conv_id = existing.conversation_id;

    let req_valid = VoiceDialogueRequest::scoped("valid-session", "valid-turn", "你好");
    *recorder.active_window_candidate.write().unwrap()=Some(existing_conv_id.clone());
    let preflight = recorder.preflight(&req_valid).await.unwrap();
    match preflight {
        VoiceDialoguePreflight::Ready { conversation_id } => {
            assert_eq!(conversation_id, existing_conv_id);
        }
        _ => panic!("expected Ready"),
    }
}

#[tokio::test]
async fn bv_03_busy_active_candidate_degrades_gracefully_to_default_voice_conversation() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).expect("canonical fixture owner"),
    )
    .await
    .unwrap();
    let conv_repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let user_id = USER_ID;

    let conversation_svc = Arc::new(ConversationService::new(
        Arc::<str>::from(user_id),
        std::env::temp_dir(),
        Arc::new(NullSink),
        Arc::new(NoopSkillResolver),
        Arc::new(EmptyRegistry),
        conv_repo.clone(),
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));

    let req: CreateConversationRequest = serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "工作对话",
        "extra": {},
    })).unwrap();
    let candidate = conversation_svc.create(user_id, req).await.unwrap();
    let candidate_conv_id = candidate.conversation_id;

    let candidate = std::sync::Arc::new(std::sync::RwLock::new(Some(candidate_conv_id.clone())));
    let journal_dir = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(
        conversation_svc.clone(),
        user_id.to_string(),
        journal_dir.path(),
    )
    .with_active_window_candidate(candidate.clone());

    // When candidate is idle: preflight resolves to candidate
    let req = VoiceDialogueRequest::scoped("session-idle", "turn-idle", "你好");
    let preflight = recorder.preflight(&req).await.unwrap();
    assert_eq!(
        preflight,
        VoiceDialoguePreflight::Ready {
            conversation_id: candidate_conv_id.clone()
        }
    );

    // Now mark candidate conversation as running/busy
    sqlx::query("UPDATE conversations SET active_turn_operation_id = 'turn-test-busy' WHERE conversation_id = ?")
        .bind(&candidate_conv_id)
        .execute(db.pool())
        .await
        .unwrap();

    // BV-03: When active candidate is busy, degrade gracefully to default "语音助手" conversation
    let req_busy = VoiceDialogueRequest::scoped("session-busy", "turn-busy", "你好");
    let preflight_busy = recorder.preflight(&req_busy).await.unwrap();
    match preflight_busy {
        VoiceDialoguePreflight::Ready { conversation_id } => {
            assert_ne!(conversation_id, candidate_conv_id);
            let row = conv_repo.get(&conversation_id).await.unwrap().unwrap();
            assert_eq!(row.name, "语音助手");
        }
        _ => panic!("expected Ready"),
    }
}

#[tokio::test]
async fn bv_04_target_becoming_busy_during_generation_rejects_record_pair() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).expect("canonical fixture owner"),
    )
    .await
    .unwrap();
    let conv_repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let user_id = USER_ID;

    let conversation_svc = Arc::new(ConversationService::new(
        Arc::<str>::from(user_id),
        std::env::temp_dir(),
        Arc::new(NullSink),
        Arc::new(NoopSkillResolver),
        Arc::new(EmptyRegistry),
        conv_repo.clone(),
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));

    let req: CreateConversationRequest = serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "目标对话",
        "extra": {},
    })).unwrap();
    let target = conversation_svc.create(user_id, req).await.unwrap();
    let target_conv_id = target.conversation_id;

    let journal_dir = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(
        conversation_svc.clone(),
        user_id.to_string(),
        journal_dir.path(),
    );

    let req = VoiceDialogueRequest::scoped("session-busy", "turn-busy", "处理中");
    *recorder.active_window_candidate.write().unwrap()=Some(target_conv_id.clone());

    // Preflight when idle succeeds
    let preflight = recorder.preflight(&req).await.unwrap();
    assert_eq!(
        preflight,
        VoiceDialoguePreflight::Ready {
            conversation_id: target_conv_id.clone()
        }
    );

    // Target becomes busy while model was generating (e.g. user submitted message in UI)
    sqlx::query("UPDATE conversations SET active_turn_operation_id = 'turn-test-busy' WHERE conversation_id = ?")
        .bind(&target_conv_id)
        .execute(db.pool())
        .await
        .unwrap();

    // BV-04: record_pair detects target became busy post-generation, rejects with BindingBusy
    let err = recorder
        .record_pair(&target_conv_id, &req, r#"{"intent_type":"chat","reply_tts":"助手回复"}"#)
        .await
        .unwrap_err();
    assert!(matches!(err, VoiceDialogueRecorderError::BindingBusy(_)));
}


async fn fixture_service() -> Arc<ConversationService> {
    let db=nomifun_db::init_database_memory_with_owner(nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap()).await.unwrap();
    Arc::new(ConversationService::new(Arc::<str>::from(USER_ID),std::env::temp_dir(),Arc::new(NullSink),Arc::new(NoopSkillResolver),Arc::new(EmptyRegistry),
        Arc::new(SqliteConversationRepository::new(db.pool().clone())),Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),Arc::new(nomifun_conversation::NoExecutionConversationBoundary)))
}

struct GatedCompleter {
    calls:std::sync::atomic::AtomicUsize,
    entered:tokio::sync::Notify,
    release:tokio::sync::Notify,
    journal:std::path::PathBuf,
}
#[async_trait]
impl nomifun_app::voice_dialogue::VoiceDialogueCompleter for GatedCompleter {
    async fn resolve_model(&self,_choice:Option<&nomifun_api_types::voice::VoiceModelRef>)->Result<nomifun_api_types::voice::VoiceModelRef,String> {Err("unused".into())}
    async fn complete(&self,_prompt:&str,_choice:Option<&nomifun_api_types::voice::VoiceModelRef>)->Result<String,String> {
        // Durability is part of the model-call boundary, not a preflight mock expectation.
        let records:serde_json::Value=serde_json::from_slice(&std::fs::read(&self.journal).unwrap()).unwrap();
        assert!(records.as_array().unwrap().iter().any(|r|r["request"]["raw_user_text"]=="你好，请介绍你自己" && r["output"].is_null()));
        self.calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        Ok(format!("```json\n{}\n```",serde_json::json!({"intent_type":"chat","understood_request":"用户想了解助手。","reply_tts":"你好，我是你的语音助手。"})))
    }
}

#[tokio::test]
async fn production_router_records_raw_input_before_one_generation_and_replays_the_raw_decision() {
    use ai_mate::replay_terminal::{driver::VoiceRouter,router::OpenAiCompatibleVoiceRouter,completion::HostDialogueClient};
    use nomifun_app::voice_dialogue::VoiceDialogueBridge;
    let dir=tempfile::tempdir().unwrap();let service=fixture_service().await;
    let recorder=Arc::new(ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path()));
    let completer=Arc::new(GatedCompleter {calls:Default::default(),entered:Default::default(),release:Default::default(),journal:dir.path().join("voice").join(format!("dialogue-{USER_ID}.json"))});
    let bridge=VoiceDialogueBridge::start_with_recorder(completer.clone(),Default::default(),"scoped-control".into(),axum::Router::new(),Some(recorder.clone())).await.unwrap();
    let router=Arc::new(OpenAiCompatibleVoiceRouter::with_completion(Arc::new(HostDialogueClient::new(bridge.endpoint.clone(),"scoped-control".into()).unwrap())));
    let scope=VoiceDialogueRequest::scoped("joint-session","joint-turn","你好，请介绍你自己");
    let r=router.clone();let req=scope.clone();
    let first=tokio::spawn(async move {r.route_scoped(&req,"你好，请介绍你自己",None,&[]).await});
    tokio::time::timeout(std::time::Duration::from_secs(3),completer.entered.notified()).await.unwrap();
    let second=router.route_scoped(&scope,"你好，请介绍你自己",None,&[]).await;
    assert!(second.0.is_err(),"a concurrent request must not issue a second generation");
    completer.release.notify_one();
    let first=first.await.unwrap();assert!(first.0.is_ok());
    let replay=router.route_scoped(&scope,"你好，请介绍你自己",None,&[]).await;
    assert_eq!(replay.0.unwrap(),first.0.unwrap());assert_eq!(replay.1,first.1);
    assert_eq!(completer.calls.load(std::sync::atomic::Ordering::SeqCst),1);
    let binding=recorder.freeze_session("joint-session").await.unwrap().unwrap();
    let messages=service.list_messages(USER_ID,&binding.conversation_id,Default::default()).await.unwrap();
    assert_eq!(messages.items.len(),2);
    assert!(messages.items.iter().any(|m|m.content["content"]=="你好，请介绍你自己"));
    assert!(messages.items.iter().any(|m|m.content["content"].as_str().is_some_and(|t|t.contains("你好，我是你的语音助手。") && !t.contains("intent_type"))));
    drop(bridge);
    let restored=ConversationVoiceDialogueRecorder::new(service,USER_ID.into(),dir.path());
    let records:serde_json::Value=serde_json::from_slice(&std::fs::read(&completer.journal).unwrap()).unwrap();
    let persisted:VoiceDialogueRequest=serde_json::from_value(records.as_array().unwrap().iter().find(|r|r["request"]["voice_record_id"]=="voice-joint-turn").unwrap()["request"].clone()).unwrap();
    assert!(matches!(restored.preflight(&persisted).await.unwrap(),VoiceDialoguePreflight::Cached{..}));
}

#[tokio::test]
async fn clearing_a_target_cannot_resurrect_a_generated_turn_or_force_regeneration() {
    let dir=tempfile::tempdir().unwrap();let service=fixture_service().await;
    let recorder=ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path());
    let request=VoiceDialogueRequest::scoped("clear-session","clear-turn","请解释这句话");
    let conv=match recorder.preflight(&request).await.unwrap(){VoiceDialoguePreflight::Ready{conversation_id}=>conversation_id,_=>panic!("new turn")};
    service.conversation_repo().clear_terminal_conversation_messages(USER_ID,&conv,nomifun_common::now_ms()).await.unwrap();
    let output=r#"{"intent_type":"chat","understood_request":"解释这句话。","reply_tts":"这是已生成的回答。"}"#;
    assert!(matches!(recorder.record_pair(&conv,&request,output).await,Err(VoiceDialogueRecorderError::BindingBusy(_))));
    let restored=ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path());
    assert_eq!(restored.preflight(&request).await.unwrap(),VoiceDialoguePreflight::Generated{conversation_id:conv.clone(),content:output.into()});
    assert!(restored.record_pair(&conv,&request,output).await.is_err());
    assert_eq!(service.list_messages(USER_ID,&conv,Default::default()).await.unwrap().items.len(),0);
    let next=VoiceDialogueRequest::scoped("clear-session","new-turn","新的话");
    assert!(matches!(restored.preflight(&next).await,Err(VoiceDialogueRecorderError::BindingInvalid(_))));
}

#[tokio::test]
async fn a_claim_survives_restart_without_an_automatic_second_generation() {
    let dir=tempfile::tempdir().unwrap();let service=fixture_service().await;
    let request=VoiceDialogueRequest::scoped("lost-session","lost-turn","你好");
    let recorder=ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path());
    recorder.preflight(&request).await.unwrap();drop(recorder);
    let restored=ConversationVoiceDialogueRecorder::new(service,USER_ID.into(),dir.path());
    assert_eq!(restored.preflight(&request).await.unwrap_err(),VoiceDialogueRecorderError::GenerationUnknown);
    let mut changed=request;changed.raw_user_text=Some("更改目标".into());
    assert_eq!(restored.preflight(&changed).await.unwrap_err(),VoiceDialogueRecorderError::Conflict);
}


#[tokio::test]
async fn a_wake_binding_freezes_the_project_and_its_execution_workspace_across_navigation() {
    use nomifun_api_types::voice::VoiceTaskCommand;
    let dir=tempfile::tempdir().unwrap();let workspace_a=tempfile::tempdir().unwrap();let workspace_b=tempfile::tempdir().unwrap();let service=fixture_service().await;
    let create=|workspace:&std::path::Path|serde_json::from_value::<CreateConversationRequest>(serde_json::json!({"type":"nomi","name":"本地项目","extra":{"workspace":workspace.to_string_lossy()}})).unwrap();
    let a=service.create(USER_ID,create(workspace_a.path())).await.unwrap();let b=service.create(USER_ID,create(workspace_b.path())).await.unwrap();
    let candidate=Arc::new(std::sync::RwLock::new(Some(a.conversation_id.clone())));
    let recorder=ConversationVoiceDialogueRecorder::new(service,USER_ID.into(),dir.path()).with_active_window_candidate(candidate.clone());
    let frozen=recorder.freeze_session("frozen-session").await.unwrap().unwrap();assert_eq!(frozen.conversation_id,a.conversation_id);
    *candidate.write().unwrap()=Some(b.conversation_id.clone());
    let request=VoiceDialogueRequest::scoped("frozen-session","frozen-turn","读取这个项目");
    assert_eq!(recorder.preflight(&request).await.unwrap(),VoiceDialoguePreflight::Ready{conversation_id:a.conversation_id.clone()});
    recorder.record_pair(&a.conversation_id,&request,r#"{"intent_type":"agent_task","understood_request":"读取当前项目。","task_goal":"读取项目A","spoken_ack":"未接受前不能播出"}"#).await.unwrap();
    let command=VoiceTaskCommand{command_id:"voice-frozen-turn".into(),session_id:"frozen-session".into(),turn_id:"frozen-turn".into(),goal:"读取项目A".into(),record_id:request.voice_record_id.clone(),binding_id:request.binding_id.clone()};
    let config=recorder.task_configuration(&command).await.unwrap().unwrap();
    assert_eq!(config.config.workspace,nomifun_common::paths::canonicalize_simplified(workspace_a.path()).unwrap().to_string_lossy());
    let implicit_command=VoiceTaskCommand{command_id:"voice-frozen-turn".into(),session_id:"frozen-session".into(),turn_id:"frozen-turn".into(),goal:"读取项目A".into(),record_id:None,binding_id:None};
    let implicit_config=recorder.task_configuration(&implicit_command).await.unwrap().unwrap();
    assert_eq!(implicit_config.config.workspace,config.config.workspace);
    let mut new_identity=command.clone();new_identity.command_id="different-delivery-for-same-record".into();
    assert_eq!(recorder.task_configuration(&new_identity).await.err().unwrap(),"task_command_conflict","one voice decision cannot create another delivery identity");
    let mut changed=command;changed.goal="读取项目B".into();assert_eq!(recorder.task_configuration(&changed).await.err().unwrap(),"task_command_conflict");
}

#[tokio::test]
async fn a_post_generation_projection_failure_returns_text_and_retry_never_calls_the_model_again() {
    use ai_mate::replay_terminal::{driver::{VoiceRouter,RouteError},router::OpenAiCompatibleVoiceRouter,completion::HostDialogueClient};
    use nomifun_app::voice_dialogue::VoiceDialogueBridge;
    let dir=tempfile::tempdir().unwrap();let service=fixture_service().await;
    let recorder=Arc::new(ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path()));
    let completer=Arc::new(GatedCompleter{calls:Default::default(),entered:Default::default(),release:Default::default(),journal:dir.path().join("voice").join(format!("dialogue-{USER_ID}.json"))});
    let bridge=VoiceDialogueBridge::start_with_recorder(completer.clone(),Default::default(),"projection-control".into(),axum::Router::new(),Some(recorder.clone())).await.unwrap();
    let router=Arc::new(OpenAiCompatibleVoiceRouter::with_completion(Arc::new(HostDialogueClient::new(bridge.endpoint.clone(),"projection-control".into()).unwrap())));
    let scope=VoiceDialogueRequest::scoped("projection-session","projection-turn","你好，请介绍你自己");
    let r=router.clone();let req=scope.clone();let first=tokio::spawn(async move{r.route_scoped(&req,"你好，请介绍你自己",None,&[]).await});
    tokio::time::timeout(std::time::Duration::from_secs(3),completer.entered.notified()).await.unwrap();
    let binding=recorder.freeze_session("projection-session").await.unwrap().unwrap();
    service.conversation_repo().clear_terminal_conversation_messages(USER_ID,&binding.conversation_id,nomifun_common::now_ms()).await.unwrap();
    completer.release.notify_one();let first=first.await.unwrap();
    assert!(matches!(first.0,Err(RouteError::Record{ref code,..}) if code=="binding_busy"));
    assert!(first.1.as_ref().unwrap().contains("你好，我是你的语音助手。"));
    let retry=router.route_scoped(&scope,"你好，请介绍你自己",None,&[]).await;
    assert!(matches!(retry.0,Err(RouteError::Record{ref code,..}) if code=="binding_busy"));assert_eq!(first.1,retry.1);
    assert_eq!(completer.calls.load(std::sync::atomic::Ordering::SeqCst),1);
    assert_eq!(service.list_messages(USER_ID,&binding.conversation_id,Default::default()).await.unwrap().items.len(),0);
}

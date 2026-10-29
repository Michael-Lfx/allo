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
use nomifun_app::voice_companion_manager::{
    ConversationVoiceDialogueRecorder, VoiceCompanionProcessManager, VoiceWindowContext,
};
use nomifun_app::voice_dialogue::{
    VoiceCallReservation, VoiceDialoguePreflight, VoiceDialogueRecorder, VoiceDialogueRecorderError,
};
use nomifun_common::{AgentKillReason, AppError, TimestampMs};
use nomifun_conversation::ConversationService;
use nomifun_conversation::skill_resolver::{ResolvedAgentSkill, SkillResolver};
use nomifun_db::{
    IConversationRepository, SortOrder, SqliteAcpSessionRepository,
    SqliteAgentMetadataRepository, SqliteConversationRepository,
};
use nomifun_realtime::UserEventSink;

#[derive(Default)]
struct ReviewCapturedEvents(std::sync::Mutex<Vec<nomifun_api_types::WebSocketMessage<serde_json::Value>>>);
impl UserEventSink for ReviewCapturedEvents {
    fn send_to_user(&self, _: &str, event: nomifun_api_types::WebSocketMessage<serde_json::Value>) {
        self.0.lock().unwrap().push(event);
    }
}

struct TestTitleCompleter;
#[async_trait::async_trait]
impl nomifun_ai_agent::conversation_title_completer::ConversationTitleCompleter for TestTitleCompleter {
    async fn summarize(
        &self,
        content: &str,
        _candidates: &[nomifun_common::ProviderWithModel],
        _observation_session: Option<std::sync::Arc<nomifun_ai_agent::agent_trace::ObservationSession>>,
    ) -> Result<nomifun_ai_agent::conversation_title_completer::ConversationTitleResult, AppError> {
        let title = if content.is_ascii() {
            "Capital of France"
        } else {
            "北京天气查询"
        };
        Ok(nomifun_ai_agent::conversation_title_completer::ConversationTitleResult {
            title: title.into(),
            llm_call_count: 1,
            response_channel: None,
            response_chars: title.chars().count(),
        })
    }
}
#[tokio::test]
async fn voice_user_created_event_preserves_the_frozen_memory_owner() {
    let db = nomifun_db::init_database_memory_with_owner(nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap()).await.unwrap();
    let repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let captured = Arc::new(ReviewCapturedEvents::default());
    let svc = ConversationService::new(Arc::<str>::from(USER_ID),std::env::temp_dir(),captured.clone(),Arc::new(NoopSkillResolver),Arc::new(EmptyRegistry),repo,Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),Arc::new(nomifun_conversation::NoExecutionConversationBoundary));
    let owner = "0190f5fe-7c00-7a00-8000-000000000003";
    let conversation = svc.create(USER_ID,serde_json::from_value(serde_json::json!({"type":"nomi","name":"voice owner probe","extra":{"voice":true,"memory_owner_companion_id":owner}})).unwrap()).await.unwrap();
    svc.append_voice_message(USER_ID,&conversation.conversation_id,&nomifun_common::MessageId::new().into_string(),"记住这句话只属于原伙伴","review-owner-command").await.unwrap();
    let events=captured.0.lock().unwrap();
    let evt=events.iter().find(|e|e.name=="message.userCreated").unwrap();
    let actual=evt.data.get("memory_owner_companion_id").or_else(||evt.data.get("companion_id")).and_then(|v|v.as_str());
    assert_eq!(actual,Some(owner),"production userCreated loses frozen memory owner: {}",evt.data);
}

#[tokio::test]
async fn voice_pair_publishes_the_same_sides_and_billing_turn_as_reloaded_messages() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap(),
    )
    .await
    .unwrap();
    let captured = Arc::new(ReviewCapturedEvents::default());
    let service = ConversationService::new(
        Arc::<str>::from(USER_ID),
        std::env::temp_dir(),
        captured.clone(),
        Arc::new(NoopSkillResolver),
        Arc::new(EmptyRegistry),
        Arc::new(SqliteConversationRepository::new(db.pool().clone())),
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    );
    let conv = service
        .ensure_default_voice_conversation(USER_ID)
        .await
        .unwrap();
    let epoch = service
        .conversation_repo()
        .get_turn_admission_state(USER_ID, &conv)
        .await
        .unwrap()
        .epoch;
    let user_id = nomifun_common::MessageId::new().into_string();
    for _ in 0..2 {
        service
            .project_voice_dialogue_pair_with_user_msg_id(
                USER_ID,
                &conv,
                "layout-billing-pair",
                Some("今天星期几？"),
                "我来确认日期。",
                "voice_companion",
                epoch,
                Some(&user_id),
            )
            .await
            .unwrap();
    }
    let rows = service
        .list_messages(USER_ID, &conv, Default::default())
        .await
        .unwrap()
        .items;
    assert_eq!(rows.len(), 2);
    let events = captured.0.lock().unwrap();
    let streams: Vec<_> = events
        .iter()
        .filter(|event| event.name == "message.stream")
        .collect();
    assert_eq!(
        streams.len(),
        3,
        "replay rebroadcasts the stable assistant without reinserting user rows"
    );
    assert_eq!(streams[0].data["position"], "right");
    assert_eq!(streams[0].data["msg_id"], user_id);
    for assistant in &streams[1..] {
        assert_eq!(assistant.data["position"], "left");
        assert_eq!(assistant.data["turn_id"], user_id);
        let stored = rows
            .iter()
            .find(|row| row.message_id == assistant.data["msg_id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored.content["turn_id"], assistant.data["turn_id"]);
    }
}

#[tokio::test]
async fn latest_user_correction_survives_a_long_assistant_reply() {
    let dir=tempfile::tempdir().unwrap();
    let recorder=ConversationVoiceDialogueRecorder::new(fixture_service().await,USER_ID.into(),dir.path());
    let first=VoiceDialogueRequest::scoped("review-c08","review-c08-one","不是北京，改深圳，数量改为 2 不是 20");
    let VoiceDialoguePreflight::Ready{conversation_id}=recorder.preflight(&first).await.unwrap() else {panic!("expected Ready")};
    let output=serde_json::json!({"intent_type":"chat","understood_request":"城市为深圳数量2","reply_tts":"这是较长的补充解释。".repeat(200)}).to_string();
    recorder.record_pair(&conversation_id,&first,&output).await.unwrap();
    let next=VoiceDialogueRequest::scoped("review-c08","review-c08-two","按刚才修正的要求创建订单");
    recorder.preflight(&next).await.unwrap();
    recorder.record_pair(&conversation_id,&next,r#"{"intent_type":"agent_task","task_goal":"创建订单","understood_request":"按更正要求创建订单","spoken_ack":"正在处理"}"#).await.unwrap();
    let command=nomifun_api_types::voice::VoiceTaskCommand{command_id:"review-c08-command".into(),session_id:"review-c08".into(),turn_id:"review-c08-two".into(),goal:"创建订单".into(),record_id:next.voice_record_id.clone(),binding_id:next.binding_id.clone()};
    let handoff=recorder.task_configuration(&command).await.unwrap().unwrap().handoff_content.unwrap();
    assert!(handoff.contains("深圳") && handoff.contains("数量改为 2"),"a long assistant reply must not silently erase the latest short correction; handoff={handoff}");
}

struct NullSink;

#[tokio::test]
async fn voice_creation_uses_the_same_authoritative_preset_and_freezes_its_snapshot() {
    use nomifun_db::IProviderRepository;
    let dir = tempfile::tempdir().unwrap();
    let db = nomifun_db::init_database_memory_with_owner(nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap()).await.unwrap();
    let pool = db.pool().clone();
    let providers = Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone()));
    providers.create(nomifun_db::CreateProviderParams {
        provider_id: Some(USER_ID), platform: "openai", name: "preset fixture", base_url: "http://127.0.0.1:1/v1", api_key_encrypted: "unused",
        models: r#"["home-model","preset-model"]"#, enabled: true, model_context_limits: None, model_protocols: None, model_descriptions: None,
        model_enabled: None, bedrock_config: None, is_full_url: false, sort_order: None,
    }).await.unwrap();
    let registry = nomifun_extension::ExtensionRegistry::new(
        nomifun_extension::ExtensionStateStore::new(dir.path().join("extensions.json")),
        Arc::new(nomifun_realtime::BroadcastEventBus::new(16)), "1.6.8".into(),
    );
    let presets = Arc::new(nomifun_preset::PresetService::new(
        Arc::new(nomifun_db::SqlitePresetRepository::new(pool.clone())),
        Arc::new(nomifun_db::SqlitePresetStateRepository::new(pool.clone())),
        Arc::new(nomifun_db::SqlitePresetTagRepository::new(pool.clone())),
        Arc::new(SqliteAgentMetadataRepository::new(pool.clone())), providers,
        Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
        Arc::new(nomifun_preset::BuiltinPresetRegistry::empty()), registry, dir.path().into(),
    ));
    let preset = presets.create(serde_json::from_value(serde_json::json!({
        "name":"voice authority fixture", "instructions":"Original frozen instructions", "targets":["conversation"],
        "model_preferences":[{"provider_id":USER_ID,"model":"preset-model","required":true}]
    })).unwrap()).await.unwrap();
    let service = ConversationService::new(Arc::<str>::from(USER_ID), dir.path().into(), Arc::new(NullSink), Arc::new(NoopSkillResolver), Arc::new(EmptyRegistry),
        Arc::new(SqliteConversationRepository::new(pool.clone())), Arc::new(SqliteAgentMetadataRepository::new(pool.clone())),
        Arc::new(SqliteAcpSessionRepository::new(pool)), Arc::new(nomifun_conversation::NoExecutionConversationBoundary));
    service.with_preset_service(presets.clone());
    let normal = service.create(USER_ID, serde_json::from_value(serde_json::json!({
        "type":"nomi", "extra":{}, "preset_id":preset.preset_id, "model":{"provider_id":USER_ID,"model":"home-model"}
    })).unwrap()).await.unwrap();
    let model = nomifun_api_types::voice::VoiceModelRef {provider_id: USER_ID.into(), model: "home-model".into()};
    let voice = service.create_provisional_voice_conversation(USER_ID, None, None, Some(dir.path().to_str().unwrap()), "resolved-preset", Some(&model), Some(&preset.preset_id)).await.unwrap();
    assert_eq!(voice.model.as_ref().unwrap().model, "preset-model");
    assert_eq!(voice.model, normal.model);
    assert_eq!(serde_json::to_value(&voice.preset_snapshot).unwrap(), serde_json::to_value(&normal.preset_snapshot).unwrap());
    presets.update(&preset.preset_id, serde_json::from_value(serde_json::json!({"instructions":"Changed after admission"})).unwrap()).await.unwrap();
    let replay = service.create_provisional_voice_conversation(USER_ID, None, None, Some(dir.path().to_str().unwrap()), "resolved-preset", Some(&model), Some(&preset.preset_id)).await.unwrap();
    assert_eq!(replay.conversation_id, voice.conversation_id);
    assert_eq!(replay.preset_snapshot.unwrap().instructions, "Original frozen instructions");
}

#[tokio::test]
async fn new_voice_call_freezes_the_home_model_and_rejects_changed_selection() {
    use nomifun_api_types::voice::{VoiceCallTarget, VoiceModelRef};
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service_with_models(&["selected-model", "other-model"]).await;
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());
    let mut target = VoiceCallTarget::NewVoiceConversation {
        workspace: None,
        model: Some(VoiceModelRef {provider_id: USER_ID.into(), model: "selected-model".into()}),
        preset_id: None,
    };
    let (id, _) = recorder.resolve_call_target(&target, Some("yolo"), Some("home-selection")).await.unwrap();
    assert_eq!(service.get(USER_ID, &id).await.unwrap().model.unwrap().model, "selected-model");
    assert_eq!(recorder.resolve_call_target(&target, Some("yolo"), Some("home-selection")).await.unwrap().0, id);
    if let VoiceCallTarget::NewVoiceConversation {model: Some(model), ..} = &mut target {model.model = "other-model".into();}
    assert!(recorder.resolve_call_target(&target, Some("yolo"), Some("home-selection")).await.is_err());
    if let VoiceCallTarget::NewVoiceConversation {preset_id, ..} = &mut target {*preset_id = Some("missing-preset".into());}
    assert!(recorder.resolve_call_target(&target, Some("yolo"), Some("missing-home-preset")).await.is_err(), "a selected preset must be resolved, never silently dropped");
}

#[tokio::test]
async fn voice_placeholder_closes_only_after_terminal_and_view_departure() {
    let dir = tempfile::tempdir().unwrap();
    let database_path = dir.path().join("conversation.sqlite");
    let (database, service) = fixture_file_service(&database_path).await;
    let conversation = service.create_provisional_voice_conversation(USER_ID, None, Some("yolo"), None, "empty-call", None, None).await.unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());
    recorder.set_call_reservation(Some(VoiceCallReservation { request_id: "empty-call".into(), target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {workspace: None, model: None, preset_id: None}, conversation_id: conversation.conversation_id.clone(), memory_owner: None, reserved_at_epoch_ms: nomifun_common::now_ms() }));
    recorder.freeze_session("empty-session", Some("empty-call")).await.unwrap();
    assert!(!recorder.finish_call_placeholder(&conversation.conversation_id, "stale-request").await.unwrap());
    assert!(recorder.finish_call_placeholder(&conversation.conversation_id, "empty-call").await.unwrap());
    recorder.cleanup_ended_placeholders(Some(&conversation.conversation_id), &[]).await.unwrap();
    assert_eq!(service.get(USER_ID, &conversation.conversation_id).await.unwrap().extra["voice_lifecycle"], "ended_empty");
    recorder.cleanup_ended_placeholders(None, &[conversation.conversation_id.clone()]).await.unwrap();
    assert_eq!(service.get(USER_ID, &conversation.conversation_id).await.unwrap().extra["voice_lifecycle"], "ended_empty");
    drop(recorder);
    drop(service);
    database.close().await;
    let (reopened_database, service) = fixture_file_service(&database_path).await;
    let restored = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());
    restored.cleanup_ended_placeholders(None, &[]).await.unwrap();
    assert_eq!(service.get(USER_ID, &conversation.conversation_id).await.unwrap().extra["voice_lifecycle"], "discarded");
    reopened_database.close().await;
}

#[tokio::test]
async fn only_retained_explicit_calls_replace_automatic_workspace_pointers() {
    use nomifun_api_types::voice::VoiceCallTarget;
    for project in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let workspace = project.then(|| dir.path().to_string_lossy().into_owned());
        let auto = workspace.as_ref().map(|workspace| VoiceCallTarget::Workspace {workspace: workspace.clone()}).unwrap_or(VoiceCallTarget::DefaultWorkspace);
        let service = fixture_service().await;
        let recorder = ConversationVoiceDialogueRecorder::new(service, USER_ID.into(), dir.path());
        let old = recorder.resolve_call_target(&auto, None, None).await.unwrap().0;
        let target = VoiceCallTarget::NewVoiceConversation {workspace, model: None, preset_id: None};
        let (new, owner) = recorder.resolve_call_target(&target, None, Some("pointer-call")).await.unwrap();
        assert_ne!(new, old);
        assert_eq!(recorder.resolve_call_target(&auto, None, None).await.unwrap().0, old, "unused call must not publish a pointer");
        recorder.set_call_reservation(Some(VoiceCallReservation {request_id: "pointer-call".into(), target: target.clone(), conversation_id: new.clone(), memory_owner: owner, reserved_at_epoch_ms: nomifun_common::now_ms()}));
        recorder.freeze_session("pointer-session", Some("pointer-call")).await.unwrap();
        let mut input = VoiceDialogueRequest::scoped("pointer-session", "pointer-turn", "保存这次对话");
        input.call_request_id = Some("pointer-call".into());
        recorder.preflight(&input).await.unwrap();
        recorder.record_pair(&new, &input, r#"{"intent_type":"chat","reply_tts":"已保存。"}"#).await.unwrap();
        assert_eq!(recorder.resolve_call_target(&auto, None, None).await.unwrap().0, new);
        let empty = recorder.resolve_call_target(&target, None, Some("unused-next-call")).await.unwrap().0;
        assert_ne!(empty, new);
        assert_eq!(recorder.resolve_call_target(&auto, None, None).await.unwrap().0, new);
    }
}

#[tokio::test]
async fn concurrent_workspace_target_resolution_keeps_both_registry_entries() {
    use nomifun_api_types::voice::VoiceCallTarget;
    let data = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(fixture_service().await, USER_ID.into(), data.path());
    let target_a = VoiceCallTarget::Workspace {workspace: first.path().to_string_lossy().into_owned()};
    let target_b = VoiceCallTarget::Workspace {workspace: second.path().to_string_lossy().into_owned()};
    let (a, b) = tokio::join!(recorder.resolve_call_target(&target_a, None, None), recorder.resolve_call_target(&target_b, None, None));
    let a = a.unwrap().0;
    let b = b.unwrap().0;
    assert_ne!(a, b);
    assert_eq!(recorder.resolve_call_target(&target_a, None, None).await.unwrap().0, a);
    assert_eq!(recorder.resolve_call_target(&target_b, None, None).await.unwrap().0, b);
    let registry: serde_json::Value = serde_json::from_slice(&std::fs::read(data.path().join("voice").join(format!("workspace-sessions-{USER_ID}.json"))).unwrap()).unwrap();
    assert_eq!(registry["sessions"].as_object().unwrap().len(), 2);
    let owners = registry["sessions"].as_object().unwrap().values().map(|entry| entry["conversation_id"].as_str().unwrap().to_owned()).collect::<std::collections::HashSet<_>>();
    assert_eq!(owners, [a, b].into_iter().collect());
}

#[tokio::test]
async fn voice_placeholder_keeps_inflight_input_and_retains_the_late_real_reply() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let conversation = service.create_provisional_voice_conversation(USER_ID, None, Some("yolo"), None, "late-call", None, None).await.unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());
    recorder.set_call_reservation(Some(VoiceCallReservation { request_id: "late-call".into(), target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {workspace: None, model: None, preset_id: None}, conversation_id: conversation.conversation_id.clone(), memory_owner: None, reserved_at_epoch_ms: nomifun_common::now_ms() }));
    recorder.freeze_session("late-session", Some("late-call")).await.unwrap();
    let mut request = VoiceDialogueRequest::scoped("late-session", "late-turn", "保留这句话");
    request.call_request_id = Some("late-call".into());
    recorder.preflight(&request).await.unwrap();
    recorder.finish_call_placeholder(&conversation.conversation_id, "late-call").await.unwrap();
    recorder.cleanup_ended_placeholders(None, &[]).await.unwrap();
    assert_ne!(service.get(USER_ID, &conversation.conversation_id).await.unwrap().extra["voice_lifecycle"], "discarded");
    recorder.record_pair(&conversation.conversation_id, &request, r#"{"intent_type":"chat","reply_tts":"已保留。"}"#).await.unwrap();
    recorder.cleanup_ended_placeholders(None, &[]).await.unwrap();
    assert_eq!(service.get(USER_ID, &conversation.conversation_id).await.unwrap().extra["voice_lifecycle"], "retained");
    assert_eq!(service.list_messages(USER_ID, &conversation.conversation_id, Default::default()).await.unwrap().items.len(), 2);
}

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
async fn new_bindings_ignore_retired_global_execution_and_recovery_keeps_the_frozen_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let unrelated = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());
    let initial = recorder.freeze_session("before-retired-config", None).await.unwrap().unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("voice_companion.json");
    let legacy = serde_json::json!({"execution":{"workspace":unrelated.path(),"model":null,"preset_id":null},
        "dialogue_model":{"provider_id":"deleted-provider","model":"deleted-model"},"autostart":false});
    std::fs::write(&config_path, legacy.to_string()).unwrap();
    let fresh = recorder.freeze_session("after-retired-config", None).await.unwrap().unwrap();
    assert_eq!(fresh.workspace, initial.workspace, "a new default binding must ignore the retired global workspace");
    assert_ne!(fresh.workspace.as_deref(), unrelated.path().to_str());
    let restored = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());
    assert_eq!(restored.freeze_session("before-retired-config", None).await.unwrap().unwrap().workspace, initial.workspace);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(&config_path).unwrap()).unwrap(), legacy,
        "resolving a binding must not rewrite other preferences or historical configuration");
    let malformed = serde_json::json!({"execution":"retired malformed value","autostart":false});
    std::fs::write(&config_path, malformed.to_string()).unwrap();
    assert_eq!(restored.freeze_session("malformed-retired-config", None).await.unwrap().unwrap().workspace, initial.workspace);
    // Existing journals may still contain an old accepted task's frozen directory.
    let journal_path = dir.path().join("voice").join(format!("dialogue-{USER_ID}.json"));
    let mut journal: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
    journal[0]["execution"]["workspace"] = unrelated.path().to_string_lossy().into_owned().into();
    std::fs::write(&journal_path,journal.to_string()).unwrap();
    let recovered = ConversationVoiceDialogueRecorder::new(service, USER_ID.into(), dir.path());
    assert_eq!(recovered.freeze_session("before-retired-config", None).await.unwrap().unwrap().workspace.as_deref(), unrelated.path().to_str(),
        "recovery must preserve old frozen parameters rather than retroactively rebind them");
}

#[tokio::test]
async fn ignored_turns_leave_no_public_messages_and_do_not_displace_valid_handoff() {
    let dir=tempfile::tempdir().unwrap();
    let service=fixture_service().await;
    let recorder=ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path());
    let valid=VoiceDialogueRequest::scoped("ignore-session","valid-turn","不是北京，是深圳，数量是2");
    let VoiceDialoguePreflight::Ready{conversation_id}=recorder.preflight(&valid).await.unwrap() else {panic!("expected ready")};
    recorder.record_pair(&conversation_id,&valid,r#"{"intent_type":"chat","understood_request":"用户纠正城市和数量","reply_tts":"好的，记住了。"}"#).await.unwrap();
    let before=service.list_messages(USER_ID,&conversation_id,Default::default()).await.unwrap().items.len();
    for index in 0..4 {
        let ignored=VoiceDialogueRequest::scoped("ignore-session",&format!("noise-{index}"),"无效杂音不能成为需求");
        recorder.preflight(&ignored).await.unwrap();
        recorder.record_pair(&conversation_id,&ignored,r#"{"intent_type":"ignore"}"#).await.unwrap();
    }
    let messages=service.list_messages(USER_ID,&conversation_id,Default::default()).await.unwrap();
    assert_eq!(messages.items.len(),before,"ignored raw text must not publish messages or trigger their memory collection");
    assert!(recorder.is_projected("ignore-session",&conversation_id,"valid-turn"));
    assert!(!recorder.is_projected("another-session",&conversation_id,"valid-turn"));
    assert!(!recorder.is_projected("ignore-session","another-conversation","valid-turn"));
    assert!(recorder.projected_message_ids("ignore-session",&conversation_id,"valid-turn").is_some());
    assert!(recorder.projected_message_ids("another-session",&conversation_id,"valid-turn").is_none());
    let restored=ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path());
    assert!(!restored.is_projected("ignore-session",&conversation_id,"valid-turn"));
    restored.freeze_session("ignore-session", None).await.unwrap();
    assert!(restored.is_projected("ignore-session",&conversation_id,"valid-turn"),"binding restore must retire persisted live previews without regenerating");
    let next=VoiceDialogueRequest::scoped("ignore-session","task-turn","按刚才要求创建订单");
    recorder.preflight(&next).await.unwrap();
    recorder.record_pair(&conversation_id,&next,r#"{"intent_type":"agent_task","task_goal":"创建订单","spoken_ack":"正在处理"}"#).await.unwrap();
    let command=nomifun_api_types::voice::VoiceTaskCommand {command_id:"ignore-handoff".into(),session_id:"ignore-session".into(),turn_id:"task-turn".into(),goal:"创建订单".into(),record_id:next.voice_record_id.clone(),binding_id:next.binding_id.clone()};
    let handoff=recorder.task_configuration(&command).await.unwrap().unwrap().handoff_content.unwrap();
    assert!(handoff.contains("深圳")&&handoff.contains("数量是2"));
    assert!(!handoff.contains("无效杂音"));
}

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

    // A normal chat supplies selection context, but must not be converted into a voice chat.
    let req: CreateConversationRequest = serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "项目对话",
        "extra": {},
    })).unwrap();
    let existing = conversation_svc.create(user_id, req).await.unwrap();
    let existing_conv_id = existing.conversation_id;

    let req_valid = VoiceDialogueRequest::scoped("valid-session", "valid-turn", "你好");
    recorder.active_window_candidate.write().unwrap().conversation_id=Some(existing_conv_id.clone());
    let preflight = recorder.preflight(&req_valid).await.unwrap();
    match preflight {
        VoiceDialoguePreflight::Ready { conversation_id } => {
            assert_ne!(conversation_id, existing_conv_id);
            assert_eq!(conversation_svc.get(user_id,&conversation_id).await.unwrap().extra["voice"],true);
            assert!(conversation_svc.list_messages(user_id,&existing_conv_id,Default::default()).await.unwrap().items.is_empty());
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

    let candidate = std::sync::Arc::new(std::sync::RwLock::new(VoiceWindowContext { conversation_id: Some(candidate_conv_id.clone()), ..Default::default() }));
    let journal_dir = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(
        conversation_svc.clone(),
        user_id.to_string(),
        journal_dir.path(),
    )
    .with_active_window_candidate(candidate.clone());

    // With no explicit project, both idle and busy ordinary chats use the separate default voice chat.
    let req = VoiceDialogueRequest::scoped("session-idle", "turn-idle", "你好");
    let preflight = recorder.preflight(&req).await.unwrap();
    let VoiceDialoguePreflight::Ready {conversation_id:idle_voice_id} = preflight else {panic!("expected Ready")};
    assert_ne!(idle_voice_id,candidate_conv_id);

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
            assert_eq!(conversation_id,idle_voice_id);
            let row = conv_repo.get(&conversation_id).await.unwrap().unwrap();
            assert_eq!(row.name, "语音助手");
        }
        _ => panic!("expected Ready"),
    }
}

#[tokio::test]
async fn bv_03_busy_workspace_candidate_preserves_workspace_voice_conversation() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).expect("canonical fixture owner"),
    )
    .await
    .unwrap();
    let conv_repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let user_id = USER_ID;

    let ws_dir = tempfile::tempdir().unwrap();
    let ws_str = ws_dir.path().to_string_lossy().to_string();

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
        "name": "项目开发",
        "extra": {
            "workspace": ws_str,
        },
    })).unwrap();
    let candidate = conversation_svc.create(user_id, req).await.unwrap();
    let candidate_conv_id = candidate.conversation_id;

    // Mark candidate conversation as busy
    sqlx::query("UPDATE conversations SET active_turn_operation_id = 'turn-test-busy' WHERE conversation_id = ?")
        .bind(&candidate_conv_id)
        .execute(db.pool())
        .await
        .unwrap();

    let candidate_holder = std::sync::Arc::new(std::sync::RwLock::new(VoiceWindowContext { conversation_id: Some(candidate_conv_id.clone()), ..Default::default() }));
    let journal_dir = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(
        conversation_svc.clone(),
        user_id.to_string(),
        journal_dir.path(),
    )
    .with_active_window_candidate(candidate_holder);

    // F03: When candidate has a workspace and is busy, preflight MUST resolve to the workspace voice conversation, NOT default "语音助手"
    let req_busy = VoiceDialogueRequest::scoped("session-ws-busy", "turn-ws-busy", "你好");
    let preflight_busy = recorder.preflight(&req_busy).await.unwrap();
    match preflight_busy {
        VoiceDialoguePreflight::Ready { conversation_id } => {
            let row = conv_repo.get(&conversation_id).await.unwrap().unwrap();
            assert_ne!(row.name, "语音助手", "must not degrade to default voice assistant");
            assert!(row.name.starts_with("语音 - "), "must resolve workspace voice conversation");
            let extra: serde_json::Value = serde_json::from_str(&row.extra).unwrap_or_default();
            assert!(extra.get("voice_task_workspace").is_some());
        }
        _ => panic!("expected Ready"),
    }
}

#[tokio::test]
async fn bv_04_voice_target_becoming_busy_during_generation_preserves_append() {
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
    let _ordinary = conversation_svc.create(user_id, req).await.unwrap();
    let target_conv_id = conversation_svc.ensure_default_voice_conversation(user_id).await.unwrap();

    let journal_dir = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(
        conversation_svc.clone(),
        user_id.to_string(),
        journal_dir.path(),
    );

    let req = VoiceDialogueRequest::scoped("session-busy", "turn-busy", "处理中");
    recorder.active_window_candidate.write().unwrap().conversation_id=Some(target_conv_id.clone());

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

    // BV-04: record_pair supports busy append while target is busy
    let res = recorder
        .record_pair(&target_conv_id, &req, r#"{"intent_type":"chat","reply_tts":"助手回复"}"#)
        .await;
    assert!(res.is_ok(), "record_pair must support appending while target is busy");
    let messages = conversation_svc.list_messages(user_id, &target_conv_id, Default::default()).await.unwrap().items;
    let user = messages.iter().find(|m| m.position == Some(nomifun_common::MessagePosition::Right)).unwrap();
    let assistant = messages.iter().find(|m| m.position == Some(nomifun_common::MessagePosition::Left)).unwrap();
    assert_eq!(assistant.content["turn_id"].as_str(), Some(user.message_id.as_str()), "busy projection must retain the billed user turn");
}


async fn fixture_service() -> Arc<ConversationService> {
    fixture_service_with_models(&[]).await
}

async fn fixture_file_service(path: &std::path::Path) -> (nomifun_db::Database, Arc<ConversationService>) {
    if !path.exists() {
        let seed = nomifun_db::init_database_memory_with_owner(nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap()).await.unwrap();
        seed.snapshot_into(path).await.unwrap();
        seed.close().await;
    }
    let database = nomifun_db::init_database(path).await.unwrap();
    let pool = database.pool().clone();
    let service = Arc::new(ConversationService::new(Arc::<str>::from(USER_ID), path.parent().unwrap().into(), Arc::new(NullSink), Arc::new(NoopSkillResolver), Arc::new(EmptyRegistry),
        Arc::new(SqliteConversationRepository::new(pool.clone())), Arc::new(SqliteAgentMetadataRepository::new(pool.clone())), Arc::new(SqliteAcpSessionRepository::new(pool)),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary)));
    (database, service)
}

async fn fixture_service_with_models(models: &[&str]) -> Arc<ConversationService> {
    let db=nomifun_db::init_database_memory_with_owner(nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap()).await.unwrap();
    if !models.is_empty() {
        use nomifun_db::IProviderRepository;
        nomifun_db::SqliteProviderRepository::new(db.pool().clone()).create(nomifun_db::CreateProviderParams {
            provider_id:Some(USER_ID),platform:"openai",name:"binding fixture",base_url:"http://127.0.0.1:1/v1",api_key_encrypted:"unused",
            models:&serde_json::to_string(models).unwrap(),enabled:true,model_context_limits:None,model_protocols:None,model_descriptions:None,
            model_enabled:None,bedrock_config:None,is_full_url:false,sort_order:None,
        }).await.unwrap();
    }
    Arc::new(ConversationService::new(Arc::<str>::from(USER_ID),std::env::temp_dir(),Arc::new(NullSink),Arc::new(NoopSkillResolver),Arc::new(EmptyRegistry),
        Arc::new(SqliteConversationRepository::new(db.pool().clone())),Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),Arc::new(nomifun_conversation::NoExecutionConversationBoundary)))
}

struct AttributionCompleter(std::sync::Mutex<Vec<nomifun_ai_agent::FlowyProxyAttribution>>);

#[async_trait]
impl nomifun_app::voice_dialogue::VoiceDialogueCompleter for AttributionCompleter {
    async fn resolve_model(
        &self,
        _: Option<&nomifun_api_types::voice::VoiceModelRef>,
    ) -> Result<nomifun_api_types::voice::VoiceModelRef, String> {
        Err("unused".into())
    }
    async fn complete(
        &self,
        _: &str,
        _: Option<&nomifun_api_types::voice::VoiceModelRef>,
    ) -> Result<String, String> {
        self.0
            .lock()
            .unwrap()
            .push(nomifun_ai_agent::FlowyProxyAttribution {
                session_id: nomifun_ai_agent::current_flowy_chat_session_id(),
                turn_id: nomifun_ai_agent::current_flowy_billing_turn_id(),
            });
        Ok(r#"{"intent_type":"chat","understood_request":"询问日期","reply_tts":"我来确认日期。"}"#.into())
    }
}

#[tokio::test]
async fn voice_fast_model_billing_uses_durable_user_turn_for_http_and_spawned_stream() {
    use nomifun_app::voice_dialogue::VoiceDialogueBridge;
    for stream in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let service = fixture_service().await;
        let recorder = Arc::new(ConversationVoiceDialogueRecorder::new(
            service.clone(),
            USER_ID.into(),
            dir.path(),
        ));
        let completer = Arc::new(AttributionCompleter(Default::default()));
        let bridge = VoiceDialogueBridge::start_with_recorder(
            completer.clone(),
            Default::default(),
            "billing-test".into(),
            axum::Router::new(),
            Some(recorder.clone()),
        )
        .await
        .unwrap();
        let mut request =
            VoiceDialogueRequest::scoped("billing-session", "acoustic-turn", "今天星期几？");
        request.prompt = "请理解并回答用户关于日期的问题".into();
        request.stream = stream;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        for _ in 0..2 {
            let response = client
                .post(&bridge.endpoint)
                .header("x-ai-mate-control-token", "billing-test")
                .json(&request)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let is_stream = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("application/x-ndjson"));
            let body = response.text().await.unwrap();
            assert!(status.is_success(), "{status}: {body}");
            if is_stream {
                let frames: Vec<nomifun_api_types::voice::VoiceDialogueFrame> = body
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert!(
                    matches!(frames.last(), Some(nomifun_api_types::voice::VoiceDialogueFrame::Complete { response }) if response.record_error.is_none()),
                    "{body}"
                );
            } else {
                let response: nomifun_api_types::voice::VoiceDialogueResponse =
                    serde_json::from_str(&body).unwrap();
                assert!(response.record_error.is_none());
            }
        }
        let binding = recorder
            .freeze_session("billing-session", None)
            .await
            .unwrap()
            .unwrap();
        let messages = service
            .list_messages(USER_ID, &binding.conversation_id, Default::default())
            .await
            .unwrap()
            .items;
        assert_eq!(messages.len(), 2, "cached replay cannot create a new pair");
        let user = messages
            .iter()
            .find(|m| m.position == Some(nomifun_common::MessagePosition::Right))
            .unwrap();
        let assistant = messages
            .iter()
            .find(|m| m.position == Some(nomifun_common::MessagePosition::Left))
            .unwrap();
        let calls = completer.0.lock().unwrap();
        assert_eq!(
            calls.len(),
            1,
            "cached replay cannot bill a second generation"
        );
        assert_eq!(
            calls[0].session_id.as_deref(),
            Some(binding.conversation_id.as_str())
        );
        assert_eq!(calls[0].turn_id.as_deref(), Some(user.message_id.as_str()));
        assert_eq!(
            assistant.content["turn_id"].as_str(),
            Some(user.message_id.as_str())
        );
        assert!(
            nomifun_ai_agent::current_flowy_billing_turn_id().is_none(),
            "billing scope cannot escape the model call"
        );
    }
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
    use voice_companion::replay_terminal::{driver::VoiceRouter,router::OpenAiCompatibleVoiceRouter,completion::HostDialogueClient};
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
    let binding=recorder.freeze_session("joint-session", None).await.unwrap().unwrap();
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
    assert!(matches!(recorder.record_pair(&conv,&request,output).await,Err(VoiceDialogueRecorderError::BindingInvalid(_))));
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
    let dir=tempfile::tempdir().unwrap();let workspace_a=tempfile::tempdir().unwrap();let workspace_b=tempfile::tempdir().unwrap();let service=fixture_service_with_models(&["project-a","project-b"]).await;
    let create=|workspace:&std::path::Path,model:&str|serde_json::from_value::<CreateConversationRequest>(serde_json::json!({"type":"nomi","name":"本地项目","model":{"provider_id":USER_ID,"model":model},"extra":{"workspace":workspace.to_string_lossy()}})).unwrap();
    let a=service.create(USER_ID,create(workspace_a.path(),"project-a")).await.unwrap();let b=service.create(USER_ID,create(workspace_b.path(),"project-b")).await.unwrap();
    let candidate=Arc::new(std::sync::RwLock::new(VoiceWindowContext { conversation_id: Some(a.conversation_id.clone()), ..Default::default() }));
    let recorder=ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path()).with_active_window_candidate(candidate.clone());
    let frozen=recorder.freeze_session("frozen-session", None).await.unwrap().unwrap();
    let bound_conv=frozen.conversation_id.clone();
    assert_ne!(bound_conv, a.conversation_id, "must resolve to workspace dedicated voice conversation per C03");
    candidate.write().unwrap().conversation_id=Some(b.conversation_id.clone());
    let request=VoiceDialogueRequest::scoped("frozen-session","frozen-turn","读取这个项目");
    assert_eq!(recorder.preflight(&request).await.unwrap(),VoiceDialoguePreflight::Ready{conversation_id:bound_conv.clone()});
    recorder.record_pair(&bound_conv,&request,r#"{"intent_type":"agent_task","understood_request":"读取当前项目。","task_goal":"读取项目A","spoken_ack":"未接受前不能播出"}"#).await.unwrap();
    assert_eq!(service.list_messages(USER_ID,&a.conversation_id,Default::default()).await.unwrap().items.len(), 0, "original normal chat remains untouched");
    let command=VoiceTaskCommand{command_id:"voice-frozen-turn".into(),session_id:"frozen-session".into(),turn_id:"frozen-turn".into(),goal:"读取项目A".into(),record_id:request.voice_record_id.clone(),binding_id:request.binding_id.clone()};
    let config=recorder.task_configuration(&command).await.unwrap().unwrap();
    assert_eq!(config.config.workspace,nomifun_common::paths::canonicalize_simplified(workspace_a.path()).unwrap().to_string_lossy());
    assert_eq!(config.config.model.as_ref().unwrap().model,"project-a");
    assert_eq!(recorder.bound_execution("frozen-session").await.unwrap().unwrap(),config.config);
    let implicit_command=VoiceTaskCommand{command_id:"voice-frozen-turn".into(),session_id:"frozen-session".into(),turn_id:"frozen-turn".into(),goal:"读取项目A".into(),record_id:None,binding_id:None};
    let implicit_config=recorder.task_configuration(&implicit_command).await.unwrap().unwrap();
    assert_eq!(implicit_config.config.workspace,config.config.workspace);
    let mut new_identity=command.clone();new_identity.command_id="different-delivery-for-same-record".into();
    assert_eq!(recorder.task_configuration(&new_identity).await.err().unwrap(),"task_command_conflict","one voice decision cannot create another delivery identity");
    let mut changed=command;changed.goal="读取项目B".into();assert_eq!(recorder.task_configuration(&changed).await.err().unwrap(),"task_command_conflict");
}

#[tokio::test]
async fn a_post_generation_projection_failure_returns_text_and_retry_never_calls_the_model_again() {
    use voice_companion::replay_terminal::{driver::{VoiceRouter,RouteError},router::OpenAiCompatibleVoiceRouter,completion::HostDialogueClient};
    use nomifun_app::voice_dialogue::VoiceDialogueBridge;
    let dir=tempfile::tempdir().unwrap();let service=fixture_service().await;
    let recorder=Arc::new(ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path()));
    let completer=Arc::new(GatedCompleter{calls:Default::default(),entered:Default::default(),release:Default::default(),journal:dir.path().join("voice").join(format!("dialogue-{USER_ID}.json"))});
    let bridge=VoiceDialogueBridge::start_with_recorder(completer.clone(),Default::default(),"projection-control".into(),axum::Router::new(),Some(recorder.clone())).await.unwrap();
    let router=Arc::new(OpenAiCompatibleVoiceRouter::with_completion(Arc::new(HostDialogueClient::new(bridge.endpoint.clone(),"projection-control".into()).unwrap())));
    let scope=VoiceDialogueRequest::scoped("projection-session","projection-turn","你好，请介绍你自己");
    let r=router.clone();let req=scope.clone();let first=tokio::spawn(async move{r.route_scoped(&req,"你好，请介绍你自己",None,&[]).await});
    tokio::time::timeout(std::time::Duration::from_secs(3),completer.entered.notified()).await.unwrap();
    let binding=recorder.freeze_session("projection-session", None).await.unwrap().unwrap();
    service.conversation_repo().clear_terminal_conversation_messages(USER_ID,&binding.conversation_id,nomifun_common::now_ms()).await.unwrap();
    completer.release.notify_one();let first=first.await.unwrap();
    assert!(matches!(first.0,Err(RouteError::Record{ref code,..}) if code=="binding_invalid" || code=="binding_busy"));
    assert!(first.1.as_ref().unwrap().contains("你好，我是你的语音助手。"));
    let retry=router.route_scoped(&scope,"你好，请介绍你自己",None,&[]).await;
    assert!(matches!(retry.0,Err(RouteError::Record{ref code,..}) if code=="binding_invalid" || code=="binding_busy"));assert_eq!(first.1,retry.1);
    assert_eq!(completer.calls.load(std::sync::atomic::Ordering::SeqCst),1);
    assert_eq!(service.list_messages(USER_ID,&binding.conversation_id,Default::default()).await.unwrap().items.len(),0);
}

#[tokio::test]
async fn fallback_execution_resolves_valid_workspace_without_separate_voice_config() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());

    // 1. Without any existing project conversation or voice_companion.json config, fallback_execution resolves default voice workspace
    let fallback = recorder.fallback_execution().await.unwrap().expect("fallback execution must resolve default voice workspace");
    let path = std::path::Path::new(&fallback.workspace);
    assert!(path.is_absolute());
    assert!(path.is_dir());

    // 2. An unselected project conversation must NOT be borrowed as the fallback workspace
    let project_dir = tempfile::tempdir().unwrap();
    let create_req = serde_json::from_value::<CreateConversationRequest>(serde_json::json!({
        "type": "nomi",
        "name": "用户主要项目",
        "extra": { "workspace": project_dir.path().to_string_lossy() }
    })).unwrap();
    let _project_conv = service.create(USER_ID, create_req).await.unwrap();

    let project_fallback = recorder.fallback_execution().await.unwrap().expect("fallback must resolve default voice workspace");
    assert_eq!(
        project_fallback.workspace,
        fallback.workspace
    );

    // 3. Freezing a session without explicit binding or voice_companion.json uses default voice workspace
    let frozen = recorder.freeze_session("fallback-session", None).await.unwrap().unwrap();
    assert_eq!(
        frozen.workspace.as_deref(),
        Some(fallback.workspace.as_str())
    );
}

#[tokio::test]
async fn review_standalone_binding_must_not_borrow_an_unselected_project() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let home = service.ensure_default_voice_conversation(USER_ID).await.unwrap();
    let home_workspace = service
        .get(USER_ID, &home)
        .await
        .unwrap()
        .extra["workspace"]
        .as_str()
        .unwrap()
        .to_owned();
    let unrelated = tempfile::tempdir().unwrap();
    service
        .create(
            USER_ID,
            serde_json::from_value::<CreateConversationRequest>(serde_json::json!({
                "type": "nomi",
                "name": "unselected confidential project",
                "extra": { "workspace": unrelated.path().to_string_lossy() }
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(service, USER_ID.into(), dir.path());
    let binding = recorder.freeze_session("review-home", None).await.unwrap().unwrap();
    assert_eq!(binding.conversation_id, home);
    assert_eq!(
        binding.workspace.as_deref(),
        Some(home_workspace.as_str()),
        "standalone voice binding must not acquire an unrelated project's workspace"
    );
}

#[tokio::test]
async fn review_handoff_must_preserve_the_prior_user_city() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let recorder = ConversationVoiceDialogueRecorder::new(service, USER_ID.into(), dir.path());
    let first = VoiceDialogueRequest::scoped("review-context", "review-city", "我在深圳");
    let conversation = match recorder.preflight(&first).await.unwrap() {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!(),
    };
    recorder
        .record_pair(
            &conversation,
            &first,
            r#"{"intent_type":"chat","understood_request":"用户明确城市为深圳","reply_tts":"知道了，你在深圳。"}"#,
        )
        .await
        .unwrap();
    let next = VoiceDialogueRequest::scoped("review-context", "review-weather", "今天天气怎么样？");
    recorder.preflight(&next).await.unwrap();
    recorder
        .record_pair(
            &conversation,
            &next,
            r#"{"intent_type":"agent_task","understood_request":"用户询问今日天气","task_goal":"查询今天的天气","spoken_ack":"查询天气"}"#,
        )
        .await
        .unwrap();
    let command = nomifun_api_types::voice::VoiceTaskCommand {
        command_id: "review-weather-command".into(),
        session_id: "review-context".into(),
        turn_id: "review-weather".into(),
        goal: "查询今天的天气".into(),
        record_id: next.voice_record_id.clone(),
        binding_id: next.binding_id.clone(),
    };
    let handoff = recorder
        .task_configuration(&command)
        .await
        .unwrap()
        .unwrap()
        .handoff_content
        .unwrap();
    assert!(
        handoff.contains("深圳"),
        "known prior fact was dropped from runtime handoff: {handoff}"
    );
}

#[test]
fn review_spoken_summary_must_not_drop_the_actual_backup_failure() {
    let raw = "目录扫描已完成，已检查全部文件并统计文件数量和大小以及当前磁盘空间使用情况并生成了详细目录结构和待处理文件清单用于下一步执行，但是备份失败没有复制任何文件且目标目录仍然为空，请勿删除原始文件以免丢失数据。";
    let summary = nomifun_app::voice_companion_manager::extract_spoken_summary(
        raw,
        nomifun_api_types::voice::VoiceTaskStatus::Completed,
        false,
    );
    assert!(
        summary.contains("备份失败") || summary.contains("没有复制"),
        "spoken summary lost the actual failure: {summary}"
    );
}

#[test]
fn review_spoken_summary_empty_result_uses_the_call_locale() {
    use nomifun_api_types::voice::VoiceTaskStatus;
    use nomifun_app::voice_companion_manager::extract_spoken_summary;
    assert_eq!(extract_spoken_summary("", VoiceTaskStatus::Completed, true),
        "Execution finished. See conversation for details.");
    assert_eq!(extract_spoken_summary("", VoiceTaskStatus::Completed, false),
        "执行已结束，详情请在会话中查看。");
    assert_eq!(extract_spoken_summary("", VoiceTaskStatus::Failed, true),
        "Execution was unsuccessful. See conversation for details.");
}

#[tokio::test]
async fn review_missing_frozen_execution_cannot_bypass_the_cleared_conversation_fence() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());
    let request = VoiceDialogueRequest::scoped("review-cleared", "review-cleared-turn", "读取文件");
    let conversation = match recorder.preflight(&request).await.unwrap() {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!(),
    };
    recorder
        .record_pair(
            &conversation,
            &request,
            r#"{"intent_type":"agent_task","task_goal":"读取文件","spoken_ack":"读取文件"}"#,
        )
        .await
        .unwrap();
    // A missing frozen execution is a valid persisted state when production prepare_configuration fails.
    let journal = dir.path().join("voice").join(format!("dialogue-{USER_ID}.json"));
    let mut records: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    for record in records.as_array_mut().unwrap() {
        record["execution"] = serde_json::Value::Null;
    }
    std::fs::write(&journal, serde_json::to_vec(&records).unwrap()).unwrap();
    service
        .conversation_repo()
        .clear_terminal_conversation_messages(USER_ID, &conversation, nomifun_common::now_ms())
        .await
        .unwrap();
    let command = nomifun_api_types::voice::VoiceTaskCommand {
        command_id: "review-cleared-command".into(),
        session_id: "review-cleared".into(),
        turn_id: "review-cleared-turn".into(),
        goal: "读取文件".into(),
        record_id: request.voice_record_id.clone(),
        binding_id: request.binding_id.clone(),
    };
    let result = recorder.task_configuration(&command).await;
    assert!(
        result.is_err(),
        "cleared conversation with no frozen execution must be rejected, not Ok(None) that admits mutable fallback config"
    );
}

#[tokio::test]
async fn test_c01_lock_inversion_barrier_prevention() {
    let dir = tempfile::tempdir().unwrap();
    let models = Arc::new(nomifun_voice_models::ModelManager::new(
        dir.path().to_path_buf(),
    ));
    let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));

    let m1 = manager.clone();
    let b1 = barrier.clone();
    let handle1 = tokio::spawn(async move {
        for i in 0..100 {
            let snap = nomifun_api_types::voice::VoiceSessionSnapshot {
                instance_id: "daemon-test".into(),
                seq: i + 1,
                phase: nomifun_api_types::voice::VoiceSessionPhase::Listening,
                acoustic_ready: true,
                ..Default::default()
            };
            m1.project_snapshot_for_test(snap, 0);
            if i % 10 == 0 {
                b1.wait().await;
            }
            tokio::task::yield_now().await;
        }
    });

    let m2 = manager.clone();
    let b2 = barrier.clone();
    let handle2 = tokio::spawn(async move {
        for i in 0..100 {
            let task = nomifun_api_types::voice::VoiceTaskSnapshot {
                command_id: format!("cmd-{i}"),
                session_id: "session-test".into(),
                acoustic_turn_id: format!("turn-{i}"),
                conversation_id: Some("conv-test".into()),
                message_id: Some(format!("msg-{i}")),
                runtime_turn_id: None,
                status: nomifun_api_types::voice::VoiceTaskStatus::Accepted,
                result_ok: None,
                result_text: None,
                error_code: None,
            };
            m2.project_task_for_test(task);
            if i % 10 == 0 {
                b2.wait().await;
            }
            tokio::task::yield_now().await;
        }
    });

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let (r1, r2) = tokio::join!(handle1, handle2);
        r1.unwrap();
        r2.unwrap();
    })
    .await
    .expect("concurrent project and project_task must not deadlock");
}

#[tokio::test]
async fn test_c03_workspace_routing_and_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().to_string_lossy().to_string();

    // Create two normal chats that both point to the same workspace
    let chat1 = service.create(USER_ID, serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "Normal Chat 1",
        "extra": { "workspace": ws_path.clone() },
    })).unwrap()).await.unwrap();

    let chat2 = service.create(USER_ID, serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "Normal Chat 2",
        "extra": { "workspace": ws_path.clone() },
    })).unwrap()).await.unwrap();

    let candidate = std::sync::Arc::new(std::sync::RwLock::new(VoiceWindowContext { conversation_id: Some(chat1.conversation_id.clone()), ..Default::default() }));
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path())
        .with_active_window_candidate(candidate.clone());

    // Call preflight with chat1 as candidate
    let req1 = VoiceDialogueRequest::scoped("c03-session-1", "turn-1", "帮我看看这个项目");
    let res1 = recorder.preflight(&req1).await.unwrap();
    let voice_conv_id1 = match res1 {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!("expected Ready"),
    };

    // Voice conversation must NOT be chat1
    assert_ne!(voice_conv_id1, chat1.conversation_id);

    // Record dialogue pair for req1
    recorder.record_pair(&voice_conv_id1, &req1, r#"{"intent_type":"chat","understood_request":"查看项目","reply_tts":"好的"}"#).await.unwrap();

    // Switch candidate to chat2
    candidate.write().unwrap().conversation_id = Some(chat2.conversation_id.clone());

    // Call preflight with chat2 as candidate
    let req2 = VoiceDialogueRequest::scoped("c03-session-2", "turn-2", "项目里有什么文件");
    let res2 = recorder.preflight(&req2).await.unwrap();
    let voice_conv_id2 = match res2 {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!("expected Ready"),
    };

    // Both normal chats in the same workspace MUST resolve to the SAME dedicated voice conversation!
    assert_eq!(voice_conv_id1, voice_conv_id2);

    // Verify chat1 and chat2 have 0 messages in DB
    assert_eq!(service.list_messages(USER_ID, &chat1.conversation_id, Default::default()).await.unwrap().items.len(), 0, "chat1 must have 0 messages");
    assert_eq!(service.list_messages(USER_ID, &chat2.conversation_id, Default::default()).await.unwrap().items.len(), 0, "chat2 must have 0 messages");

    // The dedicated voice conversation has the recorded message
    assert!(service.list_messages(USER_ID, &voice_conv_id1, Default::default()).await.unwrap().items.len() > 0, "dedicated voice conversation must receive the messages");
}

#[tokio::test]
async fn test_c04_corrupted_registry_and_crash_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().to_string_lossy().to_string();

    let chat = service.create(USER_ID, serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "Normal Chat",
        "extra": { "workspace": ws_path.clone() },
    })).unwrap()).await.unwrap();

    let candidate = std::sync::Arc::new(std::sync::RwLock::new(VoiceWindowContext { conversation_id: Some(chat.conversation_id.clone()), ..Default::default() }));
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path())
        .with_active_window_candidate(candidate.clone());
    let sessions_path = dir.path().join("voice").join(format!("workspace-sessions-{USER_ID}.json"));
    std::fs::create_dir_all(sessions_path.parent().unwrap()).unwrap();

    // 1. Corrupted registry file: writing invalid JSON must return Err(VoiceDialogueRecorderError::WriteFailed)
    std::fs::write(&sessions_path, b"NOT_A_VALID_JSON{[[{").unwrap();
    let req1 = VoiceDialogueRequest::scoped("c04-session", "turn-1", "测试损坏注册表");
    let err = recorder.preflight(&req1).await.unwrap_err();
    assert!(matches!(err, VoiceDialogueRecorderError::WriteFailed(_)), "corrupted registry must return WriteFailed error");

    // 2. Crash recovery: simulated state where registry has creation_key but conversation_id is empty,
    // and the conversation ALREADY exists in DB under that creation_key.
    let canonical = nomifun_common::paths::canonicalize_simplified(ws_dir.path()).unwrap();
    let canonical_str = canonical.to_string_lossy().to_string();
    let test_creation_key = format!("voice-create:workspace:{canonical_str}:999999");

    // Pre-create the conversation with this creation_key in DB
    let created = service.create_voice_idempotent(
        USER_ID,
        serde_json::from_value(serde_json::json!({
            "type": "nomi",
            "name": "语音 - 恢复测试",
            "extra": { "voice": true },
        })).unwrap(),
        &test_creation_key,
        &canonical_str,
        None,
    ).await.unwrap();

    // Write crash recovery registry state: creation_key is set, conversation_id is EMPTY ("")
    let registry_json = serde_json::json!({
        "sessions": {
            canonical_str.clone(): {
                "workspace": canonical_str.clone(),
                "conversation_id": "",
                "creation_key": test_creation_key,
                "last_user_interaction_at": nomifun_common::now_ms()
            }
        }
    });
    std::fs::write(&sessions_path, serde_json::to_vec(&registry_json).unwrap()).unwrap();

    // Now call preflight again: it must recover created.conversation_id and NOT create a new one!
    let req2 = VoiceDialogueRequest::scoped("c04-session-rec", "turn-rec", "测试崩溃恢复");
    let res = recorder.preflight(&req2).await.unwrap();
    match res {
        VoiceDialoguePreflight::Ready { conversation_id } => {
            assert_eq!(conversation_id, created.conversation_id, "crash recovery must reuse existing conversation by creation_key");
        }
        _ => panic!("expected Ready"),
    }

    // Verify registry was backfilled with conversation_id
    let updated_bytes = std::fs::read(&sessions_path).unwrap();
    let updated_json: serde_json::Value = serde_json::from_slice(&updated_bytes).unwrap();
    assert_eq!(
        updated_json["sessions"][&canonical_str]["conversation_id"].as_str().unwrap(),
        created.conversation_id
    );
}

#[tokio::test]
async fn test_c08_context_truncation_prioritizes_recent_correction() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let recorder = ConversationVoiceDialogueRecorder::new(service, USER_ID.into(), dir.path());

    // Turn 1: bulky turn (>1000 chars)
    let bulky_part = "这是一段非常冗长详细的对话历史记录，用户在其中详细描述了各种背景、业务历史、前期讨论内容以及各种无关紧要的细节要求。";
    let bulky_text = bulky_part.repeat(18);
    let first = VoiceDialogueRequest::scoped("c08-context", "turn-1", &bulky_text);
    let conversation = match recorder.preflight(&first).await.unwrap() {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!(),
    };
    recorder.record_pair(
        &conversation,
        &first,
        r#"{"intent_type":"chat","understood_request":"用户提供了冗长的背景描述","reply_tts":"了解了您的背景信息。"}"#,
    ).await.unwrap();

    // Turn 2: short recent correction
    let second = VoiceDialogueRequest::scoped("c08-context", "turn-2", "不是北京，改深圳，数量改为 2 不是 20");
    recorder.preflight(&second).await.unwrap();
    recorder.record_pair(
        &conversation,
        &second,
        r#"{"intent_type":"chat","understood_request":"用户修正城市为深圳数量为2","reply_tts":"好的，已为您更正为深圳和数量2。"}"#,
    ).await.unwrap();

    // Turn 3: Agent Task
    let third = VoiceDialogueRequest::scoped("c08-context", "turn-3", "帮我创建订单");
    recorder.preflight(&third).await.unwrap();
    recorder.record_pair(
        &conversation,
        &third,
        r#"{"intent_type":"agent_task","understood_request":"用户要求创建订单","task_goal":"创建订单","spoken_ack":"正在创建订单"}"#,
    ).await.unwrap();

    let command = nomifun_api_types::voice::VoiceTaskCommand {
        command_id: "c08-order-command".into(),
        session_id: "c08-context".into(),
        turn_id: "turn-3".into(),
        goal: "创建订单".into(),
        record_id: third.voice_record_id.clone(),
        binding_id: third.binding_id.clone(),
    };

    let handoff = recorder
        .task_configuration(&command)
        .await
        .unwrap()
        .unwrap()
        .handoff_content
        .unwrap();

    // Handoff must preserve the recent correction and turn_id!
    assert!(handoff.contains("[轮次 turn-2]"), "handoff must preserve turn-2 marker: {handoff}");
    assert!(handoff.contains("深圳"), "handoff must preserve recent city correction: {handoff}");
    assert!(handoff.contains("数量改为 2"), "handoff must preserve recent quantity correction: {handoff}");
    // Bulky turn 1 was omitted due to budget exhaustion from reverse traversal
    assert!(handoff.contains("（更早轮次已按预算略过）"), "handoff must note omitted earlier turns: {handoff}");
    assert!(!handoff.contains("这是一段非常冗长详细的对话历史记录"), "bulky turn 1 must be omitted to stay within budget: {handoff}");
}

#[tokio::test]
async fn workspace_interaction_registry_failure_keeps_generated_reply_retryable_without_duplicate_bubbles() {
    let dir=tempfile::tempdir().unwrap();
    let workspace=tempfile::tempdir().unwrap();
    let service=fixture_service().await;
    let source=service.create(USER_ID,serde_json::from_value(serde_json::json!({"type":"nomi","extra":{"workspace":workspace.path()}})).unwrap()).await.unwrap();
    let candidate=Arc::new(std::sync::RwLock::new(VoiceWindowContext { conversation_id: Some(source.conversation_id), ..Default::default() }));
    let recorder=ConversationVoiceDialogueRecorder::new(service.clone(),USER_ID.into(),dir.path()).with_active_window_candidate(candidate);
    let request=VoiceDialogueRequest::scoped("registry-failure-session","registry-failure-turn","你好");
    let conversation=match recorder.preflight(&request).await.unwrap(){VoiceDialoguePreflight::Ready{conversation_id}=>conversation_id,_=>panic!("ready")};
    let registry=dir.path().join("voice").join(format!("workspace-sessions-{USER_ID}.json"));
    let saved=std::fs::read(&registry).unwrap();
    std::fs::write(&registry,b"corrupt registry").unwrap();
    let reply=r#"{"intent_type":"chat","reply_tts":"你好，我在听。"}"#;
    assert!(recorder.record_pair(&conversation,&request,reply).await.is_err());
    let records:serde_json::Value=serde_json::from_slice(&std::fs::read(dir.path().join("voice").join(format!("dialogue-{USER_ID}.json"))).unwrap()).unwrap();
    let record=records.as_array().unwrap().iter().find(|r|r["request"]["turn_id"]=="registry-failure-turn").unwrap();
    assert_eq!(record["output"],reply);
    assert_eq!(record["projected"],false);
    std::fs::write(&registry,saved).unwrap();
    recorder.record_pair(&conversation,&request,reply).await.unwrap();
    assert!(matches!(recorder.preflight(&request).await.unwrap(),VoiceDialoguePreflight::Cached{..}));
    let messages=service.list_messages(USER_ID,&conversation,Default::default()).await.unwrap();
    assert_eq!(messages.items.len(),2);
}

#[tokio::test]
async fn voice_dialogue_and_assistant_projection_share_a_timeline_without_polluting_foreground_chat() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().to_string_lossy().to_string();

    // 1. Create a random temporary file in workspace P
    let random_nonce = format!("nonce-{}", nomifun_common::now_ms());
    let target_filename = "test_doc.txt";
    let target_file_path = ws_dir.path().join(target_filename);
    std::fs::write(&target_file_path, format!("Random File Content: {random_nonce}")).unwrap();

    // 2. Main window is in normal chat for workspace P
    let normal_chat = service.create(USER_ID, serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "P 普通聊天",
        "extra": { "workspace": ws_path.clone() },
    })).unwrap()).await.unwrap();

    let candidate = std::sync::Arc::new(std::sync::RwLock::new(VoiceWindowContext { conversation_id: Some(normal_chat.conversation_id.clone()), ..Default::default() }));
    let recorder = std::sync::Arc::new(
        ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path())
            .with_active_window_candidate(candidate.clone())
    );

    // Service-boundary test: no pet window, PCM, provider, runtime tool or speaker is exercised here.
    let voice_req = VoiceDialogueRequest::scoped("ui1-pet-session", "ui1-turn-1", "请读取 test_doc.txt 的内容并告诉我");
    let preflight_res = recorder.preflight(&voice_req).await.unwrap();
    let voice_conv_id = match preflight_res {
        VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
        _ => panic!("expected Ready voice conversation"),
    };

    // Dedicated voice conversation is created, distinct from normal chat
    assert_ne!(voice_conv_id, normal_chat.conversation_id);

    // Project an already obtained fast-model decision.
    let output_json = serde_json::json!({
        "intent_type": "agent_task",
        "understood_request": "读取文件 test_doc.txt",
        "task_goal": "读取文件 test_doc.txt",
        "spoken_ack": "正在为您读取文件",
        "reply_tts": "正在为您读取文件"
    }).to_string();

    recorder.record_pair(&voice_conv_id, &voice_req, &output_json).await.unwrap();

    // Local fixture content is a projection input, not evidence of runtime execution.
    let read_file_content = std::fs::read_to_string(&target_file_path).unwrap();
    assert!(read_file_content.contains(&random_nonce));

    // Assistant projection must be left-side content, not another user message.
    let execution_result_text = format!("文件已成功读取，内容为：{read_file_content}");
    service.project_assistant_message_idempotent(
        USER_ID,
        &voice_conv_id,
        "cmd-ui1-1",
        &execution_result_text,
        "voice-task-result",
    ).await.unwrap();

    // 8. Assertions:
    // - Normal chat P has ZERO messages (was NOT polluted)
    let normal_msgs = service.list_messages(USER_ID, &normal_chat.conversation_id, Default::default()).await.unwrap();
    assert_eq!(normal_msgs.items.len(), 0, "normal chat must have 0 messages");

    // - Dedicated voice conversation has all 3 messages: user request, spoken ack reply, and assistant result with real file content
    let voice_msgs = service.list_messages(USER_ID, &voice_conv_id, Default::default()).await.unwrap();
    assert_eq!(voice_msgs.items.len(), 3, "voice conversation must have exactly 3 messages: prompt, quick reply ack, and tool result");
    assert_eq!(voice_msgs.items.iter().filter(|m|m.position==Some(nomifun_common::MessagePosition::Right)).count(),1);
    assert_eq!(voice_msgs.items.iter().filter(|m|m.position==Some(nomifun_common::MessagePosition::Left)).count(),2);
    assert!(voice_msgs.items.iter().any(|m| m.content["content"] == "请读取 test_doc.txt 的内容并告诉我"));
    assert!(voice_msgs.items.iter().any(|m| m.content["content"].as_str().is_some_and(|c| c.contains("正在为您读取文件"))));
    assert!(voice_msgs.items.iter().any(|m| m.content["content"].as_str().is_some_and(|c| c.contains(&random_nonce))));
}

#[tokio::test]
async fn voice_call_reservation_freezes_target_even_when_navigating_and_supports_explicit_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service().await;
    let ws_p_dir = tempfile::tempdir().unwrap();
    let ws_p_path = ws_p_dir.path().to_string_lossy().to_string();
    let ws_q_dir = tempfile::tempdir().unwrap();
    let ws_q_path = ws_q_dir.path().to_string_lossy().to_string();

    // 1. Create chat in workspace Q
    let chat_q = service.create(USER_ID, serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "Q 聊天",
        "extra": { "workspace": ws_q_path.clone() },
    })).unwrap()).await.unwrap();

    let candidate = std::sync::Arc::new(std::sync::RwLock::new(VoiceWindowContext::default()));
    let recorder = std::sync::Arc::new(
        ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path())
            .with_active_window_candidate(candidate.clone())
    );

    // 2. Start call on Workspace P: target resolves to a dedicated voice conversation for P
    let target_p = nomifun_api_types::voice::VoiceCallTarget::Workspace { workspace: ws_p_path.clone() };
    let (conv_p_id, memory_owner) = recorder.resolve_call_target(&target_p, None, None).await.unwrap();

    // Host freezes reservation for P
    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: "req-p-1".into(),
        target: target_p,
        conversation_id: conv_p_id.clone(),
        memory_owner,
        reserved_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
    }));

    // 3. User navigates to window Q before session ready
    candidate.write().unwrap().conversation_id = Some(chat_q.conversation_id.clone());

    // 4. Session arrives and freezes
    let session_p = "session-p-ready";
    let binding_p = recorder.freeze_session(session_p, Some("req-p-1")).await.unwrap().expect("binding should be created");

    // R01 assertion: binding MUST be for P, NEVER polluted by navigation to Q!
    assert_eq!(binding_p.conversation_id, conv_p_id, "session MUST bind to frozen target P, not active candidate Q");
    assert_ne!(binding_p.conversation_id, chat_q.conversation_id);

    // 5. Test explicit old voice conversation target
    let old_voice_conv = service.create(USER_ID, serde_json::from_value(serde_json::json!({
        "type": "nomi",
        "name": "旧语音会话",
        "extra": {
            "voice": true,
            "workspace": ws_p_path.clone(),
            "memory_owner_companion_id": "flowy-bolt",
        },
    })).unwrap()).await.unwrap();

    let target_explicit = nomifun_api_types::voice::VoiceCallTarget::VoiceConversation {
        conversation_id: old_voice_conv.conversation_id.clone(),
    };
    let (resolved_explicit_id, owner_explicit) = recorder.resolve_call_target(&target_explicit, None, None).await.unwrap();
    assert_eq!(resolved_explicit_id, old_voice_conv.conversation_id);
    assert_eq!(owner_explicit.as_deref(), Some("flowy-bolt"));

    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: "req-explicit-1".into(),
        target: target_explicit.clone(),
        conversation_id: old_voice_conv.conversation_id.clone(),
        memory_owner: owner_explicit,
        reserved_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
    }));

    let session_explicit = "session-explicit-ready";
    let binding_explicit = recorder.freeze_session(session_explicit, Some("req-explicit-1")).await.unwrap().expect("binding should be created");
    assert_eq!(binding_explicit.conversation_id, old_voice_conv.conversation_id, "explicit voice conversation binds verbatim without rotation");
    assert!(recorder.current_call_reservation().is_none(), "reservation consumed only after successful persistence");

    // A late receipt after the UI timeout still belongs to its original target.
    let session_late = "session-late-ready";
    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: "req-late-1".into(),
        target: target_explicit.clone(),
        conversation_id: old_voice_conv.conversation_id.clone(),
        memory_owner: None,
        reserved_at_epoch_ms: chrono::Utc::now().timestamp_millis() - 11_000,
    }));
    let binding_late = recorder.freeze_session(session_late, Some("req-late-1")).await.unwrap().expect("binding should be created");
    assert_eq!(binding_late.conversation_id, old_voice_conv.conversation_id, "an expired UI wait must never rebind the receipt to project Q");
    assert!(recorder.current_call_reservation().is_none(), "reservation consumed on success");

    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id:"req-newer".into(),target:target_explicit,conversation_id:conv_p_id,
        memory_owner:None,reserved_at_epoch_ms:chrono::Utc::now().timestamp_millis(),
    }));
    assert!(matches!(recorder.freeze_session("old-session",Some("req-late-1")).await,Err(VoiceDialogueRecorderError::BindingInvalid(_))));
    assert!(matches!(recorder.freeze_session("unrelated-kws",None).await,Err(VoiceDialogueRecorderError::BindingInvalid(_))));
    assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-newer","old or uncorrelated wakes cannot consume a newer reservation");
    assert!(recorder.take_call_reservation("req-late-1").is_none());
    assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-newer","stale cleanup must not consume the newer reservation");
    assert_eq!(recorder.take_call_reservation("req-newer").unwrap().request_id,"req-newer");
    assert!(recorder.current_call_reservation().is_none());
}

#[tokio::test]
async fn voice_conversation_locale_titles_and_autotitle_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let service = fixture_service_with_models(&["voice-model"]).await;
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), dir.path());

    // 1. Chinese provisional conversation gets "语音通话"
    let target_zh = nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
        workspace: None,
        model: None,
        preset_id: None,
    };
    let (conv_zh_id, _) = recorder.resolve_call_target_with_locale(&target_zh, None, Some("call-zh"), Some("zh-CN")).await.unwrap();
    let conv_zh = service.get(USER_ID, &conv_zh_id).await.unwrap();
    assert_eq!(conv_zh.name, "语音通话", "Chinese provisional voice conversation must default to 语音通话");
    assert_eq!(conv_zh.extra.get("locale").and_then(|v| v.as_str()), Some("zh-CN"));

    // 2. English provisional conversation gets "Voice Call"
    let target_en = nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
        workspace: None,
        model: None,
        preset_id: None,
    };
    let (conv_en_id, _) = recorder.resolve_call_target_with_locale(&target_en, None, Some("call-en"), Some("en-US")).await.unwrap();
    let conv_en = service.get(USER_ID, &conv_en_id).await.unwrap();
    assert_eq!(conv_en.name, "Voice Call", "English provisional voice conversation must default to Voice Call");
    assert_eq!(conv_en.extra.get("locale").and_then(|v| v.as_str()), Some("en-US"));

    // 3. User manual rename before first turn is preserved
    service
        .conversation_repo()
        .update(
            &conv_en_id,
            &nomifun_db::ConversationRowUpdate {
                name: Some("User Renamed Meeting".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let renamed = service.get(USER_ID, &conv_en_id).await.unwrap();
    assert_eq!(renamed.name, "User Renamed Meeting");

    // 4. First valid voice turn on the renamed conversation does not overwrite user's custom title
    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: "call-en".into(),
        target: target_en.clone(),
        conversation_id: conv_en_id.clone(),
        memory_owner: None,
        reserved_at_epoch_ms: nomifun_common::now_ms(),
    }));
    recorder.freeze_session("session-en", Some("call-en")).await.unwrap();
    let mut req_en = VoiceDialogueRequest::scoped("session-en", "turn-en-1", "What is the capital of France?");
    req_en.call_request_id = Some("call-en".into());
    req_en.raw_user_text = Some("What is the capital of France?".into());
    recorder.preflight(&req_en).await.unwrap();
    recorder.record_pair(&conv_en_id, &req_en, r#"{"intent_type":"chat","reply_tts":"The capital of France is Paris."}"#).await.unwrap();

    // Give autotitle task a moment to process claim
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let after_turn = service.get(USER_ID, &conv_en_id).await.unwrap();
    assert_eq!(after_turn.name, "User Renamed Meeting", "User manual rename must never be overwritten by voice autotitle");

    // 5a. Without completer configured, autotitle fails gracefully and preserves default "语音通话"
    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: "call-zh".into(),
        target: target_zh.clone(),
        conversation_id: conv_zh_id.clone(),
        memory_owner: None,
        reserved_at_epoch_ms: nomifun_common::now_ms(),
    }));
    recorder.freeze_session("session-zh", Some("call-zh")).await.unwrap();
    let mut req_zh = VoiceDialogueRequest::scoped("session-zh", "turn-zh-1", "查询明天北京的天气");
    req_zh.call_request_id = Some("call-zh".into());
    req_zh.raw_user_text = Some("查询明天北京的天气".into());
    recorder.preflight(&req_zh).await.unwrap();
    recorder.record_pair(&conv_zh_id, &req_zh, r#"{"intent_type":"chat","reply_tts":"明天北京晴天。"}"#).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let conv_zh_after = service.get(USER_ID, &conv_zh_id).await.unwrap();
    // Default title "语音通话" must be preserved when completer fails / is unavailable
    assert_eq!(conv_zh_after.name, "语音通话", "Default title 语音通话 must be retained when title generation fails");
    assert_eq!(conv_zh_after.extra.get("autoTitleState").and_then(|v| v.as_str()), Some("failed"));

    // 5b. With title completer configured, default voice title is updated to the generated summary title
    service.with_title_completer(Arc::new(TestTitleCompleter));
    let target_en_gen = nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
        workspace: None,
        model: Some(nomifun_api_types::voice::VoiceModelRef {
            provider_id: USER_ID.into(),
            model: "voice-model".into(),
        }),
        preset_id: None,
    };
    let (conv_en_gen_id, _) = recorder.resolve_call_target_with_locale(&target_en_gen, None, Some("call-en-gen"), Some("en-US")).await.unwrap();
    let conv_en_gen_initial = service.get(USER_ID, &conv_en_gen_id).await.unwrap();
    assert_eq!(conv_en_gen_initial.name, "Voice Call");

    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: "call-en-gen".into(),
        target: target_en_gen.clone(),
        conversation_id: conv_en_gen_id.clone(),
        memory_owner: None,
        reserved_at_epoch_ms: nomifun_common::now_ms(),
    }));
    recorder.freeze_session("session-en-gen", Some("call-en-gen")).await.unwrap();
    let mut req_en_gen = VoiceDialogueRequest::scoped("session-en-gen", "turn-en-gen-1", "What is the capital of France?");
    req_en_gen.call_request_id = Some("call-en-gen".into());
    req_en_gen.raw_user_text = Some("What is the capital of France?".into());
    recorder.preflight(&req_en_gen).await.unwrap();
    recorder.record_pair(&conv_en_gen_id, &req_en_gen, r#"{"intent_type":"chat","reply_tts":"The capital of France is Paris."}"#).await.unwrap();

    let conv_en_gen_after = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let conv = service.get(USER_ID, &conv_en_gen_id).await.unwrap();
            if conv.extra.get("autoTitleState").and_then(|v| v.as_str()) == Some("done") {
                break conv;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.expect("title task should complete");
    assert_eq!(conv_en_gen_after.name, "Capital of France", "Generated title must overwrite default Voice Call title");

    // 6. Empty call without user turn keeps default title
    let target_empty = nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
        workspace: None,
        model: None,
        preset_id: None,
    };
    let (conv_empty_id, _) = recorder.resolve_call_target_with_locale(&target_empty, None, Some("call-empty"), Some("en-US")).await.unwrap();
    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: "call-empty".into(),
        target: target_empty,
        conversation_id: conv_empty_id.clone(),
        memory_owner: None,
        reserved_at_epoch_ms: nomifun_common::now_ms(),
    }));
    recorder.freeze_session("session-empty", Some("call-empty")).await.unwrap();
    recorder.finish_call_placeholder(&conv_empty_id, "call-empty").await.unwrap();
    let conv_empty_after = service.get(USER_ID, &conv_empty_id).await.unwrap();
    assert_eq!(conv_empty_after.name, "Voice Call", "Empty call without user turn must keep default title");
}

#[tokio::test]
async fn reviewer_busy_task_control_without_optional_display_should_not_fail_projection() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap(),
    ).await.unwrap();
    let repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let service = Arc::new(ConversationService::new(
        Arc::<str>::from(USER_ID), std::env::temp_dir(), Arc::new(NullSink),
        Arc::new(NoopSkillResolver), Arc::new(EmptyRegistry), repo,
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));
    let conversation_id = service.ensure_default_voice_conversation(USER_ID).await.unwrap();
    let journal = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), journal.path());
    recorder.active_window_candidate.write().unwrap().conversation_id = Some(conversation_id.clone());
    let request = VoiceDialogueRequest::scoped("review-session", "review-turn", "情况怎么样？");
    recorder.preflight(&request).await.unwrap();
    sqlx::query("UPDATE conversations SET active_turn_operation_id = 'review-active-task' WHERE conversation_id = ?")
        .bind(&conversation_id).execute(db.pool()).await.unwrap();
    let decision = r#"{"intent_type":"task_control","task_control_action":"query_status"}"#;
    assert!(nomifun_api_types::voice::parse_voice_decision(decision).is_ok(), "the production wire contract accepts omitted optional display fields");
    let result = recorder.record_pair(&conversation_id, &request, decision).await;
    assert!(result.is_ok(), "a valid status request must not fail solely because it has no precomputed assistant display");
    let task = task_control_snapshot(&conversation_id, "review-session", "review-turn", "真实结果 42");
    let scope = nomifun_api_types::voice::VoiceSessionControlRequest {
        instance_id: "review-instance".into(),
        session_id: Some("review-session".into()),
        turn_id: Some("review-turn".into()),
    };
    recorder.record_task_control_result(
        &scope,
        &nomifun_api_types::voice::VoiceTaskReference::from(&task),
        Some("情况怎么样？"),
        None,
        nomifun_api_types::voice::VoiceTaskControlAction::QueryStatus,
        &Ok(task),
    ).await.unwrap();
    let rows = service.list_messages(USER_ID, &conversation_id, Default::default()).await.unwrap().items;
    let right = rows.iter().filter(|row| row.position == Some(nomifun_common::MessagePosition::Right)).count();
    let left = rows.iter().filter(|row| row.position == Some(nomifun_common::MessagePosition::Left)).count();
    eprintln!("busy task control: user_rows={right}, assistant_rows={left}");
    assert_eq!(right, 1, "user message must be projected");
    assert_eq!(left, 1, "the authoritative task result must be projected exactly once");
    let assistant = rows.iter().find(|row| row.position == Some(nomifun_common::MessagePosition::Left)).unwrap();
    let assistant_text = assistant.content.get("content").and_then(|value| value.as_str()).unwrap_or_default();
    assert!(assistant_text.contains("真实结果 42"), "persisted result must be the real task result, not a placeholder: {}", assistant.content);
}

#[tokio::test]
async fn reviewer_busy_task_control_with_display_is_a_valid_control_case() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap(),
    ).await.unwrap();
    let repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let service = Arc::new(ConversationService::new(
        Arc::<str>::from(USER_ID), std::env::temp_dir(), Arc::new(NullSink),
        Arc::new(NoopSkillResolver), Arc::new(EmptyRegistry), repo,
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));
    let conversation_id = service.ensure_default_voice_conversation(USER_ID).await.unwrap();
    let journal = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), journal.path());
    recorder.active_window_candidate.write().unwrap().conversation_id = Some(conversation_id.clone());
    let request = VoiceDialogueRequest::scoped("review-control-session", "review-control-turn", "情况怎么样？");
    recorder.preflight(&request).await.unwrap();
    sqlx::query("UPDATE conversations SET active_turn_operation_id = 'review-active-task' WHERE conversation_id = ?")
        .bind(&conversation_id).execute(db.pool()).await.unwrap();
    let decision = r#"{"intent_type":"task_control","task_control_action":"query_status","understood_request":"查询任务进度"}"#;
    assert!(nomifun_api_types::voice::parse_voice_decision(decision).is_ok());
    let result = recorder.record_pair(&conversation_id, &request, decision).await;
    assert!(result.is_ok());
    let task = task_control_snapshot(&conversation_id, "review-control-session", "review-control-turn", "完成：3 项");
    let scope = nomifun_api_types::voice::VoiceSessionControlRequest {
        instance_id: "review-instance".into(),
        session_id: Some("review-control-session".into()),
        turn_id: Some("review-control-turn".into()),
    };
    recorder.record_task_control_result(
        &scope,
        &nomifun_api_types::voice::VoiceTaskReference::from(&task),
        Some("情况怎么样？"),
        None,
        nomifun_api_types::voice::VoiceTaskControlAction::QueryStatus,
        &Ok(task),
    ).await.unwrap();
    let rows = service.list_messages(USER_ID, &conversation_id, Default::default()).await.unwrap().items;
    eprintln!("busy control with display: total_rows={}", rows.len());
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.position == Some(nomifun_common::MessagePosition::Left)
        && row.content.get("content").and_then(|value| value.as_str()).is_some_and(|text| text.contains("完成：3 项"))));
}

fn task_control_snapshot(
    conversation_id: &str,
    session_id: &str,
    turn_id: &str,
    result_text: &str,
) -> nomifun_api_types::voice::VoiceTaskSnapshot {
    nomifun_api_types::voice::VoiceTaskSnapshot {
        command_id: format!("command-{turn_id}"),
        session_id: session_id.into(),
        acoustic_turn_id: turn_id.into(),
        conversation_id: Some(conversation_id.into()),
        message_id: Some(format!("message-{turn_id}")),
        runtime_turn_id: None,
        status: nomifun_api_types::voice::VoiceTaskStatus::Completed,
        result_ok: Some(true),
        result_text: Some(result_text.into()),
        error_code: None,
    }
}

async fn bound_task_control_turn(
    recorder: &ConversationVoiceDialogueRecorder,
    request_id: &str,
    session_id: &str,
    turn_id: &str,
    raw_user_text: &str,
) -> (String, VoiceDialogueRequest) {
    let target = nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
        workspace: None, model: None, preset_id: None,
    };
    let (conversation_id, _) = recorder
        .resolve_call_target_with_locale(&target, None, Some(request_id), Some("zh-CN"))
        .await.unwrap();
    recorder.set_call_reservation(Some(VoiceCallReservation {
        request_id: request_id.into(),
        target,
        conversation_id: conversation_id.clone(),
        memory_owner: None,
        reserved_at_epoch_ms: nomifun_common::now_ms(),
    }));
    recorder.freeze_session(session_id, Some(request_id)).await.unwrap();
    let mut request = VoiceDialogueRequest::scoped(session_id, turn_id, raw_user_text);
    request.call_request_id = Some(request_id.into());
    recorder.preflight(&request).await.unwrap();
    let decision = r#"{"intent_type":"task_control","task_control_action":"query_status"}"#;
    recorder.record_pair(&conversation_id, &request, decision).await.unwrap();
    (conversation_id, request)
}

fn messages_of(rows: &[nomifun_api_types::MessageResponse], position: nomifun_common::MessagePosition) -> Vec<String> {
    rows.iter()
        .filter(|row| row.position == Some(position))
        .filter_map(|row| row.content.get("content").and_then(|value| value.as_str()).map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn task_control_results_are_scoped_persisted_and_idempotent_across_restart() {
    use nomifun_api_types::voice::{
        VoiceSessionControlRequest, VoiceTaskControlAction, VoiceTaskReference,
    };
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap(),
    ).await.unwrap();
    let repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let service = Arc::new(ConversationService::new(
        Arc::<str>::from(USER_ID), std::env::temp_dir(), Arc::new(NullSink),
        Arc::new(NoopSkillResolver), Arc::new(EmptyRegistry), repo,
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));
    let journal = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), journal.path());

    let (conv_a, _request_a) = bound_task_control_turn(&recorder, "call-a", "session-a", "turn-a", "任务进度？").await;
    let (conv_b, request_b) = bound_task_control_turn(&recorder, "call-b", "session-b", "turn-b", "任务进度？").await;
    assert_ne!(conv_a, conv_b);

    let task_a = task_control_snapshot(&conv_a, "session-a", "origin-a", "A 的真实结果");
    let task_b = task_control_snapshot(&conv_b, "session-b", "origin-b", "B 的真实结果");
    let scope_a = VoiceSessionControlRequest { instance_id: "instance-a".into(), session_id: Some("session-a".into()), turn_id: Some("turn-a".into()) };
    let scope_b = VoiceSessionControlRequest { instance_id: "instance-b".into(), session_id: Some("session-b".into()), turn_id: Some("turn-b".into()) };

    // The user question is persisted before any control result exists.
    let rows_a = service.list_messages(USER_ID, &conv_a, Default::default()).await.unwrap().items;
    let rows_b = service.list_messages(USER_ID, &conv_b, Default::default()).await.unwrap().items;
    assert_eq!(messages_of(&rows_a, nomifun_common::MessagePosition::Right), vec!["任务进度？"]);
    assert!(messages_of(&rows_a, nomifun_common::MessagePosition::Left).is_empty());
    assert_eq!(messages_of(&rows_b, nomifun_common::MessagePosition::Right), vec!["任务进度？"]);

    // A's task must not be persisted into B's turn, even before B has a result.
    let cross = recorder.record_task_control_result(
        &scope_b, &VoiceTaskReference::from(&task_a), Some("任务进度？"), None,
        VoiceTaskControlAction::QueryStatus, &Ok(task_a.clone()),
    ).await;
    assert!(matches!(cross, Err(VoiceDialogueRecorderError::BindingInvalid(ref code)) if code == "task_scope_changed"));
    let rows_b = service.list_messages(USER_ID, &conv_b, Default::default()).await.unwrap().items;
    assert!(messages_of(&rows_b, nomifun_common::MessagePosition::Left).is_empty());

    // An unknown result keeps the question and identity without fabricating a reply.
    let unknown = recorder.record_task_control_result(
        &scope_b, &VoiceTaskReference::from(&task_b), Some("任务进度？"), None,
        VoiceTaskControlAction::QueryStatus, &Err("task_status_unavailable".into()),
    ).await;
    assert!(unknown.is_ok());
    assert_eq!(
        recorder.turn_error("session-b", &conv_b, "turn-b").as_ref().map(|error| error.code.as_str()),
        Some("task_status_unavailable"),
    );
    let rows_b = service.list_messages(USER_ID, &conv_b, Default::default()).await.unwrap().items;
    assert_eq!(messages_of(&rows_b, nomifun_common::MessagePosition::Right), vec!["任务进度？"]);
    assert!(messages_of(&rows_b, nomifun_common::MessagePosition::Left).is_empty());

    // Interleaved authoritative results stay in their own conversations.
    recorder.record_task_control_result(
        &scope_a, &VoiceTaskReference::from(&task_a), Some("任务进度？"), None,
        VoiceTaskControlAction::QueryStatus, &Ok(task_a.clone()),
    ).await.unwrap();
    recorder.record_task_control_result(
        &scope_b, &VoiceTaskReference::from(&task_b), Some("任务进度？"), None,
        VoiceTaskControlAction::QueryStatus, &Ok(task_b.clone()),
    ).await.unwrap();
    assert!(recorder.turn_error("session-b", &conv_b, "turn-b").is_none());
    let rows_a = service.list_messages(USER_ID, &conv_a, Default::default()).await.unwrap().items;
    let rows_b = service.list_messages(USER_ID, &conv_b, Default::default()).await.unwrap().items;
    let left_a = messages_of(&rows_a, nomifun_common::MessagePosition::Left);
    let left_b = messages_of(&rows_b, nomifun_common::MessagePosition::Left);
    assert_eq!(left_a.len(), 1);
    assert_eq!(left_b.len(), 1);
    assert!(left_a[0].contains("A 的真实结果") && !left_a[0].contains("B 的真实结果"));
    assert!(left_b[0].contains("B 的真实结果") && !left_b[0].contains("A 的真实结果"));

    // Duplicate delivery is idempotent for both the receipt and message rows.
    recorder.record_task_control_result(
        &scope_a, &VoiceTaskReference::from(&task_a), Some("任务进度？"), None,
        VoiceTaskControlAction::QueryStatus, &Ok(task_a.clone()),
    ).await.unwrap();
    let rows_a_again = service.list_messages(USER_ID, &conv_a, Default::default()).await.unwrap().items;
    assert_eq!(rows_a_again.len(), rows_a.len());
    assert_eq!(messages_of(&rows_a_again, nomifun_common::MessagePosition::Right).len(), 1);

    // A fresh recorder sees the same durable journal and can answer from the cached projection.
    let restarted = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), journal.path());
    let cached = restarted.preflight(&request_b).await.unwrap();
    assert!(matches!(cached, VoiceDialoguePreflight::Cached { ref content } if content.contains("B 的真实结果")));

    // A changed admission epoch is a real write guard failure; no fabricated result is added.
    let (conv_d, _request_d) = bound_task_control_turn(&recorder, "call-d", "session-d", "turn-d", "任务进度？").await;
    let task_d = task_control_snapshot(&conv_d, "session-d", "origin-d", "D 的真实结果");
    let scope_d = VoiceSessionControlRequest { instance_id: "instance-d".into(), session_id: Some("session-d".into()), turn_id: Some("turn-d".into()) };
    sqlx::query("UPDATE conversations SET admission_epoch = admission_epoch + 1 WHERE conversation_id = ?")
        .bind(&conv_d).execute(db.pool()).await.unwrap();
    let failed = recorder.record_task_control_result(
        &scope_d, &VoiceTaskReference::from(&task_d), Some("任务进度？"), None,
        VoiceTaskControlAction::QueryStatus, &Ok(task_d),
    ).await;
    assert!(matches!(failed, Err(VoiceDialogueRecorderError::BindingInvalid(ref code)) if code == "target_revision_changed"));
    let rows_d = service.list_messages(USER_ID, &conv_d, Default::default()).await.unwrap().items;
    assert_eq!(messages_of(&rows_d, nomifun_common::MessagePosition::Right), vec!["任务进度？"]);
    assert!(messages_of(&rows_d, nomifun_common::MessagePosition::Left).is_empty());
}

#[tokio::test]
async fn task_control_defers_display_until_the_real_task_result() {
    let db = nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).unwrap(),
    ).await.unwrap();
    let repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let service = Arc::new(ConversationService::new(
        Arc::<str>::from(USER_ID), std::env::temp_dir(), Arc::new(NullSink),
        Arc::new(NoopSkillResolver), Arc::new(EmptyRegistry), repo,
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),
        Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
    ));
    let conversation_id = service.ensure_default_voice_conversation(USER_ID).await.unwrap();
    let journal = tempfile::tempdir().unwrap();
    let recorder = ConversationVoiceDialogueRecorder::new(service.clone(), USER_ID.into(), journal.path());
    recorder.active_window_candidate.write().unwrap().conversation_id = Some(conversation_id.clone());
    let request = VoiceDialogueRequest::scoped("defer-session", "defer-turn", "任务进度怎么样？");
    recorder.preflight(&request).await.unwrap();
    sqlx::query("UPDATE conversations SET active_turn_operation_id = 'defer-active-task' WHERE conversation_id = ?")
        .bind(&conversation_id).execute(db.pool()).await.unwrap();
    let decision = r#"{"intent_type":"task_control","task_control_action":"query_status"}"#;
    let result = recorder.record_pair(&conversation_id, &request, decision).await;
    assert!(result.is_ok(), "the user question must be accepted without a fabricated display");
    let rows = service.list_messages(USER_ID, &conversation_id, Default::default()).await.unwrap().items;
    let left = rows.iter().filter(|row| row.position == Some(nomifun_common::MessagePosition::Left)).count();
    let right = rows.iter().filter(|row| row.position == Some(nomifun_common::MessagePosition::Right)).count();
    assert_eq!(right, 1, "the user question must be persisted");
    assert_eq!(left, 0, "no assistant placeholder may be persisted before the real task result");
}

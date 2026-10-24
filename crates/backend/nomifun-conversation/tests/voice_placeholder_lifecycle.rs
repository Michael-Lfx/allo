#[cfg(test)]
mod independent {
    use std::sync::Arc;
    use async_trait::async_trait;
    use nomifun_ai_agent::{AgentRuntimeRegistry, runtime_handle::AgentRuntimeHandle, types::AgentRuntimeBuildOptions};
    use nomifun_common::{AgentKillReason, AppError, TimestampMs};
    use nomifun_conversation::{ConversationService, skill_resolver::{SkillResolver, ResolvedAgentSkill}};
    use nomifun_db::{SqliteConversationRepository, SqliteAgentMetadataRepository, SqliteAcpSessionRepository};
    use nomifun_realtime::UserEventSink;

    const OWNER: &str = "0190f5fe-7c00-7a00-8000-000000000001";
    struct Sink;
    impl UserEventSink for Sink { fn send_to_user(&self, _: &str, _: nomifun_api_types::WebSocketMessage<serde_json::Value>) {} }
    struct Skills;
    #[async_trait]
    impl SkillResolver for Skills {
        async fn auto_inject_names(&self) -> Vec<String> { vec![] }
        async fn resolve_skills(&self, _: &[String]) -> Vec<ResolvedAgentSkill> { vec![] }
        async fn link_workspace_skills(&self, _: &std::path::Path, _: &[&str], _: &[ResolvedAgentSkill]) -> usize { 0 }
    }
    struct Runtime;
    #[async_trait]
    impl AgentRuntimeRegistry for Runtime {
        fn get_runtime(&self, _: &str) -> Option<AgentRuntimeHandle> { None }
        async fn get_or_create_runtime(&self, _: &str, _: AgentRuntimeBuildOptions) -> Result<AgentRuntimeHandle,AppError> { Err(AppError::Internal("No model execution in these persistence probes".into())) }
        fn terminate(&self, _: &str, _: Option<AgentKillReason>) -> Result<(),AppError> { Ok(()) }
        fn terminate_all(&self) {}
        fn active_runtime_count(&self) -> usize { 0 }
        fn collect_idle_runtimes(&self, _: TimestampMs) -> Vec<String> { vec![] }
    }
    async fn service() -> Arc<ConversationService> {
        let db = nomifun_db::init_database_memory_with_owner(nomifun_common::UserId::parse(OWNER.to_owned()).unwrap()).await.unwrap();
        Arc::new(ConversationService::new(Arc::<str>::from(OWNER),std::env::temp_dir(),Arc::new(Sink),Arc::new(Skills),Arc::new(Runtime),Arc::new(SqliteConversationRepository::new(db.pool().clone())),Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone())),Arc::new(SqliteAcpSessionRepository::new(db.pool().clone())),Arc::new(nomifun_conversation::NoExecutionConversationBoundary)))
    }
    #[tokio::test]
    async fn review_project_new_call_must_succeed() {
        let svc=service().await;
        let dir=tempfile::tempdir().unwrap();
        let result=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),Some(dir.path().to_str().unwrap()),"project-probe",None,None).await;
        assert!(result.is_ok(),"LC-01: real project creation failed: {result:?}");
    }
    #[tokio::test]
    async fn review_discarded_call_rejects_late_append() {
        let svc=service().await;
        let conv=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"late-input-probe",None,None).await.unwrap();
        assert!(svc.discard_provisional_voice_conversation(OWNER,&conv.conversation_id).await.unwrap());
        let result=svc.append_voice_message(OWNER,&conv.conversation_id,&nomifun_common::MessageId::new().into_string(),"This arrived after the placeholder was discarded","late-command").await;
        let messages=svc.list_messages(OWNER,&conv.conversation_id,Default::default()).await.unwrap();
        assert!(result.is_err(),"LC-09: discarded call accepted late append, persisted messages={}",messages.items.len());
    }
    #[tokio::test]
    async fn review_receipt_lookup_is_read_only() {
        let svc=service().await;
        let conv=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"lookup-probe",None,None).await.unwrap();
        assert!(svc.latest_completed_turn_receipt(OWNER,&conv.conversation_id).await.unwrap().is_none());
        let after=svc.get(OWNER,&conv.conversation_id).await.unwrap();
        assert_eq!(after.extra["voice_lifecycle"],"provisional","read-only recovery must not retain an unused draft");
    }
    #[tokio::test]
    async fn review_user_rename_prevents_discard() {
        let svc=service().await;
        let conv=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"rename-probe",None,None).await.unwrap();
        let registry:Arc<dyn AgentRuntimeRegistry>=Arc::new(Runtime);
        svc.update(OWNER,&conv.conversation_id,serde_json::from_value(serde_json::json!({"name":"My deliberately saved call"})).unwrap(),&registry).await.unwrap();
        assert!(!svc.discard_provisional_voice_conversation(OWNER,&conv.conversation_id).await.unwrap(),"LC-07: user-renamed call was discarded");
    }
    #[tokio::test]
    async fn review_persistent_request_rejects_changed_mode() {
        let svc=service().await;
        let original=svc.create_provisional_voice_conversation(OWNER,None,Some("default"),None,"frozen-mode-probe",None,None).await.unwrap();
        let replay=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"frozen-mode-probe",None,None).await;
        assert!(replay.is_err(),"LC-02/10: durable replay silently accepted changed mode; original={:?}, replay={replay:?}",original.extra["session_mode"]);
    }
    #[tokio::test]
    async fn review_public_patch_cannot_reopen_a_discarded_identity() {
        let svc=service().await;
        let conv=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"patch-probe",None,None).await.unwrap();
        assert!(svc.discard_provisional_voice_conversation(OWNER,&conv.conversation_id).await.unwrap());
        let registry:Arc<dyn AgentRuntimeRegistry>=Arc::new(Runtime);
        let result=svc.update(OWNER,&conv.conversation_id,serde_json::from_value(serde_json::json!({"extra":{"voice_lifecycle":"provisional"}})).unwrap(),&registry).await;
        assert!(result.is_err(),"LC-10: public PATCH reopened backend terminal identity: {result:?}");
    }
    #[tokio::test]
    async fn review_control_new_request_distinct_and_same_request_stable() {
        let svc=service().await;
        let first=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"positive-one",None,None).await.unwrap();
        let replay=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"positive-one",None,None).await.unwrap();
        let second=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"positive-two",None,None).await.unwrap();
        assert_eq!(first.conversation_id,replay.conversation_id);
        assert_ne!(first.conversation_id,second.conversation_id);
    }
    #[tokio::test]
    async fn review_control_ordinary_history_is_not_discarded() {
        let svc=service().await;
        let conv=svc.create(OWNER,serde_json::from_value(serde_json::json!({"type":"nomi","name":"ordinary","extra":{}})).unwrap()).await.unwrap();
        assert!(!svc.discard_provisional_voice_conversation(OWNER,&conv.conversation_id).await.unwrap());
    }
    #[tokio::test]
    async fn voice_placeholder_close_races_with_real_message_admission() {
        let svc=service().await;
        for index in 0..24 {
            let conv=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,&format!("race-{index}"),None,None).await.unwrap();
            let message=nomifun_common::MessageId::new().into_string();
            let (closed,accepted)=tokio::join!(
                svc.discard_provisional_voice_conversation(OWNER,&conv.conversation_id),
                svc.append_voice_message(OWNER,&conv.conversation_id,&message,"never lose accepted input","race-command")
            );
            let row=svc.get(OWNER,&conv.conversation_id).await.unwrap();
            let messages=svc.list_messages(OWNER,&conv.conversation_id,Default::default()).await.unwrap();
            if accepted.is_ok() {
                assert!(!closed.unwrap()); assert_eq!(row.extra["voice_lifecycle"],"retained"); assert_eq!(messages.items.len(),1);
            } else {
                assert!(closed.unwrap()); assert_eq!(row.extra["voice_lifecycle"],"discarded"); assert!(messages.items.is_empty());
            }
        }
    }
    #[tokio::test]
    async fn voice_placeholder_ended_request_is_durable_and_public_fields_are_reserved() {
        let svc=service().await;
        let conv=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"ended-request",None,None).await.unwrap();
        svc.append_voice_message(OWNER,&conv.conversation_id,&nomifun_common::MessageId::new().into_string(),"keep history","ended-command").await.unwrap();
        svc.finish_provisional_voice_conversation(OWNER,&conv.conversation_id,false).await.unwrap();
        assert!(svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"ended-request",None,None).await.is_err());
        assert_eq!(svc.get(OWNER,&conv.conversation_id).await.unwrap().extra["voice_lifecycle"],"retained");
        for key in ["voice_lifecycle","voice_creation_key","voice_creation_request"] {
            let mut extra=serde_json::json!({});extra[key]=serde_json::json!("injected");
            assert!(svc.create(OWNER,serde_json::from_value(serde_json::json!({"type":"nomi","extra":extra})).unwrap()).await.is_err());
        }
    }
    #[tokio::test]
    async fn voice_call_delete_guard_protects_the_shared_delete_path() {
        let svc=service().await;
        let conv=svc.create_provisional_voice_conversation(OWNER,None,Some("yolo"),None,"delete-guard",None,None).await.unwrap();
        let id=conv.conversation_id.clone();
        let active=Arc::new(std::sync::atomic::AtomicBool::new(true));
        let guard_active=active.clone();
        svc.set_voice_call_delete_guard(Arc::new(move |candidate| candidate==id && guard_active.load(std::sync::atomic::Ordering::SeqCst)));
        assert!(matches!(svc.delete(OWNER,&conv.conversation_id).await, Err(AppError::Conflict(code)) if code == "voice_call_active"));
        active.store(false,std::sync::atomic::Ordering::SeqCst);
        // This fixture deliberately has no execution-deletion boundary. Once
        // inactive, deletion must reach that normal boundary instead of the call guard.
        assert!(matches!(svc.delete(OWNER,&conv.conversation_id).await, Err(AppError::ConversationDelete(_))));
    }
}

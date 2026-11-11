//! Production [`TurnTerminalProofProvider`]: the boot-scoped side of the
//! persisted exact terminal-proof protocol.
//!
//! Constructed once per boot, only after
//! `AppServices::has_valid_boot_reconciliation_authority` has revalidated the
//! exclusive data-dir server lock. Holding that lock proves the previous
//! backend process released it (it holds the lock handle for its entire
//! lifetime), so every turn generation frozen in the boot snapshot below was
//! admitted by a process that no longer exists. What the lock does NOT prove
//! is that the dead process's descendant tree is empty — that half of the
//! proof comes from [`nomifun_ai_agent::reap_orphan_agent_processes`], which
//! verifies every durable agent-process registry entry by exact identity and
//! kills-with-proof any survivor before this provider is built.
//!
//! The snapshot freeze is the anti-ABA fence: a generation is only provable
//! if its exact `(admission_epoch, active_operation_id)` was already
//! unsettled when this process booted. Anything newer was admitted by the
//! current process and has a live owner protocol of its own.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use nomifun_ai_agent::AgentProcessReapReport;
use nomifun_conversation::terminal_proof::{
    OrphanProofRequirement, TerminalProofDecision, TurnTerminalProofProvider,
};

/// One frozen unsettled generation from the boot enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrozenOrphanGeneration {
    pub user_id: String,
    pub admission_epoch: i64,
    pub active_operation_id: Option<String>,
}

pub(crate) struct BootTerminalProofProvider {
    /// conversation_id -> the exact generation observed before any work
    /// producer started. Only these generations may ever prove.
    frozen: HashMap<String, FrozenOrphanGeneration>,
    reap_report: AgentProcessReapReport,
}

impl BootTerminalProofProvider {
    pub(crate) fn new(
        frozen: HashMap<String, FrozenOrphanGeneration>,
        reap_report: AgentProcessReapReport,
    ) -> Arc<Self> {
        Arc::new(Self {
            frozen,
            reap_report,
        })
    }
}

#[async_trait]
impl TurnTerminalProofProvider for BootTerminalProofProvider {
    async fn prove_orphan_generation_terminal(
        &self,
        user_id: &str,
        conversation_id: &str,
        requirement: OrphanProofRequirement,
        admission_epoch: i64,
        active_operation_id: Option<&str>,
    ) -> TerminalProofDecision {
        let Some(frozen) = self.frozen.get(conversation_id) else {
            return TerminalProofDecision::Unproven {
                reason: "generation was not unsettled at boot; it belongs to the current process"
                    .to_owned(),
            };
        };
        if frozen.user_id != user_id
            || frozen.admission_epoch != admission_epoch
            || frozen.active_operation_id.as_deref() != active_operation_id
        {
            return TerminalProofDecision::Unproven {
                reason: "generation no longer matches the boot-frozen unsettled snapshot"
                    .to_owned(),
            };
        }
        if !self.reap_report.registry_fully_processed() {
            return TerminalProofDecision::Unproven {
                reason: "durable agent-process registry could not be fully reaped".to_owned(),
            };
        }

        match requirement {
            // In-process owner died with the lock holder; contained children
            // died with their Job/watchdog authorities. A surviving durable
            // registry entry for this Conversation would contradict that
            // audit, so require none.
            OrphanProofRequirement::LocalContainedAuthority
            | OrphanProofRequirement::RegisteredLocalProcessTree => {
                if self
                    .reap_report
                    .conversation_tree_proven_empty(conversation_id)
                {
                    TerminalProofDecision::Proven {
                        evidence: "server-lock authority + boot-frozen generation + reaped durable process registry (no surviving entry)"
                            .to_owned(),
                    }
                } else {
                    TerminalProofDecision::Unproven {
                        reason: "durable process registry retains an unproven entry for this Conversation"
                            .to_owned(),
                    }
                }
            }
            // A gateway is registered only when self-spawned; absence could
            // mean an external/attached gateway still hosts the work.
            OrphanProofRequirement::RegisteredGatewayAuthority => {
                if self.reap_report.conversation_had_entries(conversation_id)
                    && self
                        .reap_report
                        .conversation_tree_proven_empty(conversation_id)
                {
                    TerminalProofDecision::Proven {
                        evidence: "server-lock authority + boot-frozen generation + identity-verified reaping of the self-spawned gateway"
                            .to_owned(),
                    }
                } else {
                    TerminalProofDecision::Unproven {
                        reason: "gateway process was not registry-owned by this data dir; external work cannot be proven terminal locally"
                            .to_owned(),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_ai_agent::reap_orphan_agent_processes;

    const USER: &str = "user-1";
    const CONV: &str = "conv-1";

    fn frozen_map(epoch: i64, operation: Option<&str>) -> HashMap<String, FrozenOrphanGeneration> {
        HashMap::from([(
            CONV.to_owned(),
            FrozenOrphanGeneration {
                user_id: USER.to_owned(),
                admission_epoch: epoch,
                active_operation_id: operation.map(str::to_owned),
            },
        )])
    }

    /// A fully-processed empty reap report (no registry file in the dir).
    async fn empty_reap_report() -> nomifun_ai_agent::AgentProcessReapReport {
        let dir = tempfile::tempdir().unwrap();
        reap_orphan_agent_processes(dir.path()).await
    }

    #[tokio::test]
    async fn frozen_generation_with_empty_registry_proves_local_backends() {
        let provider =
            BootTerminalProofProvider::new(frozen_map(3, Some("op-a")), empty_reap_report().await);
        for requirement in [
            OrphanProofRequirement::LocalContainedAuthority,
            OrphanProofRequirement::RegisteredLocalProcessTree,
        ] {
            let decision = provider
                .prove_orphan_generation_terminal(USER, CONV, requirement, 3, Some("op-a"))
                .await;
            assert!(
                matches!(decision, TerminalProofDecision::Proven { .. }),
                "{requirement:?}: {decision:?}"
            );
        }
    }

    #[tokio::test]
    async fn generation_outside_the_frozen_snapshot_never_proves() {
        let provider =
            BootTerminalProofProvider::new(frozen_map(3, Some("op-a")), empty_reap_report().await);
        for (epoch, operation) in [
            (4, Some("op-a")),
            (3, Some("op-b")),
            (3, None),
        ] {
            let decision = provider
                .prove_orphan_generation_terminal(
                    USER,
                    CONV,
                    OrphanProofRequirement::LocalContainedAuthority,
                    epoch,
                    operation,
                )
                .await;
            assert!(
                matches!(decision, TerminalProofDecision::Unproven { .. }),
                "epoch {epoch} op {operation:?}: {decision:?}"
            );
        }
        let unknown_conversation = provider
            .prove_orphan_generation_terminal(
                USER,
                "conv-other",
                OrphanProofRequirement::LocalContainedAuthority,
                0,
                None,
            )
            .await;
        assert!(matches!(
            unknown_conversation,
            TerminalProofDecision::Unproven { .. }
        ));
        let wrong_user = provider
            .prove_orphan_generation_terminal(
                "user-2",
                CONV,
                OrphanProofRequirement::LocalContainedAuthority,
                3,
                Some("op-a"),
            )
            .await;
        assert!(matches!(wrong_user, TerminalProofDecision::Unproven { .. }));
    }

    #[tokio::test]
    async fn gateway_requirement_treats_registry_absence_as_unproven() {
        let provider =
            BootTerminalProofProvider::new(frozen_map(1, Some("op-a")), empty_reap_report().await);
        let decision = provider
            .prove_orphan_generation_terminal(
                USER,
                CONV,
                OrphanProofRequirement::RegisteredGatewayAuthority,
                1,
                Some("op-a"),
            )
            .await;
        assert!(
            matches!(decision, TerminalProofDecision::Unproven { .. }),
            "a gateway with no registry entry may be external: {decision:?}"
        );
    }

    #[tokio::test]
    async fn unprocessed_registry_report_proves_nothing() {
        let provider = BootTerminalProofProvider::new(
            frozen_map(1, Some("op-a")),
            nomifun_ai_agent::AgentProcessReapReport::default(),
        );
        let decision = provider
            .prove_orphan_generation_terminal(
                USER,
                CONV,
                OrphanProofRequirement::LocalContainedAuthority,
                1,
                Some("op-a"),
            )
            .await;
        assert!(matches!(decision, TerminalProofDecision::Unproven { .. }));
    }

    // ------------------------------------------------------------------
    // Real durable-registry integration for the boot proof provider.
    // ------------------------------------------------------------------

    use nomifun_ai_agent::{
        AgentRuntimeRegistry, InMemoryAgentRuntimeRegistry,
    };
    use nomifun_api_types::WebSocketMessage;
    use nomifun_common::{
        AppError, ConversationId, MessageId, UserId, now_ms,
    };
    use nomifun_conversation::service::QuiescentOrphanReconciliation;
    use nomifun_conversation::skill_resolver::{ResolvedAgentSkill, SkillResolver};
    use nomifun_conversation::{ConversationService, NoExecutionConversationBoundary};
    use nomifun_db::models::{ConversationRow, MessageRow};
    use nomifun_db::{
        CreateAcpSessionParams, Database, IAcpSessionRepository, IConversationRepository,
        SortOrder, SqliteAcpSessionRepository, SqliteAgentMetadataRepository,
        SqliteConversationRepository, init_database_memory_with_owner,
    };
    use nomifun_realtime::UserEventSink;

    const OWNER: &str = "0190f5fe-7c00-7a00-8000-000000000001";

    struct NullSink;
    impl UserEventSink for NullSink {
        fn send_to_user(&self, _: &str, _: WebSocketMessage<serde_json::Value>) {}
    }

    struct NoSkills;
    #[async_trait::async_trait]
    impl SkillResolver for NoSkills {
        async fn auto_inject_names(&self) -> Vec<String> {
            Vec::new()
        }
        async fn resolve_skills(&self, _: &[String]) -> Vec<ResolvedAgentSkill> {
            Vec::new()
        }
        async fn link_workspace_skills(
            &self,
            _: &std::path::Path,
            _: &[&str],
            _: &[ResolvedAgentSkill],
        ) -> usize {
            0
        }
    }

    struct RealProviderFixture {
        service: ConversationService,
        repo: Arc<dyn IConversationRepository>,
        acp_repo: Arc<dyn IAcpSessionRepository>,
        runtime_registry: Arc<dyn AgentRuntimeRegistry>,
        conversation_id: String,
        operation_id: String,
        admitted_epoch: i64,
    }

    async fn fresh_db() -> Database {
        init_database_memory_with_owner(UserId::parse(OWNER.to_owned()).unwrap())
            .await
            .unwrap()
    }

    async fn real_provider_fixture(db: &Database, agent_type: &str) -> RealProviderFixture {
        let repo: Arc<dyn IConversationRepository> =
            Arc::new(SqliteConversationRepository::new(db.pool().clone()));
        let acp_repo: Arc<dyn IAcpSessionRepository> =
            Arc::new(SqliteAcpSessionRepository::new(db.pool().clone()));
        let agent_metadata: Arc<dyn nomifun_db::IAgentMetadataRepository> =
            Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone()));
        let runtime_registry: Arc<dyn AgentRuntimeRegistry> = Arc::new(
            InMemoryAgentRuntimeRegistry::new(Arc::new(|_options| {
                Box::pin(async {
                    Err(AppError::Internal(
                        "boot recovery must not build a runtime".into(),
                    ))
                })
            })),
        );
        let service = ConversationService::new(
            Arc::<str>::from(OWNER),
            std::env::temp_dir(),
            Arc::new(NullSink),
            Arc::new(NoSkills),
            runtime_registry.clone(),
            repo.clone(),
            agent_metadata,
            acp_repo.clone(),
            Arc::new(NoExecutionConversationBoundary),
        );

        let now = now_ms();
        let row = ConversationRow {
            id: 0,
            conversation_id: ConversationId::new().into_string(),
            user_id: OWNER.to_owned(),
            name: "Restart orphan".to_owned(),
            r#type: agent_type.to_owned(),
            extra: "{}".to_owned(),
            delegation_policy: "automatic".to_owned(),
            execution_model_pool: None,
            decision_policy: "automatic".to_owned(),
            execution_template_id: None,
            model: None,
            status: Some("pending".to_owned()),
            source: Some("nomifun".to_owned()),
            channel_chat_id: None,
            pinned: false,
            pinned_at: None,
            cron_job_id: None,
            preset_id: None,
            preset_revision: None,
            preset_snapshot: None,
            created_at: now,
            updated_at: now,
        };
        let conversation_id = row.conversation_id.clone();
        repo.create(&row).await.unwrap();
        let initial = repo
            .get_turn_admission_state(OWNER, &conversation_id)
            .await
            .unwrap();
        let operation_id =
            format!("public-turn:v1:{OWNER}:{conversation_id}:restart-orphan");
        let candidate_message_id = MessageId::new().into_string();
        let claim = repo
            .claim_turn_delivery_receipt_and_admit_with_candidate(
                OWNER,
                &conversation_id,
                &operation_id,
                &candidate_message_id,
                r#"{"content":"orphan"}"#,
                initial.epoch,
                now_ms(),
            )
            .await
            .unwrap();
        assert!(claim.claimed_new);
        let admitted = repo
            .get_turn_admission_state(OWNER, &conversation_id)
            .await
            .unwrap();
        assert_eq!(admitted.active_operation_id.as_deref(), Some(operation_id.as_str()));

        repo.insert_message(&MessageRow {
            id: 0,
            message_id: candidate_message_id.clone(),
            conversation_id: conversation_id.clone(),
            msg_id: None,
            r#type: "text".to_owned(),
            content: r#"{"content":"orphan"}"#.to_owned(),
            position: Some("right".to_owned()),
            status: None,
            hidden: false,
            created_at: now_ms(),
        })
        .await
        .unwrap();
        repo.insert_message(&MessageRow {
            id: 0,
            message_id: MessageId::new().into_string(),
            conversation_id: conversation_id.clone(),
            msg_id: Some(candidate_message_id),
            r#type: "thinking".to_owned(),
            content: r#"{"content":"still reasoning","status":"streaming"}"#.to_owned(),
            position: Some("left".to_owned()),
            status: Some("work".to_owned()),
            hidden: false,
            created_at: now_ms(),
        })
        .await
        .unwrap();
        acp_repo
            .create(&CreateAcpSessionParams {
                conversation_id: &conversation_id,
                agent_backend: "gemini",
                agent_source: "builtin",
                agent_id: "",
            })
            .await
            .unwrap();
        acp_repo
            .update_session_id(&conversation_id, "sess-restart-orphan")
            .await
            .unwrap();

        RealProviderFixture {
            service,
            repo,
            acp_repo,
            runtime_registry,
            conversation_id,
            operation_id,
            admitted_epoch: admitted.epoch,
        }
    }

    fn frozen(
        conversation_id: &str,
        epoch: i64,
        operation_id: Option<&str>,
    ) -> HashMap<String, FrozenOrphanGeneration> {
        HashMap::from([(
            conversation_id.to_owned(),
            FrozenOrphanGeneration {
                user_id: OWNER.to_owned(),
                admission_epoch: epoch,
                active_operation_id: operation_id.map(str::to_owned),
            },
        )])
    }

    fn write_registry(data_dir: &std::path::Path, contents: &str) {
        std::fs::create_dir_all(data_dir).unwrap();
        std::fs::write(data_dir.join("agent-process-registry.json"), contents).unwrap();
    }

    async fn assert_quarantined(fixture: &RealProviderFixture) {
        let row = fixture.repo.get(&fixture.conversation_id).await.unwrap().unwrap();
        assert_eq!(row.status.as_deref(), Some("running"));
        let admission = fixture
            .repo
            .get_turn_admission_state(OWNER, &fixture.conversation_id)
            .await
            .unwrap();
        assert_eq!(admission.epoch, fixture.admitted_epoch);
        assert_eq!(
            admission.active_operation_id.as_deref(),
            Some(fixture.operation_id.as_str())
        );
        let receipt = fixture
            .repo
            .get_delivery_receipt(OWNER, &fixture.conversation_id, &fixture.operation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(receipt.status, "accepted");
        assert_eq!(receipt.result_ok, None);
        assert_eq!(
            fixture
                .acp_repo
                .get(&fixture.conversation_id)
                .await
                .unwrap()
                .unwrap()
                .acp_session_id
                .as_deref(),
            Some("sess-restart-orphan")
        );
        let messages = fixture
            .repo
            .get_messages(&fixture.conversation_id, 1, 50, SortOrder::Asc)
            .await
            .unwrap()
            .items;
        let thinking = messages.iter().find(|m| m.r#type == "thinking").unwrap();
        let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
        assert_eq!(content["status"], "streaming");
        assert_eq!(thinking.status.as_deref(), Some("work"));
    }

    #[tokio::test]
    async fn real_empty_registry_proves_and_settles_a_local_orphan() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = fresh_db().await;
        let fixture = real_provider_fixture(&db, "nomi").await;
        let reap_report = reap_orphan_agent_processes(data_dir.path()).await;
        assert!(reap_report.registry_fully_processed());

        let provider = BootTerminalProofProvider::new(
            frozen(
                &fixture.conversation_id,
                fixture.admitted_epoch,
                Some(&fixture.operation_id),
            ),
            reap_report,
        );
        fixture.service.with_terminal_proof_provider(provider);

        assert_eq!(
            fixture
                .service
                .reconcile_locally_quiescent_orphan_on_boot(
                    OWNER,
                    &fixture.conversation_id,
                    &fixture.runtime_registry,
                )
                .await
                .unwrap(),
            QuiescentOrphanReconciliation::Reconciled
        );
        let row = fixture.repo.get(&fixture.conversation_id).await.unwrap().unwrap();
        assert_eq!(row.status.as_deref(), Some("finished"));
        assert_eq!(
            fixture
                .acp_repo
                .get(&fixture.conversation_id)
                .await
                .unwrap()
                .unwrap()
                .acp_session_id,
            None
        );
        let messages = fixture
            .repo
            .get_messages(&fixture.conversation_id, 1, 50, SortOrder::Asc)
            .await
            .unwrap()
            .items;
        let thinking = messages.iter().find(|m| m.r#type == "thinking").unwrap();
        let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
        assert_eq!(content["status"], "done");
    }

    #[tokio::test]
    async fn real_unreadable_registry_keeps_the_local_orphan_quarantined() {
        let data_dir = tempfile::tempdir().unwrap();
        write_registry(data_dir.path(), "{ not json");
        let db = fresh_db().await;
        let fixture = real_provider_fixture(&db, "acp").await;
        let reap_report = reap_orphan_agent_processes(data_dir.path()).await;
        assert!(!reap_report.registry_fully_processed());

        let provider = BootTerminalProofProvider::new(
            frozen(
                &fixture.conversation_id,
                fixture.admitted_epoch,
                Some(&fixture.operation_id),
            ),
            reap_report,
        );
        fixture.service.with_terminal_proof_provider(provider.clone());

        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(
                OWNER,
                &fixture.conversation_id,
                &fixture.runtime_registry,
            )
            .await
            .expect_err("an unreadable registry proves nothing");
        assert_eq!(
            provider
                .prove_orphan_generation_terminal(
                    OWNER,
                    &fixture.conversation_id,
                    OrphanProofRequirement::LocalContainedAuthority,
                    fixture.admitted_epoch,
                    Some(&fixture.operation_id),
                )
                .await,
            TerminalProofDecision::Unproven {
                reason: "durable agent-process registry could not be fully reaped".to_owned()
            }
        );
        assert_quarantined(&fixture).await;
    }

    #[tokio::test]
    async fn real_registry_with_a_live_legacy_entry_keeps_the_local_orphan_quarantined() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = fresh_db().await;
        let fixture = real_provider_fixture(&db, "acp").await;
        // A v1 (identity-less) durable entry for this Conversation with a live
        // pid cannot be attributed to the recorded instance, so the reaper
        // retains it fail-closed.
        write_registry(
            data_dir.path(),
            &format!(
                r#"{{"version":2,"processes":[{{"pid":{},"conversation_id":"{}","agent_type":"acp","registered_at_ms":1}}]}}"#,
                std::process::id(),
                fixture.conversation_id
            ),
        );
        let reap_report = reap_orphan_agent_processes(data_dir.path()).await;
        assert!(reap_report.registry_fully_processed());
        assert!(!reap_report.conversation_tree_proven_empty(&fixture.conversation_id));

        let provider = BootTerminalProofProvider::new(
            frozen(
                &fixture.conversation_id,
                fixture.admitted_epoch,
                Some(&fixture.operation_id),
            ),
            reap_report,
        );
        fixture.service.with_terminal_proof_provider(provider);

        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(
                OWNER,
                &fixture.conversation_id,
                &fixture.runtime_registry,
            )
            .await
            .expect_err("a live legacy pid is not proof of absence");
        assert_quarantined(&fixture).await;

        // The retained entry is still in the registry: absence was never
        // treated as proof.
        let registry: serde_json::Value = serde_json::from_slice(
            &std::fs::read(data_dir.path().join("agent-process-registry.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(registry["processes"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn real_identity_verified_reaping_proves_a_self_spawned_gateway() {
        let data_dir = tempfile::tempdir().unwrap();
        // A live pid whose recorded identity differs proves the recorded
        // instance dead; the reaper must not kill the current pid owner.
        write_registry(
            data_dir.path(),
            &format!(
                r#"{{"version":2,"processes":[{{"pid":{},"conversation_id":"gw-conv","agent_type":"openclaw-gateway","registered_at_ms":1,"identity":{{"pid":{},"start_time_epoch_seconds":0,"platform_start_key":18446744073709551615}}}}]}}"#,
                std::process::id(),
                std::process::id()
            ),
        );
        let db = fresh_db().await;
        let fixture = real_provider_fixture(&db, "openclaw-gateway").await;
        // Re-key the registry entry to this fixture's conversation so the
        // gateway requirement sees it as self-spawned.
        let registry_path = data_dir.path().join("agent-process-registry.json");
        let mut registry: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
        registry["processes"][0]["conversation_id"] =
            serde_json::Value::String(fixture.conversation_id.clone());
        std::fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();

        let reap_report = reap_orphan_agent_processes(data_dir.path()).await;
        assert!(reap_report.registry_fully_processed());
        assert!(reap_report.conversation_had_entries(&fixture.conversation_id));
        assert!(reap_report.conversation_tree_proven_empty(&fixture.conversation_id));

        let provider = BootTerminalProofProvider::new(
            frozen(
                &fixture.conversation_id,
                fixture.admitted_epoch,
                Some(&fixture.operation_id),
            ),
            reap_report,
        );
        fixture.service.with_terminal_proof_provider(provider);

        assert_eq!(
            fixture
                .service
                .reconcile_locally_quiescent_orphan_on_boot(
                    OWNER,
                    &fixture.conversation_id,
                    &fixture.runtime_registry,
                )
                .await
                .unwrap(),
            QuiescentOrphanReconciliation::Reconciled
        );
        let row = fixture.repo.get(&fixture.conversation_id).await.unwrap().unwrap();
        assert_eq!(row.status.as_deref(), Some("finished"));
    }

    #[tokio::test]
    async fn real_provider_refuses_a_generation_outside_the_boot_snapshot() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = fresh_db().await;
        let fixture = real_provider_fixture(&db, "acp").await;
        let reap_report = reap_orphan_agent_processes(data_dir.path()).await;

        // Frozen snapshot names the *previous* epoch: the persisted generation
        // moved after the snapshot and must stay quarantined.
        let provider = BootTerminalProofProvider::new(
            frozen(
                &fixture.conversation_id,
                fixture.admitted_epoch - 1,
                Some(&fixture.operation_id),
            ),
            reap_report,
        );
        fixture.service.with_terminal_proof_provider(provider);

        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(
                OWNER,
                &fixture.conversation_id,
                &fixture.runtime_registry,
            )
            .await
            .expect_err("a moved generation is not covered by the frozen snapshot");
        assert_quarantined(&fixture).await;
    }

    #[tokio::test]
    async fn real_provider_never_settles_a_remote_orphan() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = fresh_db().await;
        let fixture = real_provider_fixture(&db, "remote").await;
        let reap_report = reap_orphan_agent_processes(data_dir.path()).await;
        let provider = BootTerminalProofProvider::new(
            frozen(
                &fixture.conversation_id,
                fixture.admitted_epoch,
                Some(&fixture.operation_id),
            ),
            reap_report,
        );
        fixture.service.with_terminal_proof_provider(provider);

        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(
                OWNER,
                &fixture.conversation_id,
                &fixture.runtime_registry,
            )
            .await
            .expect_err("remote work cannot be proven terminal by a local restart");
        assert_quarantined(&fixture).await;
    }
}

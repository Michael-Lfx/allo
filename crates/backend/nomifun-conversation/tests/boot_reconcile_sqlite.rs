//! End-to-end verification of restart-orphan recovery against the real SQLite
//! repositories (not mocks).
//!
//! This reproduces the exact on-disk state that a process killed mid-"thinking"
//! leaves behind - a `running` conversation row admitted through the production
//! turn claim/receipt protocol, a `thinking` message whose `content.status !=
//! "done"`, and a bound resumable ACP session - then drives the production
//! proof-aware boot seam:
//!
//!   * without an exact terminal proof, every artifact (status, epoch, active
//!     operation, accepted receipt, ACP session, message content) is retained;
//!   * with proof for the exact frozen generation, the row flips to `finished`
//!     with no active operation, the accepted receipt closes as a structured
//!     `interrupted_by_restart` failure, the dangling thinking message closes,
//!     and only a genuinely interrupted turn loses its ACP session.
//!
//! The test-side proof authority is deliberately *not* a constant-true mock: it
//! proves only the exact `(epoch, active operation)` it was armed with, and the
//! remote backend never consults it. Real host providers built from the durable
//! agent-process registry are exercised in `nomifun-app` (router boot proof).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nomifun_ai_agent::runtime_handle::AgentRuntimeHandle;
use nomifun_ai_agent::types::AgentRuntimeBuildOptions;
use nomifun_ai_agent::AgentRuntimeRegistry;
use nomifun_common::{AgentKillReason, AppError, ConversationId, MessageId, TimestampMs, UserId, now_ms};
use nomifun_conversation::skill_resolver::{ResolvedAgentSkill, SkillResolver};
use nomifun_conversation::terminal_proof::{
    OrphanProofRequirement, TerminalProofDecision, TurnTerminalProofProvider,
};
use nomifun_conversation::{ConversationService, NoExecutionConversationBoundary};
use nomifun_conversation::service::QuiescentOrphanReconciliation;
use nomifun_db::models::{ConversationRow, MessageRow};
use nomifun_db::{
    CreateAcpSessionParams, Database, IAcpSessionRepository, IConversationRepository,
    MessageRowUpdate, SortOrder, SqliteAcpSessionRepository, SqliteAgentMetadataRepository,
    SqliteConversationRepository, TurnLifecycleTransition, init_database_memory_with_owner,
};
use nomifun_realtime::UserEventSink;

const OWNER: &str = "0190f5fe-7c00-7a00-8000-000000000001";
const CRASH_REASON: &str =
    "interrupted: the application exited before this turn reached a terminal state";

async fn fresh_db() -> Database {
    init_database_memory_with_owner(UserId::parse(OWNER.to_owned()).expect("canonical owner"))
        .await
        .expect("in-memory database")
}

fn conversation_row(status: &str, agent_type: &str) -> ConversationRow {
    let now = now_ms();
    ConversationRow {
        id: 0,
        conversation_id: ConversationId::new().into_string(),
        user_id: OWNER.to_owned(),
        name: "Ghost turn".to_owned(),
        r#type: agent_type.to_owned(),
        extra: r#"{"workspace":"/home/user/project"}"#.to_owned(),
        delegation_policy: "automatic".to_owned(),
        execution_model_pool: None,
        decision_policy: "automatic".to_owned(),
        execution_template_id: None,
        model: None,
        status: Some(status.to_owned()),
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
    }
}

fn dangling_thinking_message(conv_id: &str, parent_message_id: &str) -> MessageRow {
    MessageRow {
        id: 0,
        message_id: MessageId::new().into_string(),
        conversation_id: conv_id.to_owned(),
        msg_id: Some(parent_message_id.to_owned()),
        r#type: "thinking".to_owned(),
        // The killed process never wrote the terminal `"done"` marker.
        content: r#"{"content":"still reasoning about the request","status":"streaming"}"#
            .to_owned(),
        position: Some("left".to_owned()),
        status: Some("work".to_owned()),
        hidden: false,
        created_at: now_ms(),
    }
}

fn admitted_user_message(conv_id: &str, message_id: &str) -> MessageRow {
    MessageRow {
        id: 0,
        message_id: message_id.to_owned(),
        conversation_id: conv_id.to_owned(),
        msg_id: None,
        r#type: "text".to_owned(),
        content: r#"{"content":"ghost turn"}"#.to_owned(),
        position: Some("right".to_owned()),
        status: None,
        hidden: false,
        created_at: now_ms(),
    }
}

async fn bind_acp_session(acp_repo: &Arc<dyn IAcpSessionRepository>, conv_id: &str) {
    acp_repo
        .create(&CreateAcpSessionParams {
            conversation_id: conv_id,
            agent_backend: "gemini",
            agent_source: "builtin",
            // Empty maps to a NULL `agent_id`, exempt from the RESTRICT FK to
            // `agent_metadata` (no concrete catalog agent needed for this test).
            agent_id: "",
        })
        .await
        .expect("acp session row");
    acp_repo
        .update_session_id(conv_id, "sess-interrupted-turn")
        .await
        .expect("bind resumable session id");
}

struct AdmittedTurn {
    operation_id: String,
    candidate_message_id: String,
    admission_epoch_after: i64,
}

/// Persists the production turn start: claim an accepted delivery receipt and
/// atomically advance Pending -> Running (active operation + epoch + 1). This
/// is the same repository transaction the public send path uses, not a generic
/// status update.
async fn admit_turn(repo: &Arc<dyn IConversationRepository>, conv_id: &str) -> AdmittedTurn {
    let initial = repo
        .get_turn_admission_state(OWNER, conv_id)
        .await
        .expect("initial admission state");
    assert_eq!(initial.active_operation_id, None);
    let operation_id = format!("public-turn:v1:{OWNER}:{conv_id}:interrupted");
    let candidate_message_id = MessageId::new().into_string();
    let claim = repo
        .claim_turn_delivery_receipt_and_admit_with_candidate(
            OWNER,
            conv_id,
            &operation_id,
            &candidate_message_id,
            r#"{"content":"ghost turn"}"#,
            initial.epoch,
            now_ms(),
        )
        .await
        .expect("real turn claim/admission");
    assert!(claim.claimed_new, "fresh admission must win the receipt INSERT");
    assert_eq!(claim.receipt.status, "accepted");
    assert_eq!(claim.receipt.message_id, candidate_message_id);

    let admitted = repo
        .get_turn_admission_state(OWNER, conv_id)
        .await
        .expect("admitted state");
    assert_eq!(admitted.active_operation_id.as_deref(), Some(operation_id.as_str()));
    assert_eq!(admitted.epoch, initial.epoch + 1);

    AdmittedTurn {
        operation_id,
        candidate_message_id,
        admission_epoch_after: admitted.epoch,
    }
}

struct NullSink;
impl UserEventSink for NullSink {
    fn send_to_user(&self, _: &str, _: nomifun_api_types::WebSocketMessage<serde_json::Value>) {}
}

struct NoSkills;
#[async_trait]
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

/// No turn is executed in this recovery drill; recovery must never construct a
/// runtime.
struct NoRuntime;
#[async_trait]
impl AgentRuntimeRegistry for NoRuntime {
    fn get_runtime(&self, _: &str) -> Option<AgentRuntimeHandle> {
        None
    }
    async fn get_or_create_runtime(
        &self,
        _: &str,
        _: AgentRuntimeBuildOptions,
    ) -> Result<AgentRuntimeHandle, AppError> {
        Err(AppError::Internal(
            "restart-orphan recovery must not build a runtime".into(),
        ))
    }
    fn terminate(&self, _: &str, _: Option<AgentKillReason>) -> Result<(), AppError> {
        Ok(())
    }
    fn terminate_all(&self) {}
    fn active_runtime_count(&self) -> usize {
        0
    }
    fn collect_idle_runtimes(&self, _: TimestampMs) -> Vec<String> {
        Vec::new()
    }
}

struct Fixture {
    service: ConversationService,
    repo: Arc<dyn IConversationRepository>,
    acp_repo: Arc<dyn IAcpSessionRepository>,
    runtime_registry: Arc<dyn AgentRuntimeRegistry>,
}

fn fixture(db: &Database) -> Fixture {
    let repo: Arc<dyn IConversationRepository> =
        Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let acp_repo: Arc<dyn IAcpSessionRepository> =
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone()));
    let agent_metadata: Arc<dyn nomifun_db::IAgentMetadataRepository> =
        Arc::new(SqliteAgentMetadataRepository::new(db.pool().clone()));
    let runtime_registry: Arc<dyn AgentRuntimeRegistry> = Arc::new(NoRuntime);
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
    Fixture {
        service,
        repo,
        acp_repo,
        runtime_registry,
    }
}

/// Test-side terminal-proof authority: proves only the exact generation it was
/// armed with and records every consultation. `unproven_reason` models a proof
/// source that could not complete its pass (e.g. an unread durable registry).
struct ExactGenerationProof {
    proves: Mutex<HashMap<String, (i64, Option<String>)>>,
    consultations: Mutex<Vec<(String, OrphanProofRequirement)>>,
    unproven_reason: Option<String>,
}

impl ExactGenerationProof {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            proves: Mutex::new(HashMap::new()),
            consultations: Mutex::new(Vec::new()),
            unproven_reason: None,
        })
    }

    fn without_proof(reason: &str) -> Arc<Self> {
        Arc::new(Self {
            proves: Mutex::new(HashMap::new()),
            consultations: Mutex::new(Vec::new()),
            unproven_reason: Some(reason.to_owned()),
        })
    }

    fn arm(&self, conversation_id: &str, epoch: i64, operation_id: Option<&str>) {
        self.proves.lock().unwrap().insert(
            conversation_id.to_owned(),
            (epoch, operation_id.map(str::to_owned)),
        );
    }

    fn consultations(&self) -> Vec<(String, OrphanProofRequirement)> {
        self.consultations.lock().unwrap().clone()
    }
}

#[async_trait]
impl TurnTerminalProofProvider for ExactGenerationProof {
    async fn prove_orphan_generation_terminal(
        &self,
        _user_id: &str,
        conversation_id: &str,
        requirement: OrphanProofRequirement,
        admission_epoch: i64,
        active_operation_id: Option<&str>,
    ) -> TerminalProofDecision {
        self.consultations
            .lock()
            .unwrap()
            .push((conversation_id.to_owned(), requirement));
        if let Some(reason) = &self.unproven_reason {
            return TerminalProofDecision::Unproven {
                reason: reason.clone(),
            };
        }
        match self.proves.lock().unwrap().get(conversation_id) {
            Some((epoch, operation))
                if *epoch == admission_epoch
                    && operation.as_deref() == active_operation_id =>
            {
                TerminalProofDecision::Proven {
                    evidence: "test provider proved the exact frozen generation".to_owned(),
                }
            }
            _ => TerminalProofDecision::Unproven {
                reason: "test provider was not armed for this generation".to_owned(),
            },
        }
    }
}

async fn seed_ghost(
    fixture: &Fixture,
    agent_type: &str,
) -> (String, AdmittedTurn) {
    let ghost = conversation_row("pending", agent_type);
    let conv_id = ghost.conversation_id.clone();
    fixture.repo.create(&ghost).await.expect("create conversation");
    let admitted = admit_turn(&fixture.repo, &conv_id).await;
    fixture
        .repo
        .insert_message(&admitted_user_message(&conv_id, &admitted.candidate_message_id))
        .await
        .expect("project the admitted user message");
    fixture
        .repo
        .insert_message(&dangling_thinking_message(
            &conv_id,
            &admitted.candidate_message_id,
        ))
        .await
        .expect("insert dangling thinking message");
    bind_acp_session(&fixture.acp_repo, &conv_id).await;
    (conv_id, admitted)
}

async fn settlement_state(fixture: &Fixture, conv_id: &str) -> (Option<String>, i64, Option<String>) {
    let row = fixture.repo.get(conv_id).await.unwrap().unwrap();
    let admission = fixture
        .repo
        .get_turn_admission_state(OWNER, conv_id)
        .await
        .unwrap();
    (row.status, admission.epoch, admission.active_operation_id)
}

#[tokio::test]
async fn boot_reconcile_proven_orphan_settles_transcript_and_session_for_local_backends() {
    for agent_type in ["nomi", "acp", "nanobot"] {
        let db = fresh_db().await;
        let fixture = fixture(&db);
        let (conv_id, admitted) = seed_ghost(&fixture, agent_type).await;

        let provider = ExactGenerationProof::new();
        provider.arm(
            &conv_id,
            admitted.admission_epoch_after,
            Some(&admitted.operation_id),
        );
        fixture.service.with_terminal_proof_provider(provider.clone());

        assert_eq!(
            fixture
                .service
                .reconcile_locally_quiescent_orphan_on_boot(
                    OWNER,
                    &conv_id,
                    &fixture.runtime_registry,
                )
                .await
                .unwrap(),
            QuiescentOrphanReconciliation::Reconciled,
            "{agent_type}"
        );

        let after = fixture.repo.get(&conv_id).await.unwrap().unwrap();
        assert_eq!(after.status.as_deref(), Some("finished"), "{agent_type}");
        let admission_after = fixture
            .repo
            .get_turn_admission_state(OWNER, &conv_id)
            .await
            .unwrap();
        assert_eq!(admission_after.active_operation_id, None, "{agent_type}");
        assert_eq!(
            admission_after.epoch,
            admitted.admission_epoch_after + 1,
            "{agent_type}: finalizing the orphan must advance the admission epoch exactly once"
        );

        let receipt_after = fixture
            .repo
            .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
            .await
            .unwrap()
            .expect("receipt survives reconciliation");
        assert_eq!(receipt_after.status, "completed", "{agent_type}");
        assert_eq!(receipt_after.result_ok, Some(false), "{agent_type}");
        assert_eq!(
            receipt_after.result_error.as_deref(),
            Some(CRASH_REASON),
            "{agent_type}"
        );
        assert_eq!(
            receipt_after.result_error_code.as_deref(),
            Some(nomifun_conversation::relay_error_code::INTERRUPTED_BY_RESTART),
            "{agent_type}"
        );

        let session_after = fixture.acp_repo.get(&conv_id).await.unwrap().unwrap();
        assert_eq!(
            session_after.acp_session_id, None,
            "{agent_type}: a genuinely interrupted turn must drop its resumable session"
        );

        let messages = fixture
            .repo
            .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
            .await
            .unwrap()
            .items;
        let thinking = messages
            .iter()
            .find(|m| m.r#type == "thinking")
            .expect("thinking message survives reconciliation");
        let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
        assert_eq!(content["status"], "done", "{agent_type}");
        assert_eq!(
            content["content"], "still reasoning about the request",
            "{agent_type}: accumulated thinking content must be preserved"
        );
        assert!(messages.iter().any(|m| m.r#type == "tips"), "{agent_type}");

        // Idempotent replay: the healed generation is terminal, so a second
        // boot pass is a read-only no-op.
        assert_eq!(
            fixture
                .service
                .reconcile_locally_quiescent_orphan_on_boot(
                    OWNER,
                    &conv_id,
                    &fixture.runtime_registry,
                )
                .await
                .unwrap(),
            QuiescentOrphanReconciliation::AlreadyTerminal,
            "{agent_type}"
        );
        assert_eq!(
            settlement_state(&fixture, &conv_id).await,
            (Some("finished".to_owned()), admitted.admission_epoch_after + 1, None),
            "{agent_type}"
        );
    }
}

#[tokio::test]
async fn boot_reconcile_remote_orphan_stays_quarantined_even_with_a_matching_provider() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, admitted) = seed_ghost(&fixture, "remote").await;

    let provider = ExactGenerationProof::new();
    provider.arm(
        &conv_id,
        admitted.admission_epoch_after,
        Some(&admitted.operation_id),
    );
    fixture.service.with_terminal_proof_provider(provider.clone());

    let error = fixture
        .service
        .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
        .await
        .expect_err("remote work may still run outside this machine");
    assert!(matches!(error, AppError::Conflict(_)), "{error}");
    assert!(
        provider.consultations().is_empty(),
        "an external-execution backend must never consult a local proof provider"
    );

    // No terminal proof, no settlement: every durable artifact is retained.
    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("running".to_owned()),
            admitted.admission_epoch_after,
            Some(admitted.operation_id.clone())
        )
    );
    let receipt = fixture
        .repo
        .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.status, "accepted");
    assert_eq!(receipt.result_ok, None);
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn")
    );
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|m| m.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "streaming");
    assert_eq!(thinking.status.as_deref(), Some("work"));
}

#[tokio::test]
async fn boot_reconcile_unregistered_gateway_stays_quarantined_without_registry_proof() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, admitted) = seed_ghost(&fixture, "openclaw-gateway").await;

    // Arm a proof for the exact generation: the gateway requirement itself
    // must still refuse because registry absence cannot prove an external or
    // attached gateway terminal.
    let provider = ExactGenerationProof::without_proof(
        "gateway process was not registry-owned by this data dir",
    );
    fixture.service.with_terminal_proof_provider(provider.clone());

    fixture
        .service
        .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
        .await
        .expect_err("unregistered gateway work stays quarantined");
    assert_eq!(
        provider.consultations(),
        vec![(
            conv_id.clone(),
            OrphanProofRequirement::RegisteredGatewayAuthority,
        )],
        "the gateway requirement must be presented to the proof source"
    );
    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("running".to_owned()),
            admitted.admission_epoch_after,
            Some(admitted.operation_id.clone())
        )
    );
    assert_eq!(
        fixture
            .repo
            .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "accepted"
    );
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn")
    );
}

#[tokio::test]
async fn boot_reconcile_unprocessed_or_failed_proof_keeps_every_artifact() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, admitted) = seed_ghost(&fixture, "acp").await;

    let provider = ExactGenerationProof::without_proof(
        "durable agent-process registry could not be fully reaped",
    );
    fixture.service.with_terminal_proof_provider(provider.clone());

    fixture
        .service
        .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
        .await
        .expect_err("no completed registry pass means no proof");
    assert_eq!(provider.consultations().len(), 1);
    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("running".to_owned()),
            admitted.admission_epoch_after,
            Some(admitted.operation_id.clone())
        )
    );
    let receipt = fixture
        .repo
        .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.status, "accepted");
    assert_eq!(receipt.result_ok, None);
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn")
    );
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|m| m.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "streaming");
    assert_eq!(thinking.status.as_deref(), Some("work"));
}

#[tokio::test]
async fn boot_reconcile_expired_generation_proof_keeps_every_artifact() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, admitted) = seed_ghost(&fixture, "acp").await;

    // Armed for a different epoch: the generation moved, so this proof cannot
    // cover the persisted turn.
    let provider = ExactGenerationProof::new();
    provider.arm(
        &conv_id,
        admitted.admission_epoch_after + 1,
        Some(&admitted.operation_id),
    );
    fixture.service.with_terminal_proof_provider(provider);

    fixture
        .service
        .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
        .await
        .expect_err("a proof outside the exact old generation must not settle it");
    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("running".to_owned()),
            admitted.admission_epoch_after,
            Some(admitted.operation_id.clone())
        )
    );
    assert_eq!(
        fixture
            .repo
            .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "accepted"
    );
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    assert!(messages.iter().all(|m| m.r#type != "tips"));
    let thinking = messages
        .iter()
        .find(|message| message.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "streaming");
    assert_eq!(thinking.status.as_deref(), Some("work"));
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn"),
        "an unproven generation must not drop its resumable session"
    );
}

#[tokio::test]
async fn boot_reconcile_proven_leaked_marker_keeps_the_resumable_session() {
    let db = fresh_db().await;
    let fixture = fixture(&db);

    // The turn ran to completion through a path that leaked the `running`
    // write-back: a real accepted receipt and active operation exist, but no
    // dangling message was left behind. Boot must settle the lifecycle without
    // destroying the usable resumable session.
    let leaked = conversation_row("pending", "acp");
    let conv_id = leaked.conversation_id.clone();
    fixture.repo.create(&leaked).await.expect("create conversation");
    let admitted = admit_turn(&fixture.repo, &conv_id).await;
    bind_acp_session(&fixture.acp_repo, &conv_id).await;

    let provider = ExactGenerationProof::new();
    provider.arm(
        &conv_id,
        admitted.admission_epoch_after,
        Some(&admitted.operation_id),
    );
    fixture.service.with_terminal_proof_provider(provider);

    assert_eq!(
        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(
                OWNER,
                &conv_id,
                &fixture.runtime_registry,
            )
            .await
            .unwrap(),
        QuiescentOrphanReconciliation::Reconciled
    );

    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (Some("finished".to_owned()), admitted.admission_epoch_after + 1, None)
    );
    assert_eq!(
        fixture
            .repo
            .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "completed"
    );
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn"),
        "a leaked marker must not discard the healthy resumable session"
    );
}

#[tokio::test]
async fn boot_reconcile_treats_a_settled_conversation_as_already_terminal() {
    let db = fresh_db().await;
    let fixture = fixture(&db);

    let healthy = conversation_row("finished", "acp");
    let conv_id = healthy.conversation_id.clone();
    fixture.repo.create(&healthy).await.expect("create conversation");
    bind_acp_session(&fixture.acp_repo, &conv_id).await;

    assert_eq!(
        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(
                OWNER,
                &conv_id,
                &fixture.runtime_registry,
            )
            .await
            .unwrap(),
        QuiescentOrphanReconciliation::AlreadyTerminal
    );
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn"),
        "a healthy conversation's ACP session must be left intact"
    );
}

#[tokio::test]
async fn boot_reconcile_transient_acp_cleanup_failure_rolls_back_and_is_retryable() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, admitted) = seed_ghost(&fixture, "acp").await;
    let provider = ExactGenerationProof::new();
    provider.arm(
        &conv_id,
        admitted.admission_epoch_after,
        Some(&admitted.operation_id),
    );
    fixture.service.with_terminal_proof_provider(provider);

    // The durable registry may still hold an unproven entry for this
    // Conversation; the terminal-proof wait itself is re-tested in the host
    // provider integration. This test targets the transcript/ACP cleanup.
    nomifun_db::sqlx::query(
        "CREATE TRIGGER reject_recovery_acp_clear \
         BEFORE UPDATE OF acp_session_id ON acp_session \
         WHEN NEW.acp_session_id IS NULL \
         BEGIN SELECT RAISE(ABORT, 'review injected clear failure'); END",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let first = fixture
        .service
        .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
        .await;
    assert!(
        first.is_err(),
        "a cleanup failure must not be reported as a completed reconciliation"
    );

    // The lifecycle finalization and every artifact write share one
    // transaction: the failure rolls the whole generation back, so the next
    // boot still enumerates it and can retry.
    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("running".to_owned()),
            admitted.admission_epoch_after,
            Some(admitted.operation_id.clone())
        )
    );
    let receipt = fixture
        .repo
        .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.status, "accepted");
    assert_eq!(receipt.result_ok, None);
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|message| message.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "streaming");
    assert_eq!(thinking.status.as_deref(), Some("work"));
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn")
    );

    nomifun_db::sqlx::query("DROP TRIGGER reject_recovery_acp_clear")
        .execute(db.pool())
        .await
        .unwrap();

    assert_eq!(
        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
            .await
            .unwrap(),
        QuiescentOrphanReconciliation::Reconciled
    );
    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("finished".to_owned()),
            admitted.admission_epoch_after + 1,
            None
        )
    );
    let receipt = fixture
        .repo
        .get_delivery_receipt(OWNER, &conv_id, &admitted.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.status, "completed");
    assert_eq!(receipt.result_ok, Some(false));
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|message| message.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "done");
    assert_eq!(content["content"], "still reasoning about the request");
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id,
        None,
        "the retry must finish the durable cleanup"
    );

    assert_eq!(
        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
            .await
            .unwrap(),
        QuiescentOrphanReconciliation::AlreadyTerminal
    );
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id,
        None
    );
}

#[tokio::test]
async fn boot_reconcile_transient_message_cleanup_failure_rolls_back_and_is_retryable() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, admitted) = seed_ghost(&fixture, "acp").await;
    let provider = ExactGenerationProof::new();
    provider.arm(
        &conv_id,
        admitted.admission_epoch_after,
        Some(&admitted.operation_id),
    );
    fixture.service.with_terminal_proof_provider(provider);

    nomifun_db::sqlx::query(&format!(
        "CREATE TRIGGER reject_recovery_message_cleanup \
         BEFORE UPDATE OF status ON messages \
         WHEN NEW.conversation_id = '{conv_id}' AND NEW.status = 'finish' \
         BEGIN SELECT RAISE(ABORT, 'review injected message cleanup failure'); END"
    ))
    .execute(db.pool())
    .await
    .unwrap();

    fixture
        .service
        .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
        .await
        .expect_err("a message cleanup failure must roll back lifecycle settlement");

    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("running".to_owned()),
            admitted.admission_epoch_after,
            Some(admitted.operation_id.clone())
        )
    );
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|message| message.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "streaming");
    assert_eq!(thinking.status.as_deref(), Some("work"));
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn")
    );

    nomifun_db::sqlx::query("DROP TRIGGER reject_recovery_message_cleanup")
        .execute(db.pool())
        .await
        .unwrap();

    assert_eq!(
        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
            .await
            .unwrap(),
        QuiescentOrphanReconciliation::Reconciled
    );
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|message| message.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "done");
    assert_eq!(content["content"], "still reasoning about the request");
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id,
        None,
        "the retry must clear the interrupted session after message cleanup recovers"
    );
}

#[tokio::test]
async fn boot_reconcile_transient_stranded_work_message_failure_rolls_back_and_is_retryable() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let ghost = conversation_row("pending", "acp");
    let conv_id = ghost.conversation_id.clone();
    fixture.repo.create(&ghost).await.expect("create conversation");
    let admitted = admit_turn(&fixture.repo, &conv_id).await;
    fixture
        .repo
        .insert_message(&admitted_user_message(&conv_id, &admitted.candidate_message_id))
        .await
        .expect("project the admitted user message");
    // Completed thinking (no dangling thought) plus a stranded stream message:
    // the cleanup signal here is the work status alone.
    fixture
        .repo
        .insert_message(&MessageRow {
            id: 0,
            message_id: MessageId::new().into_string(),
            conversation_id: conv_id.clone(),
            msg_id: Some(admitted.candidate_message_id.clone()),
            r#type: "thinking".to_owned(),
            content: r#"{"content":"finished thought","status":"done"}"#.to_owned(),
            position: Some("left".to_owned()),
            status: Some("finish".to_owned()),
            hidden: false,
            created_at: now_ms(),
        })
        .await
        .expect("insert finished thinking message");
    let stranded_message_id = MessageId::new().into_string();
    fixture
        .repo
        .insert_message(&MessageRow {
            id: 0,
            message_id: stranded_message_id.clone(),
            conversation_id: conv_id.clone(),
            msg_id: Some(admitted.candidate_message_id.clone()),
            r#type: "text".to_owned(),
            content: r#"{"content":"partial assistant stream"}"#.to_owned(),
            position: Some("left".to_owned()),
            status: Some("work".to_owned()),
            hidden: false,
            created_at: now_ms(),
        })
        .await
        .expect("insert stranded work message");
    bind_acp_session(&fixture.acp_repo, &conv_id).await;

    let provider = ExactGenerationProof::new();
    provider.arm(
        &conv_id,
        admitted.admission_epoch_after,
        Some(&admitted.operation_id),
    );
    fixture.service.with_terminal_proof_provider(provider);

    nomifun_db::sqlx::query(&format!(
        "CREATE TRIGGER reject_recovery_work_cleanup \
         BEFORE UPDATE OF status ON messages \
         WHEN NEW.message_id = '{stranded_message_id}' AND NEW.status = 'finish' \
         BEGIN SELECT RAISE(ABORT, 'review injected work cleanup failure'); END"
    ))
    .execute(db.pool())
    .await
    .unwrap();

    fixture
        .service
        .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
        .await
        .expect_err("a stranded-work cleanup failure must roll back lifecycle settlement");
    assert_eq!(
        settlement_state(&fixture, &conv_id).await,
        (
            Some("running".to_owned()),
            admitted.admission_epoch_after,
            Some(admitted.operation_id.clone())
        )
    );
    let stranded = fixture
        .repo
        .get_message(&conv_id, &stranded_message_id)
        .await
        .unwrap()
        .expect("stranded work message retained");
    assert_eq!(stranded.status.as_deref(), Some("work"));
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("sess-interrupted-turn")
    );

    nomifun_db::sqlx::query("DROP TRIGGER reject_recovery_work_cleanup")
        .execute(db.pool())
        .await
        .unwrap();

    assert_eq!(
        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
            .await
            .unwrap(),
        QuiescentOrphanReconciliation::Reconciled
    );
    let stranded = fixture
        .repo
        .get_message(&conv_id, &stranded_message_id)
        .await
        .unwrap()
        .expect("stranded work message retained");
    assert_eq!(stranded.status.as_deref(), Some("finish"));
    assert_eq!(
        stranded.content,
        r#"{"content":"partial assistant stream"}"#,
        "cleanup must terminalize the stream without rewriting its content"
    );
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id,
        None
    );
}

#[tokio::test]
async fn stale_cleanup_cannot_touch_a_later_finished_generation_or_its_session() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, a) = seed_ghost(&fixture, "acp").await;
    assert_eq!(
        fixture
            .repo
            .finalize_exact_cancelled_turn_generation(
                OWNER,
                &conv_id,
                a.admission_epoch_after,
                Some(&a.operation_id),
                "A stopped",
                None,
                None,
                now_ms(),
            )
            .await
            .unwrap(),
        TurnLifecycleTransition::Committed
    );

    let before_b = fixture
        .repo
        .get_turn_admission_state(OWNER, &conv_id)
        .await
        .unwrap();
    let b_operation_id = format!("public-turn:v1:{OWNER}:{conv_id}:B");
    let b_candidate = MessageId::new().into_string();
    fixture
        .repo
        .claim_turn_delivery_receipt_and_admit_with_candidate(
            OWNER,
            &conv_id,
            &b_operation_id,
            &b_candidate,
            r#"{"content":"B"}"#,
            before_b.epoch,
            now_ms(),
        )
        .await
        .unwrap();
    let b_admitted = fixture
        .repo
        .get_turn_admission_state(OWNER, &conv_id)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .repo
            .finalize_exact_cancelled_turn_generation(
                OWNER,
                &conv_id,
                b_admitted.epoch,
                Some(&b_operation_id),
                "B stopped",
                None,
                None,
                now_ms(),
            )
            .await
            .unwrap(),
        TurnLifecycleTransition::Committed
    );
    let b_work_message_id = MessageId::new().into_string();
    fixture
        .repo
        .insert_message(&MessageRow {
            id: 0,
            message_id: b_work_message_id.clone(),
            conversation_id: conv_id.clone(),
            msg_id: None,
            r#type: "thinking".to_owned(),
            content: r#"{"content":"B still running","status":"streaming"}"#.to_owned(),
            position: Some("left".to_owned()),
            status: Some("work".to_owned()),
            hidden: false,
            created_at: now_ms(),
        })
        .await
        .unwrap();
    nomifun_db::sqlx::query(
        "UPDATE acp_session SET acp_session_id = 'session-of-B' WHERE conversation_id = ?",
    )
    .bind(&conv_id)
    .execute(db.pool())
    .await
    .unwrap();

    // An old A generation may still absorb A's immutable receipt, but it has
    // no permission to write B's transcript or ACP session.
    let transition = fixture
        .repo
        .finalize_exact_cancelled_turn_generation_with_artifact_cleanup(
            OWNER,
            &conv_id,
            a.admission_epoch_after,
            Some(&a.operation_id),
            "late A cleanup",
            None,
            None,
            now_ms(),
            &[(
                b_work_message_id.clone(),
                MessageRowUpdate {
                    status: Some(Some("finish".to_owned())),
                    ..Default::default()
                },
            )],
            true,
        )
        .await
        .unwrap();
    assert_eq!(transition, TurnLifecycleTransition::AlreadyApplied);
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("session-of-B"),
        "stale A cleanup must not erase the later finished B session"
    );
    let b_message = fixture
        .repo
        .get_message(&conv_id, &b_work_message_id)
        .await
        .unwrap()
        .expect("B message retained");
    assert_eq!(
        b_message.status.as_deref(),
        Some("work"),
        "stale A cleanup must not mutate B's message"
    );
    let b_terminal = fixture
        .repo
        .get_turn_admission_state(OWNER, &conv_id)
        .await
        .unwrap();
    assert_eq!(b_terminal.active_operation_id, None);
    assert_eq!(b_terminal.epoch, before_b.epoch + 2);
}

#[tokio::test]
async fn stale_cleanup_cannot_touch_a_later_running_generation() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, a) = seed_ghost(&fixture, "acp").await;
    fixture
        .repo
        .finalize_exact_cancelled_turn_generation(
            OWNER,
            &conv_id,
            a.admission_epoch_after,
            Some(&a.operation_id),
            "A stopped",
            None,
            None,
            now_ms(),
        )
        .await
        .unwrap();

    let before_b = fixture
        .repo
        .get_turn_admission_state(OWNER, &conv_id)
        .await
        .unwrap();
    let b_operation_id = format!("public-turn:v1:{OWNER}:{conv_id}:B-running");
    let b_candidate = MessageId::new().into_string();
    fixture
        .repo
        .claim_turn_delivery_receipt_and_admit_with_candidate(
            OWNER,
            &conv_id,
            &b_operation_id,
            &b_candidate,
            r#"{"content":"B running"}"#,
            before_b.epoch,
            now_ms(),
        )
        .await
        .unwrap();
    let b_work_message_id = MessageId::new().into_string();
    fixture
        .repo
        .insert_message(&MessageRow {
            id: 0,
            message_id: b_work_message_id.clone(),
            conversation_id: conv_id.clone(),
            msg_id: None,
            r#type: "thinking".to_owned(),
            content: r#"{"content":"B still running","status":"streaming"}"#.to_owned(),
            position: Some("left".to_owned()),
            status: Some("work".to_owned()),
            hidden: false,
            created_at: now_ms(),
        })
        .await
        .unwrap();
    nomifun_db::sqlx::query(
        "UPDATE acp_session SET acp_session_id = 'session-of-B' WHERE conversation_id = ?",
    )
    .bind(&conv_id)
    .execute(db.pool())
    .await
    .unwrap();
    let b_admitted = fixture
        .repo
        .get_turn_admission_state(OWNER, &conv_id)
        .await
        .unwrap();

    let transition = fixture
        .repo
        .finalize_exact_cancelled_turn_generation_with_artifact_cleanup(
            OWNER,
            &conv_id,
            a.admission_epoch_after,
            Some(&a.operation_id),
            "late A cleanup",
            None,
            None,
            now_ms(),
            &[(
                b_work_message_id.clone(),
                MessageRowUpdate {
                    status: Some(Some("finish".to_owned())),
                    ..Default::default()
                },
            )],
            true,
        )
        .await
        .unwrap();
    assert_eq!(transition, TurnLifecycleTransition::Stale);
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("session-of-B")
    );
    assert_eq!(
        fixture
            .repo
            .get_message(&conv_id, &b_work_message_id)
            .await
            .unwrap()
            .expect("B message retained")
            .status
            .as_deref(),
        Some("work")
    );
    assert_eq!(
        fixture
            .repo
            .get_turn_admission_state(OWNER, &conv_id)
            .await
            .unwrap(),
        b_admitted
    );
}

#[tokio::test]
async fn boot_reconcile_covers_transcript_tail_beyond_one_recovery_page() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, admitted) = seed_ghost(&fixture, "acp").await;
    // More than one recovery page of completed history precedes the
    // interrupted thinking message at the tail.
    for index in 0..5001 {
        let mut message = admitted_user_message(&conv_id, &MessageId::new().into_string());
        message.created_at = index;
        fixture.repo.insert_message(&message).await.unwrap();
    }
    let provider = ExactGenerationProof::new();
    provider.arm(
        &conv_id,
        admitted.admission_epoch_after,
        Some(&admitted.operation_id),
    );
    fixture.service.with_terminal_proof_provider(provider);

    assert_eq!(
        fixture
            .service
            .reconcile_locally_quiescent_orphan_on_boot(OWNER, &conv_id, &fixture.runtime_registry)
            .await
            .unwrap(),
        QuiescentOrphanReconciliation::Reconciled
    );
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id,
        None,
        "the tail interrupted session must be cleared"
    );
    let messages = fixture
        .repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Desc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|message| message.r#type == "thinking")
        .expect("thinking message retained");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(content["status"], "done");
    assert_eq!(content["content"], "still reasoning about the request");
    assert_eq!(thinking.status.as_deref(), Some("finish"));
}

#[tokio::test]
async fn stale_cleanup_cannot_touch_a_later_active_null_generation() {
    let db = fresh_db().await;
    let fixture = fixture(&db);
    let (conv_id, a) = seed_ghost(&fixture, "acp").await;
    fixture
        .repo
        .finalize_exact_cancelled_turn_generation(
            OWNER,
            &conv_id,
            a.admission_epoch_after,
            Some(&a.operation_id),
            "A stopped",
            None,
            None,
            now_ms(),
        )
        .await
        .unwrap();

    let before_b = fixture
        .repo
        .get_turn_admission_state(OWNER, &conv_id)
        .await
        .unwrap();
    let b_operation_id = format!("public-turn:v1:{OWNER}:{conv_id}:B-active-null");
    let b_candidate = MessageId::new().into_string();
    fixture
        .repo
        .claim_turn_delivery_receipt_and_admit_with_candidate(
            OWNER,
            &conv_id,
            &b_operation_id,
            &b_candidate,
            r#"{"content":"B active-null"}"#,
            before_b.epoch,
            now_ms(),
        )
        .await
        .unwrap();
    // Simulate the crash cutpoint where the running row lost its active
    // operation pointer while the next epoch is already persisted.
    nomifun_db::sqlx::query("DROP TRIGGER IF EXISTS trg_conversations_running_exit_guard")
        .execute(db.pool())
        .await
        .unwrap();
    nomifun_db::sqlx::query("DROP TRIGGER IF EXISTS trg_conversations_running_owner_immutable")
        .execute(db.pool())
        .await
        .unwrap();
    nomifun_db::sqlx::query(
        "UPDATE conversations SET active_turn_operation_id = NULL \
         WHERE conversation_id = ? AND user_id = ?",
    )
    .bind(&conv_id)
    .bind(OWNER)
    .execute(db.pool())
    .await
    .unwrap();
    let b_active_null = fixture
        .repo
        .get_turn_admission_state(OWNER, &conv_id)
        .await
        .unwrap();
    assert_eq!(b_active_null.active_operation_id, None);
    assert_eq!(b_active_null.epoch, before_b.epoch + 1);

    let b_work_message_id = MessageId::new().into_string();
    fixture
        .repo
        .insert_message(&MessageRow {
            id: 0,
            message_id: b_work_message_id.clone(),
            conversation_id: conv_id.clone(),
            msg_id: None,
            r#type: "thinking".to_owned(),
            content: r#"{"content":"B active-null work","status":"streaming"}"#.to_owned(),
            position: Some("left".to_owned()),
            status: Some("work".to_owned()),
            hidden: false,
            created_at: now_ms(),
        })
        .await
        .unwrap();
    nomifun_db::sqlx::query(
        "UPDATE acp_session SET acp_session_id = 'session-of-B' WHERE conversation_id = ?",
    )
    .bind(&conv_id)
    .execute(db.pool())
    .await
    .unwrap();

    let transition = fixture
        .repo
        .finalize_exact_cancelled_turn_generation_with_artifact_cleanup(
            OWNER,
            &conv_id,
            a.admission_epoch_after,
            Some(&a.operation_id),
            "late A cleanup",
            None,
            None,
            now_ms(),
            &[(
                b_work_message_id.clone(),
                MessageRowUpdate {
                    status: Some(Some("finish".to_owned())),
                    ..Default::default()
                },
            )],
            true,
        )
        .await
        .unwrap();
    assert_eq!(transition, TurnLifecycleTransition::Stale);
    assert_eq!(
        fixture
            .acp_repo
            .get(&conv_id)
            .await
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("session-of-B")
    );
    assert_eq!(
        fixture
            .repo
            .get_message(&conv_id, &b_work_message_id)
            .await
            .unwrap()
            .expect("B message retained")
            .status
            .as_deref(),
        Some("work")
    );
    assert_eq!(
        fixture
            .repo
            .get_turn_admission_state(OWNER, &conv_id)
            .await
            .unwrap(),
        b_active_null
    );
}

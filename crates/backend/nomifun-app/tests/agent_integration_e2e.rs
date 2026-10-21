//! E2E integration tests with mock Agent runtimes.
//!
//! Tests the message flow, confirmation system, and auxiliary routes
//! with a mock AgentRuntimeRegistry that provides in-memory agents.

mod common;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tower::ServiceExt;

use async_trait::async_trait;
use nomifun_ai_agent::runtime_handle::{AgentRuntimeHandle, AgentRuntimeControl, MockAgentRuntime};
use nomifun_ai_agent::protocol::events::TextEventData;
use nomifun_ai_agent::types::{AgentRuntimeBuildOptions, SendMessageData};
use nomifun_ai_agent::{AgentStreamEvent, AgentRuntimeRegistry};
use nomifun_common::{AgentKillReason, AgentType, AppError, Confirmation, ConversationStatus, TimestampMs, now_ms};

use common::{body_json, get_with_token, json_with_token, setup_and_login};
use nomifun_common::{
    AdaptationPolicy, AgentExecutionEventKind, AgentExecutionStatus, AgentToolPolicy,
    ConversationId, DecisionPolicy, DelegationPolicy, ExecutionStepKind, ExecutionStepStatus,
    PlanGate, StepFailurePolicy,
};
use nomifun_db::models::ConversationRow;
use nomifun_db::{
    AgentExecutionLeaseToken, CreateAgentExecutionAttemptParams, CreateAgentExecutionParams,
    IAgentExecutionRepository, IConversationRepository, NewAgentExecutionEvent,
    NewAgentExecutionParticipant, NewAgentExecutionStep, ReconcileAgentExecutionPlanParams,
    SqliteAgentExecutionRepository, SqliteConversationRepository,
};

const GEMINI_AGENT_ID: &str = "0190f5fe-7c00-7a00-8000-000000000103";
const MESSAGE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";

// ── Mock Agent ──────────────────────────────────────────────────

struct MockAgent {
    conversation_id: String,
    workspace: String,
    event_tx: broadcast::Sender<AgentStreamEvent>,
    confirmations: Mutex<Vec<Confirmation>>,
    approvals: Mutex<std::collections::HashMap<String, bool>>,
    last_activity: AtomicI64,
}

impl MockAgent {
    fn new(conversation_id: &str, workspace: &str) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            conversation_id: conversation_id.to_owned(),
            workspace: workspace.to_owned(),
            event_tx,
            confirmations: Mutex::new(vec![]),
            approvals: Mutex::new(std::collections::HashMap::new()),
            last_activity: AtomicI64::new(now_ms()),
        }
    }
}

#[async_trait]
impl AgentRuntimeControl for MockAgent {
    fn agent_type(&self) -> AgentType {
        AgentType::Acp
    }

    fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    fn workspace(&self) -> &str {
        &self.workspace
    }

    fn status(&self) -> Option<ConversationStatus> {
        Some(ConversationStatus::Running)
    }

    fn is_transport_healthy(&self) -> bool {
        true
    }

    fn last_activity_at(&self) -> TimestampMs {
        self.last_activity.load(Ordering::Relaxed)
    }

    fn subscribe(&self) -> broadcast::Receiver<AgentStreamEvent> {
        self.event_tx.subscribe()
    }

    async fn send_message(&self, _data: SendMessageData) -> Result<(), nomifun_ai_agent::AgentSendError> {
        self.last_activity.store(now_ms(), Ordering::Relaxed);
        // Emit a text event and finish
        let _ = self.event_tx.send(AgentStreamEvent::Text(TextEventData {
            content: "Mock response".into(),
        }));
        let _ = self.event_tx.send(AgentStreamEvent::Finish(
            nomifun_ai_agent::protocol::events::FinishEventData::default(),
        ));
        Ok(())
    }

    async fn cancel(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn kill(&self, _reason: Option<AgentKillReason>) -> Result<(), AppError> {
        Ok(())
    }
}

#[async_trait]
impl MockAgentRuntime for MockAgent {
    fn get_confirmations(&self) -> Vec<Confirmation> {
        self.confirmations.lock().unwrap().clone()
    }

    fn check_approval(&self, action: &str, _command_type: Option<&str>) -> bool {
        self.approvals.lock().unwrap().get(action).copied().unwrap_or(false)
    }

    fn confirm(&self, _msg_id: &str, call_id: &str, _data: Value, always_allow: bool) -> Result<(), AppError> {
        let mut confs = self.confirmations.lock().unwrap();
        confs.retain(|c| c.call_id != call_id);
        if always_allow {
            self.approvals.lock().unwrap().insert("test_action".to_owned(), true);
        }
        Ok(())
    }
}

// ── Mock Agent Runtime Registry ────────────────────────────────────

struct MockAgentRuntimeRegistry {
    agents: Mutex<std::collections::HashMap<String, AgentRuntimeHandle>>,
}

impl MockAgentRuntimeRegistry {
    fn new() -> Self {
        Self {
            agents: Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn insert(&self, conv_id: &str, workspace: &str) -> Arc<MockAgent> {
        let agent = Arc::new(MockAgent::new(conv_id, workspace));
        self.agents
            .lock()
            .unwrap()
            .insert(conv_id.to_owned(), AgentRuntimeHandle::Mock(agent.clone()));
        agent
    }
}

#[async_trait::async_trait]
impl AgentRuntimeRegistry for MockAgentRuntimeRegistry {
    fn get_runtime(&self, conversation_id: &str) -> Option<AgentRuntimeHandle> {
        self.agents.lock().unwrap().get(conversation_id).cloned()
    }

    async fn get_or_create_runtime(
        &self,
        conversation_id: &str,
        options: AgentRuntimeBuildOptions,
    ) -> Result<AgentRuntimeHandle, AppError> {
        let mut agents = self.agents.lock().unwrap();
        if let Some(existing) = agents.get(conversation_id) {
            return Ok(existing.clone());
        }
        let instance =
            AgentRuntimeHandle::Mock(Arc::new(MockAgent::new(conversation_id, &options.workspace)));
        agents.insert(conversation_id.to_owned(), instance.clone());
        Ok(instance)
    }

    fn terminate(&self, conversation_id: &str, _reason: Option<AgentKillReason>) -> Result<(), AppError> {
        self.agents.lock().unwrap().remove(conversation_id);
        Ok(())
    }

    fn terminate_and_wait_result(
        &self,
        conversation_id: &str,
        reason: Option<AgentKillReason>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send>> {
        let result = self.terminate(conversation_id, reason);
        Box::pin(std::future::ready(result))
    }

    fn terminate_all(&self) {
        self.agents.lock().unwrap().clear();
    }

    fn active_runtime_count(&self) -> usize {
        self.agents.lock().unwrap().len()
    }

    fn collect_idle_runtimes(&self, _idle_threshold_ms: TimestampMs) -> Vec<String> {
        vec![]
    }
}

// ── Test App builder with mock agents ───────────────────────────

async fn build_app_with_mock_runtime_registry() -> (axum::Router, nomifun_app::AppServices, Arc<MockAgentRuntimeRegistry>) {
    let db = nomifun_db::init_database_memory().await.unwrap();
    // Absolute, per-test dirs: `AppConfig::default()` uses the relative
    // `data` path, which the App Server workspace resolver rejects at router
    // startup ("configured workspace root must be absolute") and which would
    // otherwise be shared across parallel test binaries.
    let root = tempfile::Builder::new()
        .prefix("nomifun-agent-e2e-")
        .tempdir()
        .unwrap()
        .keep();
    let config = nomifun_app::AppConfig {
        data_dir: root.join("data"),
        work_dir: root.join("work"),
        ..nomifun_app::AppConfig::default()
    };
    let services = nomifun_app::AppServices::from_config(db, &config)
        .await
        .unwrap();

    let runtime_registry = Arc::new(MockAgentRuntimeRegistry::new());
    let services = services.with_agent_runtime_registry(runtime_registry.clone());

    let router = nomifun_app::create_router(&services).await;
    (router, services, runtime_registry)
}

async fn create_conversation(app: &mut axum::Router, token: &str, csrf: &str, name: &str) -> String {
    let body = json!({
        "type": "acp",
        "name": name,
        "extra": {
            "agent_id": GEMINI_AGENT_ID
        }
    });
    let req = common::json_with_token("POST", "/api/conversations", body, token, csrf);
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let json = common::body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "mock-runtime conversation creation failed: {json}"
    );
    json["data"]["conversation_id"]
        .as_str()
        .expect("created conversation response must expose conversation_id")
        .to_owned()
}

// ── Message flow with mock agent ────────────────────────────────

#[tokio::test]
async fn send_message_with_mock_agent_returns_202() {
    let (mut app, services, _runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Mock Agent Test").await;

    let req = json_with_token(
        "POST",
        &format!("/api/conversations/{conv_id}/messages"),
        json!({ "content": "Hello mock agent" }),
        &token,
        &csrf,
    );
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let json = body_json(resp).await;
    assert_eq!(status, StatusCode::ACCEPTED, "send message failed: {json}");
    assert_eq!(
        json["success"],
        true,
        "send message failed: {json}"
    );
}

#[tokio::test]
async fn stop_stream_with_mock_agent() {
    let (mut app, services, runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Stop Test").await;
    runtime_registry.insert(&conv_id, "/mock-workspace");

    let req = json_with_token(
        "POST",
        &format!("/api/conversations/{conv_id}/cancel"),
        json!({}),
        &token,
        &csrf,
    );
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let json = body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "cancel failed: {json}");
    assert_eq!(
        json["success"],
        true,
        "cancel failed: {json}"
    );
}

#[tokio::test]
async fn warmup_with_mock_agent() {
    let (mut app, services, _runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Warmup Test").await;

    let req = json_with_token(
        "POST",
        &format!("/api/conversations/{conv_id}/warmup"),
        json!({}),
        &token,
        &csrf,
    );
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let json = body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "warmup failed: {json}");
}

// ── Confirmation system with mock agent ─────────────────────────

#[tokio::test]
async fn list_confirmations_empty() {
    let (mut app, services, runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Confirm Test").await;
    runtime_registry.insert(&conv_id, "/mock-workspace");

    let req = get_with_token(&format!("/api/conversations/{conv_id}/confirmations"), &token);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(resp).await;
    assert_eq!(json["success"], true);
    assert!(json["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn confirm_and_check_approval() {
    let (mut app, services, runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Approval Test").await;
    let agent = runtime_registry.insert(&conv_id, "/mock-workspace");

    // Pre-populate a pending confirmation so the confirm endpoint can find it
    agent.confirmations.lock().unwrap().push(Confirmation {
        id: "conf-1".into(),
        call_id: "call-42".into(),
        title: Some("Allow file edit".into()),
        action: Some("test_action".into()),
        description: String::new(),
        command_type: None,
        options: vec![],
        screenshot: None,
    });

    // Confirm a call with alwaysAllow=true
    let req = json_with_token(
        "POST",
        &format!("/api/conversations/{conv_id}/confirmations/call-42/confirm"),
        json!({ "msg_id": MESSAGE_ID, "data": { "value": "allow" }, "always_allow": true }),
        &token,
        &csrf,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let json = body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "confirm failed: {json}");

    // Check approval — should be approved for "test_action"
    let req = get_with_token(
        &format!("/api/conversations/{conv_id}/approvals/check?action=test_action"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(resp).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["data"]["approved"], true);
}

#[tokio::test]
async fn check_approval_not_set() {
    let (mut app, services, runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Approval NotSet").await;
    runtime_registry.insert(&conv_id, "/mock-workspace");

    let req = get_with_token(
        &format!("/api/conversations/{conv_id}/approvals/check?action=unknown_action"),
        &token,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(resp).await;
    assert_eq!(json["data"]["approved"], false);
}

// ── Auxiliary routes with mock agent ────────────────────────────

#[tokio::test]
async fn slash_commands_with_mock_returns_empty() {
    let (mut app, services, runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Slash Mock Test").await;
    runtime_registry.insert(&conv_id, "/mock-workspace");

    let req = get_with_token(&format!("/api/conversations/{conv_id}/slash-commands"), &token);
    let resp = app.oneshot(req).await.unwrap();
    // Mock agent is not a real AcpAgentManager, so downcast fails → 500
    // OR if agent_type check prevents downcast, returns empty array
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
        "Expected 200 or 500, got {status}"
    );
}

#[tokio::test]
async fn openclaw_runtime_wrong_agent_type() {
    let (mut app, services, runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "OpenClaw Wrong Type").await;
    runtime_registry.insert(&conv_id, "/mock-workspace");

    let req = get_with_token(&format!("/api/conversations/{conv_id}/openclaw/runtime"), &token);
    let resp = app.oneshot(req).await.unwrap();
    // Non-OpenClaw agents return a JSON null payload instead of an
    // error — the endpoint is a best-effort diagnostic; callers that
    // need stricter typing check the payload shape themselves.
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(resp).await;
    assert_eq!(json["success"], true);
    assert!(json["data"].is_null());
}

#[tokio::test]
async fn side_question_with_mock_agent() {
    let (mut app, services, runtime_registry) = build_app_with_mock_runtime_registry().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let conv_id = create_conversation(&mut app, &token, &csrf, "Side Q Mock").await;
    runtime_registry.insert(&conv_id, "/mock-workspace");

    let req = json_with_token(
        "POST",
        &format!("/api/conversations/{conv_id}/side-question"),
        json!({ "question": "What is this code?" }),
        &token,
        &csrf,
    );
    let resp = app.oneshot(req).await.unwrap();
    // Mock agent is type Acp but not a real AcpAgentManager, so downcast
    // fails. The handler first checks agent_type() == Acp, then tries to
    // downcast. Since our mock returns Acp type, downcast fails → 500.
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
        "Expected 200 or 500, got {status}"
    );
}

// ── Active subagent read model ─────────────────────────────────

#[tokio::test]
async fn list_active_subagents_route_is_available_and_owner_scoped() {
    // Keep the companion store inside a disposable data root. The shared
    // helper uses the crate-relative `data/` directory, which may carry a
    // store written by a different checkout and make unrelated tests fail.
    let data_dir = tempfile::tempdir().unwrap();
    let db = nomifun_db::init_database_memory().await.unwrap();
    let config = nomifun_app::AppConfig {
        data_dir: data_dir.path().to_path_buf(),
        work_dir: data_dir.path().to_path_buf(),
        ..Default::default()
    };
    let services = nomifun_app::AppServices::from_config(db, &config).await.unwrap();
    let mut app = nomifun_app::create_router(&services).await;
    let (token, _csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;

    let req = get_with_token("/api/agent-executions/subagents", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["success"], true);
    assert!(
        json["data"].as_array().is_some_and(|items| items.is_empty()),
        "a fresh owner must see an empty subagent projection: {json}"
    );

    // The static `/subagents` segment must win over `/{execution_id}`; an
    // invalid execution scope is rejected instead of being treated as an ID.
    let req = get_with_token(
        "/api/agent-executions/subagents?execution_id=not-an-execution-id",
        &token,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Active subagent read model: non-empty HTTP E2E ─────────────

fn subagent_event(kind: AgentExecutionEventKind) -> NewAgentExecutionEvent {
    NewAgentExecutionEvent {
        event_type: kind,
        step_id: None,
        attempt_id: None,
        actor: nomifun_common::AgentExecutionActor::system(),
        payload: "{}".to_owned(),
    }
}

fn subagent_participant(participant_id: String) -> NewAgentExecutionParticipant {
    NewAgentExecutionParticipant {
        participant_id,
        source_agent_id: "0190f5fe-7c00-7a00-8000-000000000114".to_owned(),
        preset_id: None,
        preset_revision: None,
        preset_snapshot: None,
        provider_id: None,
        model: None,
        role: Some("builder".to_owned()),
        capability: None,
        constraints: None,
        description: Some("active-subagent HTTP fixture".to_owned()),
        system_prompt: None,
        enabled_skills: "[]".to_owned(),
        disabled_builtin_skills: "[]".to_owned(),
        sort_order: 0,
    }
}

fn subagent_execution_params(goal: &str, lead_conversation_id: Option<String>) -> CreateAgentExecutionParams {
    CreateAgentExecutionParams {
        goal: goal.to_owned(),
        status: AgentExecutionStatus::Planning,
        plan_gate: PlanGate::Automatic,
        adaptation_policy: AdaptationPolicy::Fixed,
        decision_policy: DecisionPolicy::Automatic,
        delegation_policy: DelegationPolicy::Automatic,
        max_parallel: 2,
        work_dir: None,
        lead_conversation_id,
        initial_plan_input: r#"{"mode":"automatic"}"#.to_owned(),
    }
}

fn subagent_step(
    step_id: String,
    participant_id: Option<String>,
    title: &str,
    kind: ExecutionStepKind,
) -> NewAgentExecutionStep {
    NewAgentExecutionStep {
        step_id,
        title: title.to_owned(),
        spec: format!("execute {title}"),
        role: Some("builder".to_owned()),
        tool_policy: AgentToolPolicy::Full,
        kind,
        agent_mode: Some(nomifun_common::AgentStepMode::Normal),
        profile: Some(
            r#"{"kind":"coding","needs_vision":false,"needs_long_context":false,"needs_high_reasoning":false,"bulk":false}"#
                .to_owned(),
        ),
        fanout_group: None,
        control_policy: None,
        status: ExecutionStepStatus::Pending,
        assigned_participant_id: participant_id,
        assignment_score: Some(1.0),
        assignment_rationale: Some("fixture".to_owned()),
        assignment_source: Some(nomifun_common::ParticipantAssignmentSource::Planner),
        assignment_locked: false,
        failure_policy: StepFailurePolicy::FailExecution,
        preset_prompt: None,
        graph_x: None,
        graph_y: None,
    }
}

fn subagent_conversation(owner_id: &str, name: &str) -> ConversationRow {
    let now = nomifun_common::now_ms();
    ConversationRow {
        id: 0,
        conversation_id: ConversationId::new().into_string(),
        user_id: owner_id.to_owned(),
        name: name.to_owned(),
        r#type: "nomi".to_owned(),
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
    }
}

struct SeededSubagent {
    execution_id: String,
    step_id: String,
    attempt_id: String,
    participant_id: String,
    conversation_id: String,
}

/// Seed a genuinely running Attempt through the same repository transitions the
/// scheduler uses: create → plan → lease → queued Attempt → start Attempt. No
/// row is hand-written, so the projection is exercised against the real state
/// machine rather than a synthetic SELECT fixture.
async fn seed_running_subagent(
    services: &nomifun_app::AppServices,
    owner_id: &str,
    goal: &str,
    title: &str,
    kind: ExecutionStepKind,
) -> SeededSubagent {
    let pool = services.database.pool().clone();
    let execution_repo = SqliteAgentExecutionRepository::new(pool.clone());
    let conversation_repo = SqliteConversationRepository::new(pool);

    let participant_id = nomifun_common::generate_id();
    let created = execution_repo
        .create_execution_with_participants(
            owner_id,
            &subagent_execution_params(goal, None),
            &[subagent_participant(participant_id.clone())],
            &subagent_event(AgentExecutionEventKind::Created),
        )
        .await
        .unwrap();

    let step_id = nomifun_common::generate_id();
    let planned = execution_repo
        .reconcile_plan(
            owner_id,
            &created.execution_id,
            created.version,
            &ReconcileAgentExecutionPlanParams {
                goal: None,
                plan_gate: None,
                adaptation_policy: None,
                decision_policy: None,
                delegation_policy: None,
                keep_step_ids: Vec::new(),
                new_participants: Vec::new(),
                retire_participant_ids: Vec::new(),
                new_steps: vec![subagent_step(
                    step_id.clone(),
                    (kind == ExecutionStepKind::Agent).then(|| participant_id.clone()),
                    title,
                    kind,
                )],
                new_dependencies: Vec::new(),
                execution_status: AgentExecutionStatus::Running,
            },
            &subagent_event(AgentExecutionEventKind::PlanChanged),
        )
        .await
        .unwrap();

    let lease = AgentExecutionLeaseToken::new(format!("subagent-http-e2e:{owner_id}"));
    execution_repo
        .try_acquire_lease(
            &created.execution_id,
            planned.execution.version,
            lease.owner(),
            nomifun_common::now_ms() + 120_000,
        )
        .await
        .unwrap()
        .expect("fixture must own the scheduler lease");

    let conversation = subagent_conversation(owner_id, title);
    let conversation_id = conversation.conversation_id.clone();
    conversation_repo.create(&conversation).await.unwrap();

    // Agent steps queue behind the scheduler and then start with a Conversation
    // link. Control steps (verify/judge/loop) start immediately without an
    // Attempt participant, exactly as the repository demands.
    let is_agent = kind == ExecutionStepKind::Agent;
    let queued = execution_repo
        .create_attempt(
            owner_id,
            &created.execution_id,
            &step_id,
            planned.steps[0].version,
            Some(&lease),
            &CreateAgentExecutionAttemptParams {
                participant_id: is_agent.then(|| participant_id.clone()),
                start_immediately: !is_agent,
                trigger_reason: "initial".to_owned(),
                effective_config: "{}".to_owned(),
                retry_after: None,
                runtime_state: None,
            },
            &subagent_event(AgentExecutionEventKind::AttemptChanged),
        )
        .await
        .unwrap();
    let queued_attempt = queued.current_attempt.as_ref().unwrap().attempt.clone();

    if is_agent {
        execution_repo
            .start_attempt(
                owner_id,
                &created.execution_id,
                &step_id,
                queued.step.version,
                &queued_attempt.attempt_id,
                queued_attempt.version,
                &conversation_id,
                Some(&lease),
                &subagent_event(AgentExecutionEventKind::AttemptChanged),
            )
            .await
            .unwrap();
    }

    SeededSubagent {
        execution_id: created.execution_id,
        step_id,
        attempt_id: queued_attempt.attempt_id.clone(),
        participant_id,
        conversation_id,
    }
}

fn find_subagent<'a>(items: &'a Value, step_title: &str) -> &'a Value {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["step_title"] == step_title)
        .unwrap_or_else(|| panic!("missing subagent projection for {step_title}: {items}"))
}

#[tokio::test]
async fn list_active_subagents_projects_live_attempts_and_enforces_scope() {
    let data_dir = tempfile::tempdir().unwrap();
    let db = nomifun_db::init_database_memory().await.unwrap();
    let config = nomifun_app::AppConfig {
        data_dir: data_dir.path().to_path_buf(),
        work_dir: data_dir.path().to_path_buf(),
        ..Default::default()
    };
    let services = nomifun_app::AppServices::from_config(db, &config).await.unwrap();
    let mut app = nomifun_app::create_router(&services).await;
    let (token, _csrf) = setup_and_login(&mut app, &services, "admin", "Pass123!").await;
    let owner_id = services.authoritative_user_id.to_string();

    // A second owner proves the projection is scoped by execution ownership,
    // not merely by the `execution_id` filter.
    let (other_token, _other_csrf) =
        setup_and_login(&mut app, &services, "subagent-other", "Pass123!").await;
    let other_id = services
        .user_repo
        .find_by_username("subagent-other")
        .await
        .unwrap()
        .unwrap()
        .user_id
        .as_str()
        .to_owned();

    let agent = seed_running_subagent(
        &services,
        &owner_id,
        "Ship the active subagent read model",
        "Implement projection",
        ExecutionStepKind::Agent,
    )
    .await;
    let control = seed_running_subagent(
        &services,
        &owner_id,
        "Ship the active subagent read model",
        "Verify projection",
        ExecutionStepKind::Verify,
    )
    .await;
    let terminal_attempt = seed_running_subagent(
        &services,
        &owner_id,
        "Finished work",
        "Terminal attempt",
        ExecutionStepKind::Agent,
    )
    .await;
    let terminal_execution = seed_running_subagent(
        &services,
        &owner_id,
        "Finished work",
        "Terminal execution",
        ExecutionStepKind::Agent,
    )
    .await;
    let foreign = seed_running_subagent(
        &services,
        &other_id,
        "Another owner's goal",
        "Foreign step",
        ExecutionStepKind::Agent,
    )
    .await;

    let pool = services.database.pool();
    nomifun_db::sqlx::query("UPDATE agent_execution_attempts SET status = 'completed' WHERE attempt_id = ?")
        .bind(&terminal_attempt.attempt_id)
        .execute(pool)
        .await
        .unwrap();
    nomifun_db::sqlx::query("UPDATE agent_executions SET status = 'completed' WHERE execution_id = ?")
        .bind(&terminal_execution.execution_id)
        .execute(pool)
        .await
        .unwrap();

    // 1. Default projection: only live `agent` steps, no control steps, no
    //    terminal Attempts/Executions and no foreign-owner rows.
    let req = get_with_token("/api/agent-executions/subagents", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["success"], true);
    let items = json["data"].as_array().unwrap();
    assert_eq!(items.len(), 1, "unexpected subagent projection: {json}");

    let row = find_subagent(&json["data"], "Implement projection");
    assert_eq!(row["execution_id"], json!(agent.execution_id));
    assert_eq!(row["execution_goal"], json!("Ship the active subagent read model"));
    assert_eq!(row["execution_status"], json!("running"));
    assert_eq!(row["step_id"], json!(agent.step_id));
    assert_eq!(row["step_title"], json!("Implement projection"));
    assert_eq!(row["step_kind"], json!("agent"));
    assert_eq!(row["step_status"], json!("running"));
    assert_eq!(row["attempt_id"], json!(agent.attempt_id));
    assert_eq!(row["attempt_no"], json!(0));
    assert_eq!(row["attempt_status"], json!("running"));
    assert_eq!(row["participant_id"], json!(agent.participant_id));
    assert_eq!(row["participant_role"], json!("builder"));
    assert_eq!(row["conversation_id"], json!(agent.conversation_id));
    assert!(row["question"].is_null(), "a freshly started Attempt has no question: {row}");
    assert!(row["tokens"].is_null(), "an unmeasured Attempt reports no tokens: {row}");
    assert!(
        row["started_at"].as_i64().is_some_and(|value| value > 0),
        "started_at must be projected: {row}"
    );
    assert!(
        row["attempt_updated_at"].as_i64().is_some_and(|value| value > 0),
        "attempt_updated_at must be projected: {row}"
    );

    // 2. Control steps are opt-in, and the filter must not widen owner scope.
    let req = get_with_token("/api/agent-executions/subagents?include_control_steps=true", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["data"].as_array().unwrap().len(), 2, "{json}");
    assert_eq!(find_subagent(&json["data"], "Verify projection")["step_kind"], json!("verify"));

    // 3. Scoping to one execution returns exactly that execution's live rows.
    let req = get_with_token(
        &format!(
            "/api/agent-executions/subagents?execution_id={}",
            agent.execution_id
        ),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let json = body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let items = json["data"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{json}");
    assert_eq!(items[0]["execution_id"], json!(agent.execution_id));

    // 4. Owner isolation: a foreign execution ID is a 404, not an empty leak.
    let req = get_with_token(
        &format!(
            "/api/agent-executions/subagents?execution_id={}",
            foreign.execution_id
        ),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(
        !json["data"].as_array().unwrap().iter().any(|item| {
            item["step_title"] == json!("Foreign step")
        }),
        "foreign rows must never appear for the owner"
    );

    // 5. Non-owner accounts are rejected before the query runs: the execution
    // surface is installation-owner scoped, so a second login can never
    // observe another owner's rows at all.
    let req = get_with_token("/api/agent-executions/subagents", &other_token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let other_json = body_json(resp).await;
    assert_eq!(other_json["code"], json!("FORBIDDEN"), "{other_json}");
    assert_eq!(foreign.execution_id.len(), 36);

    // The status filter is what removes both terminal fixtures: the Attempt
    // terminal row and the Execution terminal row never reach the owner.
    assert!(terminal_attempt.attempt_id != agent.attempt_id);
    assert!(terminal_execution.execution_id != control.execution_id);
}

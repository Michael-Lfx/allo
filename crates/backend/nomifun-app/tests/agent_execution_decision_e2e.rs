//! Approval decision gate through the App Server runtime adapter (R8 · W2).
//!
//! `AgentRuntimeAdapter::answer_decision` must stay a transparent, owner-scoped
//! pass-through of `AgentExecutionEngine::answer_decision`, which is the single
//! answer gate. These tests pin the four refusals that gate owns — foreign
//! owner, stale CAS version, non-`WaitingInput` attempt, empty answer — and the
//! projected decision context (`approval.requested` carrying step/attempt ids
//! plus the three CAS versions) that a client needs to construct an answer the
//! gate will accept.
//!
//! The engine under test is built by the application's own state assembly over
//! the real in-memory SQLite schema, so the adapter, the repository CAS and the
//! event projection are exercised on production wiring rather than a double.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use tower::ServiceExt;

use common::{body_json, build_app, setup_and_login};

use nomifun_agent_execution::{AgentExecutionEngine, AgentRuntimeAdapter, RuntimeAdapterError};
use nomifun_api_types::AnswerExecutionDecisionRequest;
use nomifun_common::{
    AdaptationPolicy, AgentExecutionActor, AgentExecutionEventKind, AgentExecutionStatus, AppError,
    ConversationId, DecisionPolicy, DelegationPolicy, ExecutionAttemptStatus, ExecutionStepStatus,
    PlanGate,
};
use nomifun_db::models::ConversationRow;
use nomifun_db::{
    AgentExecutionLeaseToken, CreateAgentExecutionAttemptParams, CreateAgentExecutionParams,
    IAgentExecutionRepository, IConversationRepository, NewAgentExecutionEvent,
    NewAgentExecutionParticipant, NewAgentExecutionStep, ReconcileAgentExecutionPlanParams,
    SettleAgentExecutionAttemptParams, SqliteAgentExecutionRepository, SqliteConversationRepository,
};

const PROVIDER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000013";
const SOURCE_AGENT_ID: &str = "0190f5fe-7c00-7a00-8000-000000000114";
/// A valid id that is deliberately *not* the installation owner.
const FOREIGN_OWNER: &str = "0190f5fe-7c00-7a00-8000-0000000000f0";
const PROTOCOL_VERSION: &str = "2026-08-26";

// ── HTTP: capability advertisement ─────────────────────────────────────────

fn bearer_json(
    uri: &str,
    body: serde_json::Value,
    token: &str,
    csrf: &str,
) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).unwrap(),
        ))
        .unwrap()
}

/// ④ `capabilities.approvals` must be true on a production assembly (which owns
/// a runtime) — the whole point of flipping the bit is that the answer path is
/// actually reachable on this connection.
#[tokio::test]
async fn initialize_advertises_approvals_when_the_runtime_is_present() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;

    let response = app
        .clone()
        .oneshot(bearer_json(
            "/api/app-server/initialize",
            serde_json::json!({
                "protocol_version": PROTOCOL_VERSION,
                "client": { "name": "approval-e2e", "version": "1" },
                "capabilities": {},
            }),
            &token,
            &csrf,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let init = body_json(response).await;
    assert_eq!(
        init["capabilities"]["approvals"],
        serde_json::json!(true),
        "the runtime-backed assembly must advertise the approval answer path"
    );
    assert_eq!(init["capabilities"]["agents"], serde_json::json!(true));
}

// ── Fixtures ───────────────────────────────────────────────────────────────

struct DecisionFixture {
    adapter: AgentRuntimeAdapter,
    execution_id: String,
    step_id: String,
    attempt_id: String,
    owner_id: String,
    execution_version: i64,
    step_version: i64,
    attempt_version: i64,
}

impl DecisionFixture {
    fn request(&self, answer: &str) -> AnswerExecutionDecisionRequest {
        AnswerExecutionDecisionRequest {
            answer: answer.to_owned(),
            expected_execution_version: self.execution_version,
            expected_step_version: self.step_version,
            expected_attempt_version: self.attempt_version,
        }
    }
}

async fn seed_provider(pool: &nomifun_db::sqlx::SqlitePool) {
    nomifun_db::sqlx::query(
        "INSERT INTO providers (\
            provider_id, platform, name, base_url, api_key_encrypted, enabled, \
            created_at, updated_at\
         ) VALUES (?, 'openai', 'approval fixture', 'https://example.invalid', \
                   'encrypted', 1, 1, 1)",
    )
    .bind(PROVIDER_ID)
    .execute(pool)
    .await
    .unwrap();
}

fn event(
    kind: nomifun_common::AgentExecutionEventKind,
    step_id: Option<&str>,
    attempt_id: Option<&str>,
) -> NewAgentExecutionEvent {
    NewAgentExecutionEvent {
        event_type: kind,
        step_id: step_id.map(str::to_owned),
        attempt_id: attempt_id.map(str::to_owned),
        actor: AgentExecutionActor::system(),
        payload: "{}".to_owned(),
    }
}

fn participant() -> NewAgentExecutionParticipant {
    NewAgentExecutionParticipant {
        participant_id: nomifun_common::generate_id(),
        source_agent_id: SOURCE_AGENT_ID.to_owned(),
        preset_id: None,
        preset_revision: None,
        preset_snapshot: None,
        provider_id: Some(PROVIDER_ID.to_owned()),
        model: Some("model_test".to_owned()),
        role: Some("builder".to_owned()),
        capability: None,
        constraints: None,
        description: Some("approval fixture participant".to_owned()),
        system_prompt: None,
        enabled_skills: "[]".to_owned(),
        disabled_builtin_skills: "[]".to_owned(),
        sort_order: 0,
    }
}

fn step(step_id: String, participant_id: String) -> NewAgentExecutionStep {
    NewAgentExecutionStep {
        step_id,
        title: "decide".to_owned(),
        spec: "ask the owner before continuing".to_owned(),
        role: Some("builder".to_owned()),
        tool_policy: nomifun_common::AgentToolPolicy::Full,
        kind: nomifun_common::ExecutionStepKind::Agent,
        agent_mode: Some(nomifun_common::AgentStepMode::Normal),
        profile: Some(
            r#"{"kind":"general","needs_vision":false,"needs_long_context":false,"needs_high_reasoning":false,"bulk":false}"#
                .to_owned(),
        ),
        fanout_group: None,
        control_policy: None,
        status: ExecutionStepStatus::Pending,
        assigned_participant_id: Some(participant_id),
        assignment_score: Some(1.0),
        assignment_rationale: Some("fixture".to_owned()),
        assignment_source: Some(nomifun_common::ParticipantAssignmentSource::Planner),
        assignment_locked: false,
        failure_policy: nomifun_common::StepFailurePolicy::FailExecution,
        preset_prompt: None,
        graph_x: None,
        graph_y: None,
    }
}

fn conversation_row(owner_id: &str) -> ConversationRow {
    let now = nomifun_common::now_ms();
    ConversationRow {
        id: 0,
        conversation_id: ConversationId::new().into_string(),
        user_id: owner_id.to_owned(),
        name: "Approval fixture conversation".to_owned(),
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

/// Seed one execution whose attempt is either `waiting_input` (ready for an
/// answer) or still `running` (the state that must be refused).
///
/// The fixture holds the scheduler lease for the whole test so the engine's own
/// scheduler cannot take over the execution while assertions run.
async fn decision_fixture(
    engine: Arc<AgentExecutionEngine>,
    pool: nomifun_db::sqlx::SqlitePool,
    owner_id: &str,
    wait_for_input: bool,
) -> DecisionFixture {
    let repository = SqliteAgentExecutionRepository::new(pool.clone());
    let conversations = SqliteConversationRepository::new(pool.clone());
    let created = repository
        .create_execution_with_participants(
            owner_id,
            &CreateAgentExecutionParams {
                goal: "verify the approval decision gate".to_owned(),
                status: AgentExecutionStatus::Planning,
                plan_gate: PlanGate::Automatic,
                adaptation_policy: AdaptationPolicy::Fixed,
                decision_policy: DecisionPolicy::AskUser,
                delegation_policy: DelegationPolicy::Automatic,
                max_parallel: 1,
                work_dir: None,
                lead_conversation_id: None,
                initial_plan_input: r#"{"mode":"automatic"}"#.to_owned(),
            },
            &[participant()],
            &event(AgentExecutionEventKind::Created, None, None),
        )
        .await
        .unwrap();
    let participant_id = repository
        .get_execution_detail(owner_id, &created.execution_id)
        .await
        .unwrap()
        .expect("execution exists")
        .participants[0]
        .participant_id
        .clone();
    let step_id = nomifun_common::generate_id();
    let planned = repository
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
                new_steps: vec![step(step_id.clone(), participant_id.clone())],
                new_dependencies: Vec::new(),
                execution_status: AgentExecutionStatus::Running,
            },
            &event(AgentExecutionEventKind::PlanChanged, None, None),
        )
        .await
        .unwrap();
    let lease = AgentExecutionLeaseToken::new("approval-fixture".to_owned());
    let expiry = nomifun_common::now_ms() + 600_000;
    repository
        .try_acquire_lease(
            &created.execution_id,
            planned.execution.version,
            lease.owner(),
            expiry,
        )
        .await
        .unwrap()
        .expect("fixture lease");
    let conversation = conversation_row(owner_id);
    let conversation_id = conversation.conversation_id.clone();
    conversations.create(&conversation).await.unwrap();
    let queued = repository
        .create_attempt(
            owner_id,
            &created.execution_id,
            &step_id,
            planned.steps[0].version,
            Some(&lease),
            &CreateAgentExecutionAttemptParams {
                participant_id: Some(participant_id),
                start_immediately: false,
                trigger_reason: "initial".to_owned(),
                effective_config: "{}".to_owned(),
                retry_after: None,
                runtime_state: None,
            },
            &event(AgentExecutionEventKind::AttemptChanged, None, None),
        )
        .await
        .unwrap();
    let attempt_id = queued
        .current_attempt
        .as_ref()
        .expect("queued attempt")
        .attempt
        .attempt_id
        .clone();
    let running = repository
        .start_attempt(
            owner_id,
            &created.execution_id,
            &step_id,
            queued.step.version,
            &attempt_id,
            queued.current_attempt.as_ref().unwrap().attempt.version,
            &conversation_id,
            Some(&lease),
            &event(AgentExecutionEventKind::AttemptChanged, None, None),
        )
        .await
        .unwrap();
    let mut step_version = running.step.version;
    let mut attempt_version = running
        .current_attempt
        .as_ref()
        .expect("running attempt")
        .attempt
        .version;
    if wait_for_input {
        let waiting = repository
            .settle_attempt(
                owner_id,
                &created.execution_id,
                &step_id,
                step_version,
                &attempt_id,
                attempt_version,
                Some(&lease),
                &SettleAgentExecutionAttemptParams {
                    attempt_status: ExecutionAttemptStatus::WaitingInput,
                    step_status: ExecutionStepStatus::WaitingInput,
                    execution_status: Some(AgentExecutionStatus::WaitingInput),
                    question: Some(Some("Continue with the deployment?".to_owned())),
                    error: None,
                    output_summary: None,
                    output_files: None,
                    tokens: None,
                    retry_after: None,
                    runtime_state: None,
                    started_at: None,
                    finished_at: None,
                    loop_repeat_reset: None,
                },
                &event(
                    AgentExecutionEventKind::DecisionRequested,
                    Some(&step_id),
                    Some(&attempt_id),
                ),
            )
            .await
            .unwrap();
        step_version = waiting.step.version;
        attempt_version = waiting
            .current_attempt
            .as_ref()
            .expect("waiting attempt")
            .attempt
            .version;
    }
    let execution_version = repository
        .get_execution(owner_id, &created.execution_id)
        .await
        .unwrap()
        .expect("execution row")
        .version;
    DecisionFixture {
        adapter: AgentRuntimeAdapter::new(engine),
        execution_id: created.execution_id,
        step_id,
        attempt_id,
        owner_id: owner_id.to_owned(),
        execution_version,
        step_version,
        attempt_version,
    }
}

/// Build the production state bundle and take the adapter the App Server mounts.
///
/// Returns `(adapter, engine, installation owner)`.
async fn app_engine(
    services: &nomifun_app::AppServices,
) -> (AgentRuntimeAdapter, Arc<AgentExecutionEngine>, String) {
    let (states, _) = nomifun_app::build_module_states(services).await;
    let engine = states.agent_execution.clone();
    (
        AgentRuntimeAdapter::new(engine.clone()),
        engine,
        services.authoritative_user_id.to_string(),
    )
}

/// Seed the fixture that every gate test starts from.
async fn seeded_fixture(
    services: &nomifun_app::AppServices,
    wait_for_input: bool,
) -> DecisionFixture {
    let (_, engine, owner) = app_engine(services).await;
    seed_provider(services.database.pool()).await;
    decision_fixture(engine, services.database.pool().clone(), &owner, wait_for_input).await
}

fn expect_refusal(result: Result<impl std::fmt::Debug, RuntimeAdapterError>, want: &str) -> String {
    match result {
        Ok(view) => panic!("expected {want} refusal, got a successful answer: {view:?}"),
        Err(RuntimeAdapterError::Runtime(error)) => {
            let (kind, message) = match &error {
                AppError::NotFound(message) => ("not_found", message.clone()),
                AppError::Conflict(message) => ("conflict", message.clone()),
                AppError::BadRequest(message) => ("bad_request", message.clone()),
                other => panic!("expected {want} refusal, got a different error: {other:?}"),
            };
            assert_eq!(kind, want, "wrong refusal kind: {message}");
            message
        }
        Err(other) => panic!("expected {want} refusal, got an adapter error: {other:?}"),
    }
}

// ── ① The four refusals ────────────────────────────────────────────────────

#[tokio::test]
async fn foreign_owner_cannot_reach_a_waiting_attempt() {
    let (_, services) = build_app().await;
    let fixture = seeded_fixture(&services, true).await;
    let adapter = fixture.adapter.clone();

    let error = expect_refusal(
        adapter
            .answer_decision(
                FOREIGN_OWNER,
                &fixture.execution_id,
                &fixture.step_id,
                &fixture.attempt_id,
                fixture.request("yes"),
            )
            .await,
        "not_found",
    );
    assert!(
        !error.contains(&fixture.step_id),
        "a foreign owner must not learn anything about the attempt: {error}"
    );

    // The refusal is a read-side miss, not a mutation: the owner can still answer.
    let answered = fixture
        .adapter
        .answer_decision(
            &fixture.owner_id,
            &fixture.execution_id,
            &fixture.step_id,
            &fixture.attempt_id,
            fixture.request("yes"),
        )
        .await
        .expect("the owner still holds the gate");
    assert_eq!(answered.run_id, fixture.execution_id);
}

#[tokio::test]
async fn stale_cas_versions_are_refused_without_mutating_state() {
    let (_, services) = build_app().await;
    let fixture = seeded_fixture(&services, true).await;

    let stale_execution = AnswerExecutionDecisionRequest {
        answer: "yes".to_owned(),
        expected_execution_version: fixture.execution_version - 1,
        ..fixture.request("yes")
    };
    let stale_step = AnswerExecutionDecisionRequest {
        answer: "yes".to_owned(),
        expected_step_version: fixture.step_version - 1,
        ..fixture.request("yes")
    };
    let stale_attempt = AnswerExecutionDecisionRequest {
        answer: "yes".to_owned(),
        expected_attempt_version: fixture.attempt_version - 1,
        ..fixture.request("yes")
    };
    for (label, request) in [
        ("stale execution version", stale_execution),
        ("stale step version", stale_step),
        ("stale attempt version", stale_attempt),
    ] {
        expect_refusal(
            fixture
                .adapter
                .answer_decision(
                    &fixture.owner_id,
                    &fixture.execution_id,
                    &fixture.step_id,
                    &fixture.attempt_id,
                    request,
                )
                .await,
            "conflict",
        );
        // A refused CAS must leave the attempt exactly where it was.
        let view = fixture
            .adapter
            .get_run(&fixture.owner_id, &fixture.execution_id)
            .await
            .unwrap();
        assert_eq!(
            view.status,
            nomifun_agent_execution::AgentRunStatus::WaitingInput,
            "{label} must not move the run"
        );
        assert_eq!(
            view.version, fixture.execution_version,
            "{label} must not advance the aggregate version"
        );
    }

    // ...and the gate is still open for the current versions.
    let answered = fixture
        .adapter
        .answer_decision(
            &fixture.owner_id,
            &fixture.execution_id,
            &fixture.step_id,
            &fixture.attempt_id,
            fixture.request("yes"),
        )
        .await
        .expect("current versions are still accepted");
    assert!(
        !answered.status.is_terminal(),
        "the run resumes; it does not finish at the answer"
    );
}

#[tokio::test]
async fn non_waiting_attempt_cannot_be_answered() {
    let (_, services) = build_app().await;
    let fixture = seeded_fixture(&services, false).await;

    let error = expect_refusal(
        fixture
            .adapter
            .answer_decision(
                &fixture.owner_id,
                &fixture.execution_id,
                &fixture.step_id,
                &fixture.attempt_id,
                fixture.request("lying about a pending question"),
            )
            .await,
        "conflict",
    );
    assert!(
        error.contains("waiting attempt"),
        "the refusal must name the waiting-attempt precondition: {error}"
    );
}

#[tokio::test]
async fn empty_answer_is_rejected_as_bad_request() {
    let (_, services) = build_app().await;
    let fixture = seeded_fixture(&services, true).await;

    for blank in ["", "   ", "\n\t"] {
        expect_refusal(
            fixture
                .adapter
                .answer_decision(
                    &fixture.owner_id,
                    &fixture.execution_id,
                    &fixture.step_id,
                    &fixture.attempt_id,
                    fixture.request(blank),
                )
                .await,
            "bad_request",
        );
    }
    assert_eq!(
        fixture
            .adapter
            .get_run(&fixture.owner_id, &fixture.execution_id)
            .await
            .unwrap()
            .status,
        nomifun_agent_execution::AgentRunStatus::WaitingInput
    );
}

// ── ③ The projected decision context ───────────────────────────────────────

#[tokio::test]
async fn approval_requested_projects_ids_and_the_three_cas_versions() {
    let (_, services) = build_app().await;
    let fixture = seeded_fixture(&services, true).await;

    let events = fixture
        .adapter
        .list_events(&fixture.owner_id, &fixture.execution_id, None, Some(200))
        .await
        .unwrap();
    let requested = events
        .iter()
        .find(|event| event.event_type == "approval.requested")
        .expect("the decision request is on the wire");
    assert_eq!(requested.step_id.as_deref(), Some(fixture.step_id.as_str()));
    assert_eq!(
        requested.attempt_id.as_deref(),
        Some(fixture.attempt_id.as_str())
    );
    assert_eq!(
        requested.expected_execution_version,
        Some(fixture.execution_version)
    );
    assert_eq!(requested.expected_step_version, Some(fixture.step_version));
    assert_eq!(
        requested.expected_attempt_version,
        Some(fixture.attempt_version)
    );

    // An event that is not a decision request stays a pure row projection.
    let created = events
        .iter()
        .find(|event| event.event_type == "run.started")
        .expect("the created event is on the wire");
    assert!(created.expected_execution_version.is_none());
    assert!(created.step_id.is_none());

    // The projected versions are exactly what the gate accepts, and the answer
    // is then visible as `approval.responded` on the same stream.
    fixture
        .adapter
        .answer_decision(
            &fixture.owner_id,
            &fixture.execution_id,
            &fixture.step_id,
            &fixture.attempt_id,
            AnswerExecutionDecisionRequest {
                answer: "  approved  ".to_owned(),
                expected_execution_version: requested.expected_execution_version.unwrap(),
                expected_step_version: requested.expected_step_version.unwrap(),
                expected_attempt_version: requested.expected_attempt_version.unwrap(),
            },
        )
        .await
        .expect("the projected context is accepted as-is");

    let after = fixture
        .adapter
        .list_events(&fixture.owner_id, &fixture.execution_id, None, Some(200))
        .await
        .unwrap();
    let responded = after
        .iter()
        .find(|event| event.event_type == "approval.responded")
        .expect("the answer is a durable event");
    assert_eq!(responded.step_id.as_deref(), Some(fixture.step_id.as_str()));
    assert_eq!(
        responded.attempt_id.as_deref(),
        Some(fixture.attempt_id.as_str())
    );

    // The projection is read-time authoritative: replaying the same
    // `approval.requested` event after it was answered re-projects the *moved*
    // versions and hits the `WaitingInput` precondition, so an answered decision
    // can never be applied twice.
    let replay = after
        .iter()
        .find(|event| event.event_type == "approval.requested")
        .expect("the request stays in history");
    expect_refusal(
        fixture
            .adapter
            .answer_decision(
                &fixture.owner_id,
                &fixture.execution_id,
                &fixture.step_id,
                &fixture.attempt_id,
                AnswerExecutionDecisionRequest {
                    answer: "approved again".to_owned(),
                    expected_execution_version: replay.expected_execution_version.unwrap(),
                    expected_step_version: replay.expected_step_version.unwrap(),
                    expected_attempt_version: replay.expected_attempt_version.unwrap(),
                },
            )
            .await,
        "conflict",
    );
}

// ── Degenerate inputs ──────────────────────────────────────────────────────

#[tokio::test]
async fn malformed_ids_are_rejected_before_the_lookup() {
    let (_, services) = build_app().await;
    let fixture = seeded_fixture(&services, true).await;
    let adapter = fixture.adapter.clone();

    expect_refusal(
        adapter
            .answer_decision(
                &fixture.owner_id,
                &fixture.execution_id,
                "not-a-step-id",
                &fixture.attempt_id,
                fixture.request("yes"),
            )
            .await,
        "bad_request",
    );
    expect_refusal(
        adapter
            .answer_decision(
                &fixture.owner_id,
                &fixture.execution_id,
                &fixture.step_id,
                "not-an-attempt-id",
                fixture.request("yes"),
            )
            .await,
        "bad_request",
    );
    assert_eq!(
        fixture
            .adapter
            .get_run(&fixture.owner_id, &fixture.execution_id)
            .await
            .unwrap()
            .status,
        nomifun_agent_execution::AgentRunStatus::WaitingInput,
        "a malformed id must not consume the pending decision"
    );
}

// ── ⑥ Plan projection (W4 / W6 · D-W6-1) ───────────────────────────────────

/// The plan snapshot is the only place the wire carries step **titles**,
/// per-attempt **reasons**, **errors** and **timings** — the event log is an
/// append-only record of markers. This test pins that the projection reads the
/// engine's authoritative rows, keeps the owner scope identical to `run/get`,
/// and does not smuggle internal ids onto the wire.
#[tokio::test]
async fn plan_snapshot_carries_titles_statuses_and_member_attribution() {
    let (_, services) = build_app().await;
    let fixture = seeded_fixture(&services, true).await;

    let plan = fixture
        .adapter
        .plan(&fixture.owner_id, &fixture.execution_id)
        .await
        .expect("the owner can read the plan snapshot");
    assert_eq!(plan.run_id, fixture.execution_id);
    assert_eq!(plan.version, fixture.execution_version);

    let step = plan
        .steps
        .iter()
        .find(|step| step.step_id == fixture.step_id)
        .expect("the seeded step is in the plan");
    // Title / kind / status come from the engine row, not from any event marker.
    assert_eq!(step.title, "decide");
    assert_eq!(step.kind.as_str(), "agent");
    assert_eq!(step.status, ExecutionStepStatus::WaitingInput);
    // Member attribution is human-readable, sourced from the participant row.
    assert_eq!(step.role.as_deref(), Some("builder"));
    assert_eq!(step.model.as_deref(), Some("model_test"));

    assert_eq!(step.attempts.len(), 1, "the fixture seeded exactly one attempt");
    let attempt = &step.attempts[0];
    assert_eq!(attempt.attempt_id, fixture.attempt_id);
    // 引擎的 `attempt_no` 从 0 起（这里是第 0 次 = 首次尝试）。界面另有展示序号，
    // 见 `web/src/lib/run-plan.ts` 的 `ordinal`——两者不混。
    assert_eq!(attempt.attempt_no, 0);
    assert_eq!(attempt.status, ExecutionAttemptStatus::WaitingInput);
    assert_eq!(attempt.trigger_reason, "initial");
    assert_eq!(attempt.role.as_deref(), Some("builder"));
    // 审批问题也在快照上（不必从事件里翻）。
    assert_eq!(attempt.question.as_deref(), Some("Continue with the deployment?"));

    // No new internal ids on the wire: participant ids and source agent ids stay
    // inside the engine (the plan is a projection, not a dump).
    let encoded = serde_json::to_string(&plan).expect("the plan serializes");
    assert!(
        !encoded.contains("participant_id") && !encoded.contains(SOURCE_AGENT_ID),
        "the plan must not leak internal identities: {encoded}"
    );

    // Same owner scope as `run/get`: a foreign owner gets NotFound and learns
    // nothing about the run.
    let error = expect_refusal(
        fixture.adapter.plan(FOREIGN_OWNER, &fixture.execution_id).await,
        "not_found",
    );
    assert!(
        !error.contains(&fixture.step_id),
        "a foreign owner must not learn anything about the plan: {error}"
    );
}

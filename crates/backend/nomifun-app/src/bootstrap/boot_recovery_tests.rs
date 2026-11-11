//! Exercise the real AppServices -> create_router composition with a reopened
//! file database and retained OS lock, without injecting a terminal-proof provider.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nomifun_common::{AgentType, ConversationId, MessageId, now_ms};
use nomifun_db::models::{ConversationRow, MessageRow};
use nomifun_db::{
    CreateAcpSessionParams, Database, IAcpSessionRepository, IConversationRepository,
    SqliteAcpSessionRepository, SqliteConversationRepository,
};
use tower::ServiceExt;

use super::server_lock::acquire_server_lock;
use crate::{AppConfig, AppServices, create_router};

struct InterruptedTurn {
    conversation_id: String,
    operation_id: String,
    input_id: String,
    thinking_id: String,
    interrupted: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct PersistedTurn {
    status: Option<String>,
    epoch: i64,
    operation_id: Option<String>,
    receipt_status: String,
    result_ok: Option<bool>,
    error_code: Option<String>,
    input: String,
    thinking: String,
    thinking_status: Option<String>,
    session: Option<String>,
}

async fn seed_turn(
    database: &Database,
    owner: &str,
    backend: AgentType,
    interrupted: bool,
) -> InterruptedTurn {
    let repo = SqliteConversationRepository::new(database.pool().clone());
    let conversation_id = ConversationId::new().into_string();
    let now = now_ms();
    repo.create(&ConversationRow {
        id: 0,
        conversation_id: conversation_id.clone(),
        user_id: owner.to_owned(),
        name: "Boot recovery".to_owned(),
        r#type: backend.serde_name().to_owned(),
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
    })
    .await
    .unwrap();
    let input_id = MessageId::new().into_string();
    let operation_id = format!("public-turn:v1:{owner}:{conversation_id}:before-restart");
    let initial = repo
        .get_turn_admission_state(owner, &conversation_id)
        .await
        .unwrap();
    let claim = repo
        .claim_turn_delivery_receipt_and_admit_with_candidate(
            owner,
            &conversation_id,
            &operation_id,
            &input_id,
            r#"{"content":"keep my original request"}"#,
            initial.epoch,
            now,
        )
        .await
        .unwrap();
    assert!(claim.claimed_new);
    repo.insert_message(&MessageRow {
        id: 0,
        message_id: input_id.clone(),
        conversation_id: conversation_id.clone(),
        msg_id: None,
        r#type: "text".to_owned(),
        content: r#"{"content":"keep my original request"}"#.to_owned(),
        position: Some("right".to_owned()),
        status: None,
        hidden: false,
        created_at: now,
    })
    .await
    .unwrap();
    let thinking_id = MessageId::new().into_string();
    repo.insert_message(&MessageRow {
        id: 0,
        message_id: thinking_id.clone(),
        conversation_id: conversation_id.clone(),
        msg_id: Some(input_id.clone()),
        r#type: "thinking".to_owned(),
        content: serde_json::json!({
            "content": "keep the accumulated reasoning",
            "status": if interrupted { "streaming" } else { "done" },
        })
        .to_string(),
        position: Some("left".to_owned()),
        status: Some(if interrupted { "work" } else { "finish" }.to_owned()),
        hidden: false,
        created_at: now,
    })
    .await
    .unwrap();
    let sessions = SqliteAcpSessionRepository::new(database.pool().clone());
    sessions
        .create(&CreateAcpSessionParams {
            conversation_id: &conversation_id,
            agent_backend: "gemini",
            agent_source: "builtin",
            agent_id: "",
        })
        .await
        .unwrap();
    sessions
        .update_session_id(&conversation_id, "session-before-restart")
        .await
        .unwrap();
    InterruptedTurn {
        conversation_id,
        operation_id,
        input_id,
        thinking_id,
        interrupted,
    }
}

async fn persisted_turn(database: &Database, owner: &str, turn: &InterruptedTurn) -> PersistedTurn {
    let repo = SqliteConversationRepository::new(database.pool().clone());
    let row = repo.get(&turn.conversation_id).await.unwrap().unwrap();
    let admission = repo
        .get_turn_admission_state(owner, &turn.conversation_id)
        .await
        .unwrap();
    let receipt = repo
        .get_delivery_receipt(owner, &turn.conversation_id, &turn.operation_id)
        .await
        .unwrap()
        .unwrap();
    let input = repo
        .get_message(&turn.conversation_id, &turn.input_id)
        .await
        .unwrap()
        .unwrap();
    let thinking = repo
        .get_message(&turn.conversation_id, &turn.thinking_id)
        .await
        .unwrap()
        .unwrap();
    let session = SqliteAcpSessionRepository::new(database.pool().clone())
        .get(&turn.conversation_id)
        .await
        .unwrap()
        .unwrap();
    PersistedTurn {
        status: row.status,
        epoch: admission.epoch,
        operation_id: admission.active_operation_id,
        receipt_status: receipt.status,
        result_ok: receipt.result_ok,
        error_code: receipt.result_error_code,
        input: input.content,
        thinking: thinking.content,
        thinking_status: thinking.status,
        session: session.acp_session_id,
    }
}

#[derive(Clone, Copy)]
enum Authority {
    Retained,
    Missing,
    UnreadableRegistry,
}

async fn boot_composition(authority: Authority) {
    let root = tempfile::tempdir().unwrap();
    let config = AppConfig {
        data_dir: root.path().join("data"),
        work_dir: root.path().join("work"),
        ..AppConfig::default()
    };
    let previous_lock = acquire_server_lock(&config.data_dir).unwrap();
    let old_database = nomifun_db::init_database(&config.database_path())
        .await
        .unwrap();
    let owner = nomifun_db::installation_owner_id(old_database.pool())
        .await
        .unwrap();
    let mut turns = Vec::new();
    for (backend, interrupted) in [
        (AgentType::Nomi, true),
        (AgentType::Acp, true),
        (AgentType::Remote, true),
        (AgentType::OpenclawGateway, true),
        (AgentType::Acp, false),
    ] {
        turns.push(seed_turn(&old_database, &owner, backend, interrupted).await);
    }
    let mut before = Vec::new();
    for turn in &turns {
        before.push(persisted_turn(&old_database, &owner, turn).await);
    }
    old_database.close().await;
    drop(previous_lock);

    // Reacquire real ownership and reopen the previous boot's file database.
    let lock = Arc::new(acquire_server_lock(&config.data_dir).unwrap());
    if matches!(authority, Authority::UnreadableRegistry) {
        std::fs::write(
            config.data_dir.join("agent-process-registry.json"),
            "invalid registry",
        )
        .unwrap();
    }
    let database = nomifun_db::init_database(&config.database_path())
        .await
        .unwrap();
    let services = AppServices::from_config(database, &config).await.unwrap();
    let mut after_services = Vec::new();
    for turn in &turns {
        after_services.push(persisted_turn(&services.database, &owner, turn).await);
    }
    let services = if matches!(authority, Authority::Missing) {
        services
    } else {
        services
            .with_boot_reconciliation_authority(lock.boot_authority(), &config)
            .await
            .unwrap()
    };
    let router = tokio::time::timeout(Duration::from_secs(30), create_router(&services))
        .await
        .expect("real router startup must finish without running a model");
    let response = router
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let response_status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let active_runtimes = services.agent_runtime_registry.active_runtime_count();
    let mut after_router = Vec::new();
    for turn in &turns {
        after_router.push(persisted_turn(&services.database, &owner, turn).await);
    }
    services.shutdown_browser_platform().await.unwrap();
    services.database.close().await;
    drop(services);
    drop(lock);

    let reopened_lock = acquire_server_lock(&config.data_dir).unwrap();
    let reopened = nomifun_db::init_database(&config.database_path())
        .await
        .unwrap();
    let mut durable = Vec::new();
    for turn in &turns {
        durable.push(persisted_turn(&reopened, &owner, turn).await);
    }
    reopened.close().await;
    drop(reopened_lock);

    assert_eq!(
        response_status,
        StatusCode::OK,
        "the real router was published"
    );
    assert_eq!(health["status"], "ok");
    assert_eq!(
        active_runtimes, 0,
        "startup recovery must not execute a model turn"
    );
    for state in &before {
        assert_eq!(state.status.as_deref(), Some("running"));
        assert_eq!(state.receipt_status, "accepted");
        assert_eq!(state.result_ok, None);
        assert!(state.operation_id.is_some());
        assert_eq!(state.session.as_deref(), Some("session-before-restart"));
    }
    assert_eq!(
        after_services, before,
        "AppServices must not settle turns before boot proof exists"
    );
    assert_eq!(
        durable, after_router,
        "all recovery writes must survive a file reopen"
    );
    for (index, ((turn, old), current)) in turns.iter().zip(&before).zip(&durable).enumerate() {
        // Remote and unregistered gateway work can outlive this host.
        let proven_local = matches!(authority, Authority::Retained) && matches!(index, 0 | 1 | 4);
        if !proven_local {
            assert_eq!(
                current, old,
                "unproven turn {index} must keep every artifact"
            );
            continue;
        }
        assert_eq!(current.status.as_deref(), Some("finished"));
        assert_eq!(current.epoch, old.epoch + 1);
        assert_eq!(current.operation_id, None);
        assert_eq!(current.receipt_status, "completed");
        assert_eq!(current.result_ok, Some(false));
        assert_eq!(
            current.error_code.as_deref(),
            Some("interrupted_by_restart")
        );
        assert_eq!(
            current.input, old.input,
            "recovery must preserve the user's request"
        );
        let thinking: serde_json::Value = serde_json::from_str(&current.thinking).unwrap();
        assert_eq!(thinking["status"], "done");
        assert_eq!(thinking["content"], "keep the accumulated reasoning");
        assert_eq!(current.thinking_status.as_deref(), Some("finish"));
        assert_eq!(
            current.session,
            if turn.interrupted {
                None
            } else {
                old.session.clone()
            }
        );
    }
}

#[tokio::test]
async fn boot_composition_heals_only_proven_local_turns_before_router_publication() {
    boot_composition(Authority::Retained).await;
}

#[tokio::test]
async fn boot_composition_without_retained_authority_keeps_all_turns_quarantined() {
    boot_composition(Authority::Missing).await;
}

#[tokio::test]
async fn boot_composition_unreadable_registry_keeps_all_turns_quarantined() {
    boot_composition(Authority::UnreadableRegistry).await;
}

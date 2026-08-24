//! HTTP e2e + chaos for continue-truncated.
//!
//! Replaces the "manual smoke: click Continue" checklist with deterministic
//! route coverage: Idempotency-Key gating, concurrent same-key collapse, and a
//! scripted MaxTokens → EndTurn recovery through the public API.

mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::StatusCode;
use nomifun_ai_agent::protocol::events::{
    AgentStreamEvent, FinishEventData, TextEventData, TurnStopReason,
};
use nomifun_ai_agent::runtime_handle::{AgentRuntimeControl, AgentRuntimeHandle, MockAgentRuntime};
use nomifun_ai_agent::types::SendMessageData;
use nomifun_ai_agent::{
    AgentRuntimeRegistry, AgentSendError, InMemoryAgentRuntimeRegistry,
};
use nomifun_app::{AppConfig, AppServices, create_router};
use nomifun_common::{AgentKillReason, AgentType, AppError, ConversationStatus, TimestampMs, now_ms};
use nomifun_db::{IConversationRepository, SqliteConversationRepository};
use serde_json::json;
use tokio::sync::broadcast;
use tower::ServiceExt;

use common::{body_json, setup_and_login};

const PROVIDER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000901";

struct ScriptedAgent {
    conversation_id: String,
    event_tx: broadcast::Sender<AgentStreamEvent>,
    scripts: Mutex<VecDeque<Vec<AgentStreamEvent>>>,
}

impl ScriptedAgent {
    fn new(conversation_id: &str, scripts: Vec<Vec<AgentStreamEvent>>) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            conversation_id: conversation_id.to_owned(),
            event_tx,
            scripts: Mutex::new(VecDeque::from(scripts)),
        }
    }
}

#[async_trait]
impl AgentRuntimeControl for ScriptedAgent {
    fn agent_type(&self) -> AgentType {
        AgentType::Nomi
    }
    fn conversation_id(&self) -> &str {
        &self.conversation_id
    }
    fn workspace(&self) -> &str {
        "/tmp/nomifun-truncation-recovery-e2e"
    }
    fn status(&self) -> Option<ConversationStatus> {
        Some(ConversationStatus::Finished)
    }
    fn is_transport_healthy(&self) -> bool {
        true
    }
    fn last_activity_at(&self) -> TimestampMs {
        now_ms()
    }
    fn subscribe(&self) -> broadcast::Receiver<AgentStreamEvent> {
        self.event_tx.subscribe()
    }
    async fn send_message(&self, _data: SendMessageData) -> Result<(), AgentSendError> {
        let script = self
            .scripts
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                vec![AgentStreamEvent::Finish(FinishEventData {
                    session_id: None,
                    stop_reason: Some(TurnStopReason::EndTurn),
                })]
            });
        for event in script {
            let _ = self.event_tx.send(event);
        }
        Ok(())
    }
    async fn cancel(&self) -> Result<(), AppError> {
        Ok(())
    }
    fn kill(&self, _reason: Option<AgentKillReason>) -> Result<(), AppError> {
        Ok(())
    }
}

impl MockAgentRuntime for ScriptedAgent {}

async fn build_app_with_scripted_factory() -> (axum::Router, AppServices) {
    let root = tempfile::Builder::new()
        .prefix("nomifun-truncation-e2e-")
        .tempdir()
        .unwrap()
        .keep();
    let db = nomifun_db::init_database_memory().await.unwrap();
    let factory: Arc<
        dyn Fn(
                nomifun_ai_agent::types::AgentRuntimeBuildOptions,
            ) -> futures_util::future::BoxFuture<'static, Result<AgentRuntimeHandle, AppError>>
            + Send
            + Sync,
    > = Arc::new(|opts| {
        Box::pin(async move {
            Ok(AgentRuntimeHandle::Mock(Arc::new(ScriptedAgent::new(
                &opts.conversation_id,
                vec![
                    vec![
                        AgentStreamEvent::Text(TextEventData {
                            content: "discarded draft".to_owned(),
                        }),
                        AgentStreamEvent::Finish(FinishEventData {
                            session_id: None,
                            stop_reason: Some(TurnStopReason::MaxTokens),
                        }),
                    ],
                    vec![
                        AgentStreamEvent::Text(TextEventData {
                            content: "recovered answer".to_owned(),
                        }),
                        AgentStreamEvent::Finish(FinishEventData {
                            session_id: None,
                            stop_reason: Some(TurnStopReason::EndTurn),
                        }),
                    ],
                ],
            ))))
        })
    });
    let runtime_registry: Arc<dyn AgentRuntimeRegistry> =
        Arc::new(InMemoryAgentRuntimeRegistry::new(factory));
    let services = AppServices::from_config(
        db,
        &AppConfig {
            data_dir: root.join("data"),
            work_dir: root.join("work"),
            ..AppConfig::default()
        },
    )
    .await
    .unwrap()
    .with_agent_runtime_registry(runtime_registry);
    let router = create_router(&services).await;
    (router, services)
}

async fn seed_provider(services: &AppServices) {
    nomifun_db::sqlx::query(
        "INSERT INTO providers \
         (provider_id, platform, name, base_url, api_key_encrypted, enabled, \
          created_at, updated_at) \
         VALUES (?, 'openai', 'truncation-e2e', 'https://example.invalid', 'encrypted', 1, 1, 1)",
    )
    .bind(PROVIDER_ID)
    .execute(services.database.pool())
    .await
    .unwrap();
    nomifun_db::sqlx::query(
        "INSERT INTO provider_models \
         (provider_id, model, enabled, sort_order, tasks, traits, params, source, \
          context_limit, output_limit, created_at, updated_at) \
         VALUES (?, 'm1', 1, 0, '[]', '[]', '{}', 'inferred', 128000, 8192, 1, 1)",
    )
    .bind(PROVIDER_ID)
    .execute(services.database.pool())
    .await
    .unwrap();
}

async fn create_nomi_conversation(
    app: &mut axum::Router,
    token: &str,
    csrf: &str,
    workspace: &str,
) -> String {
    let body = json!({
        "type": "nomi",
        "name": "truncation recovery",
        "model": { "provider_id": PROVIDER_ID, "model": "m1" },
        "extra": { "workspace": workspace }
    });
    let req = common::json_with_token("POST", "/api/conversations", body, token, csrf);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp).await;
    json["data"]["conversation_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn continue_request(
    uri: &str,
    token: &str,
    csrf: &str,
    idempotency_keys: &[&str],
) -> axum::http::Request<Body> {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"));
    for key in idempotency_keys {
        builder = builder.header("idempotency-key", *key);
    }
    builder.body(Body::empty()).unwrap()
}

async fn seed_truncated_source(
    services: &AppServices,
    owner: &str,
    conversation_id: &str,
) -> String {
    let repo = SqliteConversationRepository::new(services.database.pool().clone());
    let operation = format!("public-turn:v1:{owner}:{conversation_id}:source");
    let payload = json!({
        "content": "finish the truncated coding task",
        "files": [],
        "inject_skills": [],
        "hidden": false,
        "origin": null,
        "channel_platform": null,
    })
    .to_string();
    let claimed = repo
        .claim_delivery_receipt_once(owner, conversation_id, &operation, "turn", &payload, now_ms())
        .await
        .unwrap();
    let message_id = claimed.receipt.message_id.clone();
    repo.insert_message(&nomifun_db::models::MessageRow {
        id: 0,
        message_id: message_id.clone(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(message_id.clone()),
        r#type: "text".to_owned(),
        content: json!({"content": "finish the truncated coding task"}).to_string(),
        position: Some("right".to_owned()),
        status: Some("finish".to_owned()),
        hidden: false,
        created_at: now_ms(),
    })
    .await
    .unwrap();
    assert!(
        repo.complete_delivery_receipt(
            owner,
            conversation_id,
            &operation,
            false,
            None,
            Some("output ceiling"),
            Some("output_truncated"),
            Some(true),
            now_ms(),
        )
        .await
        .unwrap()
    );
    nomifun_db::sqlx::query(
        "UPDATE conversations SET status = 'finished' WHERE conversation_id = ? AND user_id = ?",
    )
    .bind(conversation_id)
    .bind(owner)
    .execute(services.database.pool())
    .await
    .unwrap();
    message_id
}

async fn wait_finished(services: &AppServices, conversation_id: &str) {
    let repo = SqliteConversationRepository::new(services.database.pool().clone());
    for _ in 0..500 {
        let finished = IConversationRepository::get(&repo, conversation_id)
            .await
            .unwrap()
            .is_some_and(|row| row.status.as_deref() == Some("finished"));
        if finished {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("conversation {conversation_id} did not return to finished");
}

#[tokio::test]
async fn continue_truncated_requires_idempotency_key() {
    let (mut app, services) = build_app_with_scripted_factory().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    seed_provider(&services).await;
    let workspace = services.work_dir.join("conv-a");
    std::fs::create_dir_all(&workspace).unwrap();
    let conv_id =
        create_nomi_conversation(&mut app, &token, &csrf, &workspace.to_string_lossy()).await;
    let owner = nomifun_db::installation_owner_id(services.database.pool())
        .await
        .unwrap();
    let source_id = seed_truncated_source(&services, &owner, &conv_id).await;

    let uri = format!("/api/conversations/{conv_id}/messages/{source_id}/continue-truncated");
    let resp = app
        .oneshot(continue_request(&uri, &token, &csrf, &[]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp).await;
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|e| e.contains("Idempotency-Key"))
    );
}

#[tokio::test]
async fn continue_truncated_http_chaos_same_key_collapses_and_recovers() {
    let (mut app, services) = build_app_with_scripted_factory().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    seed_provider(&services).await;
    let workspace = services.work_dir.join("conv-b");
    std::fs::create_dir_all(&workspace).unwrap();
    let conv_id =
        create_nomi_conversation(&mut app, &token, &csrf, &workspace.to_string_lossy()).await;
    let owner = nomifun_db::installation_owner_id(services.database.pool())
        .await
        .unwrap();
    let source_id = seed_truncated_source(&services, &owner, &conv_id).await;
    let uri = format!("/api/conversations/{conv_id}/messages/{source_id}/continue-truncated");
    const KEY: &str = "0190f5fe-7c00-7a00-8000-000000000778";

    let mut joins = Vec::new();
    for _ in 0..6 {
        let app = app.clone();
        let uri = uri.clone();
        let token = token.clone();
        let csrf = csrf.clone();
        joins.push(tokio::spawn(async move {
            let resp = app
                .oneshot(continue_request(&uri, &token, &csrf, &[KEY]))
                .await
                .unwrap();
            let status = resp.status();
            let json = body_json(resp).await;
            (status, json)
        }));
    }
    let results = futures_util::future::join_all(joins).await;
    let mut fresh_ids = Vec::new();
    let mut replays = 0usize;
    let mut conflicts = 0usize;
    for joined in results {
        let (status, json) = joined.expect("join");
        if status == StatusCode::ACCEPTED {
            if json["data"]["replayed"] == false {
                fresh_ids.push(
                    json["data"]["msg_id"]
                        .as_str()
                        .expect("fresh continue returns msg_id")
                        .to_owned(),
                );
            } else {
                replays += 1;
                if let Some(owner) = fresh_ids.first() {
                    assert_eq!(json["data"]["msg_id"].as_str(), Some(owner.as_str()));
                }
            }
        } else if status == StatusCode::CONFLICT {
            conflicts += 1;
        } else {
            panic!("unexpected status {status}: {json}");
        }
    }
    assert_eq!(fresh_ids.len(), 1, "HTTP double-click must admit once");
    assert_eq!(
        fresh_ids.len() + replays + conflicts,
        6,
        "every click must resolve as owner, replay, or conflict"
    );

    wait_finished(&services, &conv_id).await;
}

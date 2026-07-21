//! End-to-end verification of conversation boot reconciliation against the
//! real SQLite repositories (not mocks).
//!
//! This reproduces the exact on-disk state that a process killed mid-"thinking"
//! leaves behind — a `running` conversation row, a `thinking` message whose
//! `content.status != "done"`, and a bound resumable ACP session — then runs
//! the production `reconcile_running_conversations_on_boot` and asserts the
//! reopened conversation is settled and can no longer spin forever:
//!
//!   * conversation row flips to `finished`,
//!   * the dangling thinking message is closed (`content.status == "done"`),
//!   * the ACP session id is cleared so the next prompt opens a fresh session
//!     instead of resuming/replaying the interrupted turn.

use std::sync::Arc;

use nomifun_common::{ConversationId, MessageId, UserId, now_ms};
use nomifun_conversation::reconcile_running_conversations_on_boot;
use nomifun_db::models::{ConversationRow, MessageRow};
use nomifun_db::{
    ConversationRowUpdate, CreateAcpSessionParams, Database, IAcpSessionRepository,
    IConversationRepository, SortOrder, SqliteAcpSessionRepository, SqliteConversationRepository,
    init_database_memory_with_owner,
};

const OWNER: &str = "user_0190f5fe-7c00-7a00-8000-000000000001";

async fn fresh_db() -> Database {
    init_database_memory_with_owner(UserId::parse(OWNER.to_owned()).expect("canonical owner"))
        .await
        .expect("in-memory database")
}

fn conversation_row(status: &str) -> ConversationRow {
    let now = now_ms();
    ConversationRow {
        id: ConversationId::new().into_string(),
        user_id: OWNER.to_owned(),
        name: "Ghost turn".to_owned(),
        r#type: "gemini".to_owned(),
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

fn dangling_thinking_message(conv_id: &str) -> MessageRow {
    MessageRow {
        id: MessageId::new().into_string(),
        conversation_id: conv_id.to_owned(),
        msg_id: Some(MessageId::new().into_string()),
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

#[tokio::test]
async fn boot_reconcile_settles_a_ghost_thinking_conversation_end_to_end() {
    let db = fresh_db().await;
    let conv_repo: Arc<dyn IConversationRepository> =
        Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let acp_repo: Arc<dyn IAcpSessionRepository> =
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone()));

    // 1. Recreate the exact state a mid-"thinking" kill leaves on disk.
    let ghost = conversation_row("pending");
    let conv_id = ghost.id.clone();
    conv_repo.create(&ghost).await.expect("create conversation");
    conv_repo
        .update(
            &conv_id,
            &ConversationRowUpdate {
                status: Some("running".to_owned()),
                ..Default::default()
            },
        )
        .await
        .expect("persist running marker (turn start)");
    conv_repo
        .insert_message(&dangling_thinking_message(&conv_id))
        .await
        .expect("insert dangling thinking message");
    bind_acp_session(&acp_repo, &conv_id).await;

    // Precondition: this really is the stuck state we mean to fix.
    let before = conv_repo.get(&conv_id).await.unwrap().unwrap();
    assert_eq!(before.status.as_deref(), Some("running"));
    assert_eq!(
        acp_repo.get(&conv_id).await.unwrap().unwrap().session_id.as_deref(),
        Some("sess-interrupted-turn")
    );

    // 2. Boot reconciliation — the production entrypoint wired at startup.
    let reconciled = reconcile_running_conversations_on_boot(&conv_repo, &acp_repo).await;
    assert_eq!(reconciled, 1, "the single ghost conversation must be settled");

    // 3. The reopened conversation is idle and can no longer spin forever.
    let after = conv_repo.get(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        after.status.as_deref(),
        Some("finished"),
        "conversation row must be flipped to the terminal status"
    );

    let session_after = acp_repo.get(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        session_after.session_id, None,
        "resumable ACP session must be cleared so the next prompt opens a fresh session"
    );

    let messages = conv_repo
        .get_messages(&conv_id, 1, 5000, SortOrder::Asc)
        .await
        .unwrap()
        .items;
    let thinking = messages
        .iter()
        .find(|m| m.r#type == "thinking")
        .expect("thinking message survives reconciliation");
    let content: serde_json::Value = serde_json::from_str(&thinking.content).unwrap();
    assert_eq!(
        content["status"], "done",
        "dangling thinking message must be closed as an ended thought"
    );
    assert_eq!(
        content["content"], "still reasoning about the request",
        "accumulated thinking content must be preserved, only its status closed"
    );
}

#[tokio::test]
async fn boot_reconcile_leaves_healthy_conversations_untouched() {
    let db = fresh_db().await;
    let conv_repo: Arc<dyn IConversationRepository> =
        Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let acp_repo: Arc<dyn IAcpSessionRepository> =
        Arc::new(SqliteAcpSessionRepository::new(db.pool().clone()));

    // A normal, already-finished conversation with a live ACP session must not
    // be swept — the boot sweep targets only `running` crash remnants.
    let healthy = conversation_row("finished");
    let conv_id = healthy.id.clone();
    conv_repo.create(&healthy).await.expect("create conversation");
    bind_acp_session(&acp_repo, &conv_id).await;

    let reconciled = reconcile_running_conversations_on_boot(&conv_repo, &acp_repo).await;
    assert_eq!(reconciled, 0, "no running rows means nothing to settle");

    let after = conv_repo.get(&conv_id).await.unwrap().unwrap();
    assert_eq!(after.status.as_deref(), Some("finished"));
    assert_eq!(
        acp_repo.get(&conv_id).await.unwrap().unwrap().session_id.as_deref(),
        Some("sess-interrupted-turn"),
        "a healthy conversation's ACP session must be left intact"
    );
}

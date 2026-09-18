use nomifun_db::{
    IConversationRepository, SqliteConversationRepository,
    models::{ConversationRow},
};

const USER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000001";
const PROVIDER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000002";

async fn init_database_memory() -> Result<nomifun_db::Database, nomifun_db::DbError> {
    nomifun_db::init_database_memory_with_owner(
        nomifun_common::UserId::parse(USER_ID.to_owned()).expect("canonical fixture owner"),
    )
    .await
}

async fn setup() -> (SqliteConversationRepository, nomifun_db::Database) {
    let db = init_database_memory().await.unwrap();
    sqlx::query(
        "INSERT INTO providers (\
            provider_id, platform, name, base_url, api_key_encrypted, enabled, \
            created_at, updated_at\
         ) VALUES (\
            ?, 'openai', 'Fixture provider', 'https://example.invalid', \
            'encrypted', 1, 0, 0\
         )",
    )
    .bind(PROVIDER_ID)
    .execute(db.pool())
    .await
    .unwrap();
    let repo = SqliteConversationRepository::new(db.pool().clone());
    (repo, db)
}

fn make_conversation(suffix: &str) -> ConversationRow {
    let now = nomifun_common::now_ms();
    ConversationRow {
        id: 0,
        conversation_id: nomifun_common::ConversationId::new().into_string(),
        user_id: USER_ID.to_string(),
        name: format!("Conversation {suffix}"),
        r#type: "nomi".to_string(),
        extra: r#"{"app_server_chat":true,"workspace":"/home/user/project"}"#.to_string(),
        delegation_policy: "automatic".to_string(),
        execution_model_pool: None,
        decision_policy: "automatic".to_string(),
        execution_template_id: None,
        model: Some(format!(
            r#"{{"provider_id":"{PROVIDER_ID}","model":"mimo-v2.5-free"}}"#
        )),
        status: Some("pending".to_string()),
        source: Some("nomifun".to_string()),
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

#[tokio::test]
async fn context_usage_upsert_and_get_round_trip() {
    let (repo, _db) = setup().await;
    let mut conversation = make_conversation("usage-roundtrip");
    conversation.conversation_id = repo.create(&conversation).await.unwrap();

    assert!(
        repo.get_app_server_context_usage(&conversation.conversation_id)
            .await
            .unwrap()
            .is_none(),
        "no snapshot exists before the first measured turn"
    );

    repo.upsert_app_server_context_usage(
        &conversation.conversation_id,
        1200,
        200_000,
        Some(1200),
        Some(340),
        1000,
    )
    .await
    .unwrap();
    let row = repo
        .get_app_server_context_usage(&conversation.conversation_id)
        .await
        .unwrap()
        .expect("snapshot must be readable");
    assert_eq!(row.context_tokens, 1200);
    assert_eq!(row.window_tokens, 200_000);
    assert_eq!(row.last_turn_input_tokens, Some(1200));
    assert_eq!(row.last_turn_output_tokens, Some(340));
    assert_eq!(row.updated_at, 1000);
}

/// R14 / W9 ③：未上报的一轮必须读回 `None`（NULL），**不是 `0`** —— 库里存 0，
/// 重载后的界面就会把「没上报」显示成「这一轮免费」。
#[tokio::test]
async fn last_turn_tokens_stay_null_when_the_runtime_reported_nothing() {
    let (repo, db) = setup().await;
    let mut conversation = make_conversation("usage-last-turn-null");
    conversation.conversation_id = repo.create(&conversation).await.unwrap();

    repo.upsert_app_server_context_usage(&conversation.conversation_id, 700, 200_000, None, None, 10)
        .await
        .unwrap();

    let row = repo
        .get_app_server_context_usage(&conversation.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.last_turn_input_tokens, None);
    assert_eq!(row.last_turn_output_tokens, None);

    // 直接读列：NULL 是 SQL NULL，不是被默认值补出来的 0。
    let (input_is_null, output_is_null): (i64, i64) = sqlx::query_as(
        "SELECT last_turn_input_tokens IS NULL, last_turn_output_tokens IS NULL \
         FROM app_server_context_usage WHERE conversation_id = ?",
    )
    .bind(&conversation.conversation_id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(input_is_null, 1, "unreported input tokens must be SQL NULL");
    assert_eq!(output_is_null, 1, "unreported output tokens must be SQL NULL");
}

#[tokio::test]
async fn context_usage_upsert_replaces_the_single_row_per_conversation() {
    let (repo, _db) = setup().await;
    let mut conversation = make_conversation("usage-replace");
    conversation.conversation_id = repo.create(&conversation).await.unwrap();

    repo.upsert_app_server_context_usage(&conversation.conversation_id, 100, 200_000, Some(100), Some(20), 1)
        .await
        .unwrap();
    // 第二轮：上下文占用与「上一轮」token 一起被原地覆盖 —— 这一列只承载最近一轮，
    // 它不是逐轮审计（回放历史要另立表）。
    repo.upsert_app_server_context_usage(&conversation.conversation_id, 9000, 200_000, Some(9000), Some(0), 2)
        .await
        .unwrap();

    let actual: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM app_server_context_usage WHERE conversation_id = ?",
    )
    .bind(&conversation.conversation_id)
    .fetch_one(_db.pool())
    .await
    .unwrap();
    assert_eq!(actual, 1, "upsert keeps exactly one snapshot per conversation");

    let row = repo
        .get_app_server_context_usage(&conversation.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.context_tokens, 9000);
    assert_eq!(row.last_turn_input_tokens, Some(9000));
    // 运行时报的 0 是测量结果，照原样存下（与 NULL「未上报」区分开）。
    assert_eq!(row.last_turn_output_tokens, Some(0));
    assert_eq!(row.updated_at, 2);
}

#[tokio::test]
async fn context_usage_is_removed_with_the_conversation() {
    let (repo, _db) = setup().await;
    let mut conversation = make_conversation("usage-delete");
    conversation.conversation_id = repo.create(&conversation).await.unwrap();
    repo.upsert_app_server_context_usage(&conversation.conversation_id, 500, 200_000, Some(500), Some(80), 3)
        .await
        .unwrap();

    repo.delete_with_cleanup(&conversation.conversation_id).await.unwrap();
    assert!(
        repo.get_app_server_context_usage(&conversation.conversation_id)
            .await
            .unwrap()
            .is_none(),
        "deleting the conversation must remove its context snapshot"
    );
}

#[tokio::test]
async fn context_usage_rejects_negative_counts() {
    let (repo, _db) = setup().await;
    let mut conversation = make_conversation("usage-negative");
    conversation.conversation_id = repo.create(&conversation).await.unwrap();
    assert!(repo
        .upsert_app_server_context_usage(&conversation.conversation_id, -1, 200_000, None, None, 3)
        .await
        .is_err());
    // 上报的逐轮 token 同样不许为负（NULL 才是「未上报」，负数不是一种未知）。
    assert!(repo
        .upsert_app_server_context_usage(&conversation.conversation_id, 10, 200_000, Some(-1), Some(0), 3)
        .await
        .is_err());
    assert!(repo
        .upsert_app_server_context_usage(&conversation.conversation_id, 10, 200_000, Some(0), Some(-1), 3)
        .await
        .is_err());
}
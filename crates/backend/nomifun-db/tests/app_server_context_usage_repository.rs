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

    repo.upsert_app_server_context_usage(&conversation.conversation_id, 1200, 200_000, 1000)
        .await
        .unwrap();
    let row = repo
        .get_app_server_context_usage(&conversation.conversation_id)
        .await
        .unwrap()
        .expect("snapshot must be readable");
    assert_eq!(row.context_tokens, 1200);
    assert_eq!(row.window_tokens, 200_000);
    assert_eq!(row.updated_at, 1000);
}

#[tokio::test]
async fn context_usage_upsert_replaces_the_single_row_per_conversation() {
    let (repo, _db) = setup().await;
    let mut conversation = make_conversation("usage-replace");
    conversation.conversation_id = repo.create(&conversation).await.unwrap();

    repo.upsert_app_server_context_usage(&conversation.conversation_id, 100, 200_000, 1)
        .await
        .unwrap();
    repo.upsert_app_server_context_usage(&conversation.conversation_id, 9000, 200_000, 2)
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
    assert_eq!(row.updated_at, 2);
}

#[tokio::test]
async fn context_usage_is_removed_with_the_conversation() {
    let (repo, _db) = setup().await;
    let mut conversation = make_conversation("usage-delete");
    conversation.conversation_id = repo.create(&conversation).await.unwrap();
    repo.upsert_app_server_context_usage(&conversation.conversation_id, 500, 200_000, 3)
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
        .upsert_app_server_context_usage(&conversation.conversation_id, -1, 200_000, 3)
        .await
        .is_err());
}
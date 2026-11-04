//! 迁移 070 §14 节点容量反馈信号（y07）的契约：
//! `learning_lessons.capacity_note` 是可空的内容元数据列（旧行 NULL，重生成
//! 缺席写 NULL 清旧值）；`learning_lesson_capacity_feedback` 是 per-user /
//! per-lesson latest-wins 留痕（`feedback_id` UUIDv7 唯一业务身份，
//! `UNIQUE (user_id, lesson_id)` 兜底）。`choice` 枚举无 DB CHECK（ADR-0015，
//! Rust 单源，写侧严格、读侧宽容）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const MODULE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const LESSON_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678903";
const USER_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678904";
const OTHER_USER_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678905";
const FEEDBACK_ID: &str = "0190f5fe-7c00-7a00-8abc-0123456789aa";

async fn setup() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn seed_lesson(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '容量课', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules \
         (module_id, course_id, title, description, position) \
         VALUES (?, ?, '模块', '', 0)",
    )
    .bind(MODULE_ID)
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, position, estimated_minutes) \
         VALUES (?, ?, '课时', '', 0, 10)",
    )
    .bind(LESSON_ID)
    .bind(MODULE_ID)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_feedback(
    pool: &SqlitePool,
    feedback_id: &str,
    user_id: &str,
    lesson_id: &str,
    choice: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO learning_lesson_capacity_feedback \
         (feedback_id, user_id, lesson_id, choice, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 2000, 2000)",
    )
    .bind(feedback_id)
    .bind(user_id)
    .bind(lesson_id)
    .bind(choice)
    .execute(pool)
    .await
    .map(|_| ())
}

/// 旧课时行容忍：加列后可空、无默认（NULL = 无自报），新值原样落库。
#[tokio::test]
async fn capacity_note_column_is_nullable_and_legacy_rows_read_null() {
    let pool = setup().await;
    seed_lesson(&pool).await;
    let note: Option<String> =
        sqlx::query_scalar("SELECT capacity_note FROM learning_lessons WHERE lesson_id = ?")
            .bind(LESSON_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(note, None, "旧行 capacity_note 必须为 NULL");

    sqlx::query("UPDATE learning_lessons SET capacity_note = ? WHERE lesson_id = ?")
        .bind("保覆盖就必超：第三节的推导超出 25 分钟预算")
        .bind(LESSON_ID)
        .execute(&pool)
        .await
        .unwrap();
    let note: Option<String> =
        sqlx::query_scalar("SELECT capacity_note FROM learning_lessons WHERE lesson_id = ?")
            .bind(LESSON_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        note.as_deref(),
        Some("保覆盖就必超：第三节的推导超出 25 分钟预算")
    );
}

/// 反馈表列形状：`note` 默认空串、时间戳必填，`choice` 无 DB CHECK
///（Rust 单源，写侧严格读侧宽容）。
#[tokio::test]
async fn feedback_rows_default_note_and_accept_any_choice_string() {
    let pool = setup().await;
    seed_lesson(&pool).await;
    insert_feedback(&pool, FEEDBACK_ID, USER_ID, LESSON_ID, "accept")
        .await
        .unwrap();
    let row = sqlx::query(
        "SELECT note, created_at, updated_at FROM learning_lesson_capacity_feedback \
         WHERE feedback_id = ?",
    )
    .bind(FEEDBACK_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("note"), "");
    assert_eq!(row.get::<i64, _>("created_at"), 2000);
    assert_eq!(row.get::<i64, _>("updated_at"), 2000);

    // 未知/未来枚举值写侧不入库是应用层契约；DB 不设 CHECK（ADR-0015）。
    insert_feedback(
        &pool,
        "0190f5fe-7c00-7a00-8abc-0123456789ab",
        OTHER_USER_ID,
        LESSON_ID,
        "future_arm",
    )
    .await
    .unwrap();
}

/// 身份与唯一性：`feedback_id` 是 36 位 UUIDv7 唯一业务身份（同形校验），
/// `(user_id, lesson_id)` 唯一——latest-wins 靠应用层 UPSERT，DB 兜底重复。
#[tokio::test]
async fn feedback_identity_and_latest_wins_key_are_unique() {
    let pool = setup().await;
    seed_lesson(&pool).await;
    insert_feedback(&pool, FEEDBACK_ID, USER_ID, LESSON_ID, "accept")
        .await
        .unwrap();
    assert!(
        insert_feedback(&pool, FEEDBACK_ID, OTHER_USER_ID, LESSON_ID, "split")
            .await
            .is_err(),
        "feedback_id 唯一"
    );
    assert!(
        insert_feedback(
            &pool,
            "0190f5fe-7c00-7a00-8abc-0123456789ac",
            USER_ID,
            LESSON_ID,
            "split"
        )
        .await
        .is_err(),
        "(user_id, lesson_id) 唯一兜底"
    );
    assert!(
        insert_feedback(&pool, "not-a-uuid", USER_ID, LESSON_ID, "accept")
            .await
            .is_err(),
        "feedback_id 必须与现有 36 位 UUIDv7 校验同形"
    );
}

/// 读侧索引：按课时聚合/回读与按用户清理各有一条具名索引；id 契约注册
/// （PRODUCT_TABLES + identity + 两条逻辑引用）全库校验通过。
#[tokio::test]
async fn feedback_indexes_and_id_schema_contract_are_registered() {
    let pool = setup().await;
    seed_lesson(&pool).await;
    insert_feedback(&pool, FEEDBACK_ID, USER_ID, LESSON_ID, "accept")
        .await
        .unwrap();

    let lesson_indexed: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_index_info('idx_learning_lesson_capacity_feedback_lesson') \
         ORDER BY seqno",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(lesson_indexed, vec!["lesson_id".to_owned()]);

    let user_indexed: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_index_info('idx_learning_lesson_capacity_feedback_user') \
         ORDER BY seqno",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(user_indexed, vec!["user_id".to_owned()]);

    nomifun_db::validate_id_schema_contract(&pool)
        .await
        .expect("id schema contract (含 learning_lesson_capacity_feedback)");
}

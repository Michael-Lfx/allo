//! 迁移 070 §13 学习系统动作簿（`learning_system_actions`）的契约：一次动作
//! 一行终态、`action_id` 是唯一业务身份（36 位 UUIDv7 GLOB）、枚举列无 DB
//! CHECK（ADR-0015，Rust 单源）、读侧索引 `(course_id, at)`。课程删除随删
//! （逻辑引用 Cascade，见 id_schema_contract）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const ACTION_ID: &str = "0190f5fe-7c00-7a00-8abc-0123456789aa";

async fn setup() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn seed_course(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '动作簿课', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_action(pool: &SqlitePool, action_id: &str, result: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO learning_system_actions \
         (action_id, course_id, kind, trigger_source, result, at) \
         VALUES (?, ?, 'growth', 'manual', ?, 2000)",
    )
    .bind(action_id)
    .bind(COURSE_ID)
    .bind(result)
    .execute(pool)
    .await
    .map(|_| ())
}

#[tokio::test]
async fn system_actions_row_defaults_and_result_set() {
    let pool = setup().await;
    seed_course(&pool).await;
    insert_action(&pool, ACTION_ID, "done").await.unwrap();

    let row = sqlx::query(
        "SELECT basis, reason, actor_user_id, at FROM learning_system_actions \
         WHERE action_id = ?",
    )
    .bind(ACTION_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("basis"), "");
    assert_eq!(row.get::<String, _>("reason"), "");
    assert_eq!(row.get::<Option<String>, _>("actor_user_id"), None);
    assert_eq!(row.get::<i64, _>("at"), 2000);

    // failed 与未知结果值都合法：枚举集在 Rust 单源（ADR-0015），DB 无 CHECK。
    insert_action(&pool, "0190f5fe-7c00-7a00-8abc-0123456789ab", "failed")
        .await
        .unwrap();
    insert_action(&pool, "0190f5fe-7c00-7a00-8abc-0123456789ac", "future_arm")
        .await
        .unwrap();
}

#[tokio::test]
async fn system_actions_action_id_is_the_unique_business_identity() {
    let pool = setup().await;
    seed_course(&pool).await;
    insert_action(&pool, ACTION_ID, "done").await.unwrap();
    assert!(
        insert_action(&pool, ACTION_ID, "failed").await.is_err(),
        "action_id 唯一"
    );
    assert!(
        insert_action(&pool, "not-a-uuid", "done").await.is_err(),
        "action_id 必须与现有 36 位 UUIDv7 校验同形"
    );
}

#[tokio::test]
async fn system_actions_course_index_is_the_read_side_index() {
    let pool = setup().await;
    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, type FROM pragma_table_info('learning_system_actions') ORDER BY cid",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let names: Vec<&str> = columns.iter().map(|(name, _)| name.as_str()).collect();
    for expected in [
        "id",
        "action_id",
        "course_id",
        "kind",
        "trigger_source",
        "basis",
        "result",
        "reason",
        "actor_user_id",
        "at",
    ] {
        assert!(names.contains(&expected), "missing column {expected}: {names:?}");
    }
    assert!(
        !names.contains(&"ref_id"),
        "ref_id 是对挂载/修补/批次行的技术 id 复制（data-and-identifier-standards §1）——不得回填：{names:?}"
    );

    // 读侧按课程倒序：索引前两列 = (course_id, at)。
    let indexed: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_index_info('idx_learning_system_actions_course') ORDER BY seqno",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(indexed, vec!["course_id".to_owned(), "at".to_owned()]);
}

/// v3 契约注册：新表进 PRODUCT_TABLES + action_id 身份 + course_id 逻辑引用
///（Cascade）+ actor_user_id 用户引用；整库契约校验全绿（表 → 宿主列 →
/// 具名索引三面对账）。
#[tokio::test]
async fn system_actions_is_registered_in_the_id_schema_contract() {
    let pool = setup().await;
    seed_course(&pool).await;
    insert_action(&pool, ACTION_ID, "done").await.unwrap();
    nomifun_db::validate_id_schema_contract(&pool)
        .await
        .expect("id schema contract (含 learning_system_actions)");
}

//! 迁移 076（`learning_courses.source_kb_ids`）的契约：多知识库（t4）的
//! 来源库列表是 `source_kb_id` 的伴随列（首个 = 主来源 = 旧单列），老行由
//! 单列回填成单元素数组；`source_kb_id` 列保留不动（读侧对已删库 id 过滤）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const OTHER_COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const KB_ID: &str = "0190f5fe-7c00-7a00-8abc-112345678901";
const SECOND_KB_ID: &str = "0190f5fe-7c00-7a00-8abc-112345678902";

async fn setup() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

/// 只跑 `(from, to]` 区间内的迁移——回填类迁移必须在有老数据的库上验证
/// （整库一次性 run 是空库，回填 UPDATE 无行可测）。
async fn migrate_range(pool: &SqlitePool, from: i64, to: i64) {
    for migration in MIGRATOR.iter() {
        if migration.version <= from || migration.version > to {
            continue;
        }
        sqlx::raw_sql(&migration.sql)
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("migration {} failed: {error}", migration.version));
    }
}

/// 老行回填：073 之后 `source_kb_ids` = `[source_kb_id]`；无单列的行得到
/// 空数组（描述流/已删库的旧行为不变）。
#[tokio::test]
async fn migration_backfills_legacy_single_source_into_a_one_element_list() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrate_range(&pool, 0, 75).await;
    sqlx::query(
        "INSERT INTO knowledge_bases (knowledge_base_id, name, root_path, created_at, updated_at) \
         VALUES (?, '金融笔记', '/tmp/kb', 1000, 1000)",
    )
    .bind(KB_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, source_kb_id, version, created_at, updated_at) \
         VALUES (?, '老课程', '', 'general', ?, 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .bind(KB_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '描述流课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(OTHER_COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();

    migrate_range(&pool, 75, i64::MAX).await;

    let backfilled: String =
        sqlx::query_scalar("SELECT source_kb_ids FROM learning_courses WHERE course_id = ?")
            .bind(COURSE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&backfilled).unwrap(),
        vec![KB_ID.to_owned()],
        "{backfilled}"
    );
    let single: Option<String> =
        sqlx::query_scalar("SELECT source_kb_id FROM learning_courses WHERE course_id = ?")
            .bind(COURSE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(single.as_deref(), Some(KB_ID), "单列保留不动（主来源）");

    let empty: String =
        sqlx::query_scalar("SELECT source_kb_ids FROM learning_courses WHERE course_id = ?")
            .bind(OTHER_COURSE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(empty, "[]", "无来源的旧行回填为空数组");
}

/// 新行：默认 `[]`（NOT NULL），存 JSON 数组原样读回，非数组/非法 JSON 被
/// CHECK 拒绝；多库列表与单列并存（首个 = 主来源）。
#[tokio::test]
async fn source_kb_ids_round_trips_as_a_json_array() {
    let pool = setup().await;
    for kb_id in [KB_ID, SECOND_KB_ID] {
        sqlx::query(
            "INSERT INTO knowledge_bases (knowledge_base_id, name, root_path, created_at, updated_at) \
             VALUES (?, ?, '/tmp/kb', 1000, 1000)",
        )
        .bind(kb_id)
        .bind(format!("库 {kb_id}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '默认课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();
    let default_list: String =
        sqlx::query_scalar("SELECT source_kb_ids FROM learning_courses WHERE course_id = ?")
            .bind(COURSE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(default_list, "[]", "新列 NOT NULL DEFAULT '[]'");

    let list = serde_json::json!([KB_ID, SECOND_KB_ID]).to_string();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, source_kb_id, source_kb_ids, version, created_at, updated_at) \
         VALUES (?, '多库课程', '', 'general', ?, ?, 1, 1000, 1000)",
    )
    .bind(OTHER_COURSE_ID)
    .bind(KB_ID)
    .bind(&list)
    .execute(&pool)
    .await
    .unwrap();
    let row = sqlx::query("SELECT source_kb_id, source_kb_ids FROM learning_courses WHERE course_id = ?")
        .bind(OTHER_COURSE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    let stored: String = row.try_get("source_kb_ids").unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&stored).unwrap(),
        vec![KB_ID.to_owned(), SECOND_KB_ID.to_owned()],
        "列表顺序原样保留：首个 = 主来源"
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("source_kb_id").unwrap().as_deref(),
        Some(KB_ID)
    );

    // 对象 / 标量 / 非法 JSON 都必须被 CHECK 拒绝。
    for bad in ["{\"a\":1}", "\"text\"", "42", "not json"] {
        let result = sqlx::query("UPDATE learning_courses SET source_kb_ids = ? WHERE course_id = ?")
            .bind(bad)
            .bind(OTHER_COURSE_ID)
            .execute(&pool)
            .await;
        assert!(result.is_err(), "{bad} must be rejected by the CHECK");
    }
}

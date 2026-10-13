//! 迁移 075（`learning_lessons.source_paths_json`）的契约：多引用（t4 评审
//! 补齐）的课时来源列表是 `source_path`/`source_start`/`source_end` 三列的
//! 伴随列（首个 = 主来源 = 三列的兼容位），老行由三列回填成单元素数组；
//! 三列保留不动（读侧对空列表回退三列）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const MODULE_ID: &str = "0190f5fe-7c00-7a00-8abc-022345678901";
const LESSON_ID: &str = "0190f5fe-7c00-7a00-8abc-032345678901";
const OTHER_LESSON_ID: &str = "0190f5fe-7c00-7a00-8abc-032345678902";

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

/// 老行回填：075 之后 `source_paths_json` = `[{"path":…}]`（三列的单来源
/// 形状）；无来源的旧行得到空数组（描述流/旧课程行为不变）。
#[tokio::test]
async fn migration_backfills_legacy_single_source_into_a_one_element_list() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrate_range(&pool, 0, 74).await;
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '老课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '模块', '', 0)",
    )
    .bind(MODULE_ID)
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();
    let mut position = 0i64;
    for (lesson_id, source_path) in [
        (LESSON_ID, Some("docs/basics.md")),
        (OTHER_LESSON_ID, None),
    ] {
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
              content_generated, source_path, source_start, source_end) \
             VALUES (?, ?, '课时', '', 'p', ?, 10, 1, ?, NULL, NULL)",
        )
        .bind(lesson_id)
        .bind(MODULE_ID)
        .bind(position)
        .bind(source_path)
        .execute(&pool)
        .await
        .unwrap();
        position += 1;
    }

    migrate_range(&pool, 74, i64::MAX).await;

    let backfilled: String =
        sqlx::query_scalar("SELECT source_paths_json FROM learning_lessons WHERE lesson_id = ?")
            .bind(LESSON_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_str(&backfilled).unwrap();
    assert_eq!(entries.len(), 1, "{backfilled}");
    assert_eq!(entries[0]["path"], "docs/basics.md");
    assert_eq!(entries[0]["start"], serde_json::Value::Null, "NULL 三列原样为 null: {backfilled}");
    assert_eq!(entries[0]["end"], serde_json::Value::Null);
    // 三列保留不动（兼容位）。
    let single: Option<String> =
        sqlx::query_scalar("SELECT source_path FROM learning_lessons WHERE lesson_id = ?")
            .bind(LESSON_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(single.as_deref(), Some("docs/basics.md"), "三列保留不动");

    let empty: String =
        sqlx::query_scalar("SELECT source_paths_json FROM learning_lessons WHERE lesson_id = ?")
            .bind(OTHER_LESSON_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(empty, "[]", "无来源的旧行回填为空数组（读侧回退三列）");
}

/// 新行：默认 `[]`（NOT NULL），存 JSON 数组原样读回，非数组/非法 JSON 被
/// CHECK 拒绝；多引用列表与三列并存（首个 = 主来源）。
#[tokio::test]
async fn source_paths_json_round_trips_as_a_json_array() {
    let pool = setup().await;
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '模块', '', 0)",
    )
    .bind(MODULE_ID)
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, content_generated) \
         VALUES (?, ?, '课时', '', 'p', 0, 10, 0)",
    )
    .bind(LESSON_ID)
    .bind(MODULE_ID)
    .execute(&pool)
    .await
    .unwrap();
    let default_list: String =
        sqlx::query_scalar("SELECT source_paths_json FROM learning_lessons WHERE lesson_id = ?")
            .bind(LESSON_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(default_list, "[]", "新列 NOT NULL DEFAULT '[]'");

    let list = serde_json::json!([
        {"path": "docs/a.md", "start": null, "end": null},
        {"path": "docs/b.md", "start": 1, "end": 9},
    ])
    .to_string();
    sqlx::query(
        "UPDATE learning_lessons SET source_paths_json = ?, source_path = 'docs/a.md' WHERE lesson_id = ?",
    )
    .bind(&list)
    .bind(LESSON_ID)
    .execute(&pool)
    .await
    .unwrap();
    let row = sqlx::query("SELECT source_path, source_paths_json FROM learning_lessons WHERE lesson_id = ?")
        .bind(LESSON_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    let stored: String = row.try_get("source_paths_json").unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<serde_json::Value>>(&stored).unwrap().len(),
        2,
        "列表顺序原样保留：首个 = 主来源"
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("source_path").unwrap().as_deref(),
        Some("docs/a.md")
    );

    // 对象 / 标量 / 非法 JSON 都必须被 CHECK 拒绝。
    for bad in ["{\"a\":1}", "\"text\"", "42", "not json"] {
        let result = sqlx::query("UPDATE learning_lessons SET source_paths_json = ? WHERE lesson_id = ?")
            .bind(bad)
            .bind(LESSON_ID)
            .execute(&pool)
            .await;
        assert!(result.is_err(), "{bad} must be rejected by the CHECK");
    }
}

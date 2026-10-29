//! 迁移 080（概念别名结构化 + 概念网边 ref_name）的契约（三轮 g2，j04）：
//! `aliases_json` 元素从字符串回填为 `{name, status:"active"}` 记录（定义/
//! 出处缺席 = 与概念定义同义/出处未知）；`learning_lesson_concepts.ref_name`
//! 可空列承载节点引用时的投影名（老行为 NULL）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 只跑 `(from, to]` 区间内的迁移——回填类迁移必须在有老数据的库上验证。
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

/// 老形状字符串回填：每个别名升为 `{name, status:"active"}` 记录。
#[tokio::test]
async fn migration_backfills_string_aliases_into_records() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrate_range(&pool, 0, 79).await;
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-111111111111', '极限', '[\"Limits\",\"配方法\"]', 'd', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    migrate_range(&pool, 79, 80).await;
    let aliases_json: String =
        sqlx::query_scalar("SELECT aliases_json FROM learning_concept_registry WHERE canonical = '极限'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&aliases_json).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0]["name"], "Limits");
    assert_eq!(parsed[0]["status"], "active");
    assert_eq!(parsed[1]["name"], "配方法");
    // 读侧（json_extract 路径）按名字可查活跃别名。
    let hit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_concept_registry reg, json_each(reg.aliases_json) je \
         WHERE lower(COALESCE(json_extract(je.value, '$.name'), je.value)) = 'limits' \
           AND COALESCE(json_extract(je.value, '$.status'), 'active') = 'active'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hit, 1);
}

/// ref_name 可空列：新写带投影名，旧行为 NULL（读侧回退正名）。
#[tokio::test]
async fn ref_name_column_is_nullable() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    let nullable: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), SUM(NOT \"notnull\") FROM pragma_table_info('learning_lesson_concepts') \
         WHERE name = 'ref_name'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(nullable, (1, 1), "ref_name exists and is nullable");
}

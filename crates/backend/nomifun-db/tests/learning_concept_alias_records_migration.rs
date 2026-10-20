//! 迁移 068 学习域合并基线的别名段（概念别名结构化 + 概念网边 ref_name）的
//! 契约（三轮 g2，j04）：`aliases_json` 承载 `{name, definition?, attestation?,
//! status}` 记录（定义/出处缺席 = 与概念定义同义/出处未知），读侧按
//! json_extract 路径取活跃别名；`learning_lesson_concepts.ref_name` 可空列
//! 承载节点引用时的投影名（老行为 NULL）。
//!
//! 别名回填语句随合并基线落在一个版本里，新库的登记表在同一迁移内创建、
//! 恒为空表，回填不再有可测的中间状态——这里改为钉住记录形状的读写契约。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

async fn setup() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

/// 别名记录形状：结构化记录原样读回，json_extract 路径按名字命中活跃别名。
#[tokio::test]
async fn registry_aliases_are_record_shaped() {
    let pool = setup().await;
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-111111111111', '极限', \
                 '[{\"name\":\"Limits\",\"status\":\"active\"},{\"name\":\"配方法\",\"status\":\"active\"}]', \
                 'd', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

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

    // aliases_json 必须是 JSON 数组：对象/标量落库即被 CHECK 拒绝。
    for bad in ["{\"name\":\"x\"}", "\"Limits\"", "42", "not json"] {
        let result = sqlx::query(
            "UPDATE learning_concept_registry SET aliases_json = ? WHERE canonical = '极限'",
        )
        .bind(bad)
        .execute(&pool)
        .await;
        assert!(result.is_err(), "{bad} must be rejected by the CHECK");
    }
}

/// ref_name 可空列：新写带投影名，旧行为 NULL（读侧回退正名）。
#[tokio::test]
async fn ref_name_column_is_nullable() {
    let pool = setup().await;
    let nullable: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), SUM(NOT \"notnull\") FROM pragma_table_info('learning_lesson_concepts') \
         WHERE name = 'ref_name'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(nullable, (1, 1), "ref_name exists and is nullable");
}

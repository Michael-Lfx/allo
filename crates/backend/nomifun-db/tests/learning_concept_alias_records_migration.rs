//! 基线 070（概念别名结构化 + 概念网边 ref_name，原迁移 080）的契约（三轮
//! g2，j04）：`aliases_json` 的元素形状是 `{name, status:"active"}` 记录（定义/
//! 出处缺席 = 与概念定义同义/出处未知）——原 080 的字符串回填随折叠消失
//! （登记表在基线里出生即记录形，用户库不存在中间态），这里改钉记录形状
//! 契约；`learning_lesson_concepts.ref_name` 可空列承载节点引用时的投影名
//! （老行为 NULL）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 记录形状契约：登记表接受 `{name, status}` 记录，读侧 json_extract 路径
/// 按名字查活跃别名；可选 definition/attestation 与 retired 状态同样合法。
#[tokio::test]
async fn alias_records_pin_the_structured_shape() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
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

    // 可选出处的 retired 记录同样是合法形状（退役不占名池）。
    sqlx::query(
        "UPDATE learning_concept_registry SET aliases_json = ? WHERE canonical = '极限'",
    )
    .bind(r#"[{"name":"Limits","definition":"d","attestation":"a","status":"retired"}]"#)
    .execute(&pool)
    .await
    .unwrap();
    let retired: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_concept_registry reg, json_each(reg.aliases_json) je \
         WHERE json_extract(je.value, '$.status') = 'retired'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retired, 1);
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

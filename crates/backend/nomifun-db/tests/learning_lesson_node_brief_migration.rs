//! 迁移 070「原 073」节同区追加的节点声明面两列（absorb-8 z01，learnhub
//! #205 同构）：`learning_lessons.intent` = 创建意图（为何有这个学习点/课中
//! 角色，一句话）、`learning_lessons.scope` = 教学范围（教什么/教到哪/明确
//! 不碰什么，一两句）。两列跟随 purpose/difficulty 先例：`NOT NULL DEFAULT
//! ''`——空串 = 旧档未声明（零回填零破坏）；非空约束（trim 后 1..=200 字）
//! 由生长契约门（`ng.node.brief`）执法，DB 不重验（ADR-0015：DB 只保留数值
//! 域约束，语义单源在应用层）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678911";
const MODULE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678912";
const LESSON_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678913";

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
         VALUES (?, '声明课', '', 'general', 1, 1000, 1000)",
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
    // 不带 intent/scope 的 INSERT（旧写法/旧档路径）：依赖列缺省。
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

/// 列形状：两列在场、`NOT NULL DEFAULT ''`（缺省空串 = 旧档未声明）——
/// pragma 逐列钉住 notnull/dflt_value，列序随追加位（difficulty 区之后）。
#[tokio::test]
async fn brief_columns_are_not_null_with_empty_default() {
    let pool = setup().await;
    let columns: Vec<(String, i64, Option<String>)> = sqlx::query(
        "SELECT name, \"notnull\", dflt_value FROM pragma_table_info('learning_lessons')",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        (
            row.get::<String, _>("name"),
            row.get::<i64, _>("notnull"),
            row.get::<Option<String>, _>("dflt_value"),
        )
    })
    .collect();
    for (name, default_sql) in [("intent", "''"), ("scope", "''")] {
        let (_, notnull, dflt) = columns
            .iter()
            .find(|(column, _, _)| column == name)
            .unwrap_or_else(|| panic!("{name} 列必须在场：{columns:?}"));
        assert_eq!(*notnull, 1, "{name} 必须 NOT NULL（空串 = 旧档语义）");
        assert_eq!(
            dflt.as_deref(),
            Some(default_sql),
            "{name} 缺省必须为空串"
        );
    }
}

/// 旧档容忍：不带两键的旧写法 INSERT 与既有行读回空串（零回填零破坏，
/// purpose/difficulty 先例同款）；带值写入原样落库。
#[tokio::test]
async fn legacy_rows_read_empty_and_values_roundtrip() {
    let pool = setup().await;
    seed_lesson(&pool).await;
    let (intent, scope): (String, String) = sqlx::query_as(
        "SELECT intent, scope FROM learning_lessons WHERE lesson_id = ?",
    )
    .bind(LESSON_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(intent, "", "旧行 intent 缺省空串");
    assert_eq!(scope, "", "旧行 scope 缺省空串");

    sqlx::query(
        "UPDATE learning_lessons SET intent = ?, scope = ? WHERE lesson_id = ?",
    )
    .bind("为后续借用规则提供对象生命期的直观底座")
    .bind("教移动语义的直观图景；不涉及编译器实现")
    .bind(LESSON_ID)
    .execute(&pool)
    .await
    .unwrap();
    let (intent, scope): (String, String) = sqlx::query_as(
        "SELECT intent, scope FROM learning_lessons WHERE lesson_id = ?",
    )
    .bind(LESSON_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(intent, "为后续借用规则提供对象生命期的直观底座");
    assert_eq!(scope, "教移动语义的直观图景；不涉及编译器实现");
}

/// v3 契约不动面：无新表、无新 `_id` 业务键——PRODUCT_TABLES 全库校验照旧
/// 通过（absorb-8 z01 只加两列，126 表计数契约不受影响）。
#[tokio::test]
async fn id_schema_contract_still_holds_with_brief_columns() {
    let pool = setup().await;
    seed_lesson(&pool).await;
    nomifun_db::validate_id_schema_contract(&pool)
        .await
        .expect("id schema contract（learning_lessons 仍以 lesson_id 为业务键）");
}

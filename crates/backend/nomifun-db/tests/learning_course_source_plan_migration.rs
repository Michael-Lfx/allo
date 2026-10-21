//! 迁移 069（`learning_courses.course_source_plan_json`）的契约：建课时点
//! 的来源计划快照列（t1 清点/分诊 + t2 计划 + t3 覆盖的原子 JSON 信封）。
//! Nullable、无 default：NULL = 旧课程/描述流/未启用新管线——与空对象必须
//! 可区分，旧行不回填。CHECK 只放行 JSON 对象（信封恒为
//! `{version, inventory, triage, plan, coverage}`）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";

async fn setup() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

/// 旧课程行（无来源计划）必须保持可读：course_source_plan_json 读出
/// NULL，其余既有列（samples_meta_json/learning_goal）不受影响——不回填、
/// 不报错。
#[tokio::test]
async fn legacy_course_rows_read_back_without_source_plan() {
    let pool = setup().await;
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '旧课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT course_source_plan_json, samples_meta_json FROM learning_courses WHERE course_id = ?",
    )
    .bind(COURSE_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    let plan: Option<String> = sqlx::Row::try_get(&row, "course_source_plan_json").unwrap();
    let meta: Option<String> = sqlx::Row::try_get(&row, "samples_meta_json").unwrap();
    assert!(plan.is_none(), "legacy rows must stay NULL");
    assert!(meta.is_none(), "existing snapshot columns untouched");
}

/// 新行必须存 JSON 对象（version 信封原样读回），且与既有的采样披露列
/// 互不干扰。
#[tokio::test]
async fn source_plan_round_trips_as_an_object() {
    let pool = setup().await;
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, course_source_plan_json, created_at, updated_at) \
         VALUES (?, '计划课程', '', 'general', 1, ?, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .bind(r#"{"version":1,"inventory":{"total_files":2},"triage":{"counts":{"core":2}},"plan":{"goal":"学会排序"},"coverage":{"lessons":1}}"#)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT course_source_plan_json FROM learning_courses WHERE course_id = ?",
    )
    .bind(COURSE_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    let plan: String = sqlx::Row::try_get(&row, "course_source_plan_json").unwrap();
    assert!(plan.contains("\"version\":1"), "{plan}");
    assert!(plan.contains("\"goal\":\"学会排序\""), "{plan}");

    // 数组 / 标量 / 非法 JSON 都必须被 CHECK 拒绝。
    for bad in ["[1]", "\"text\"", "42", "not json"] {
        let result =
            sqlx::query("UPDATE learning_courses SET course_source_plan_json = ? WHERE course_id = ?")
                .bind(bad)
                .bind(COURSE_ID)
                .execute(&pool)
                .await;
        assert!(result.is_err(), "{bad} must be rejected by the CHECK");
    }
}

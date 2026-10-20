//! 迁移 068 学习域合并基线的 samples_meta_json 段（`learning_courses.samples_meta_json`）的契约：采样披露是
//! `samples_json` 的伴随结构，独立列；旧行 NULL（旧快照继续按旧形状读），
//! 新行必须是 JSON 对象。

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

/// 旧课程行（无采样元数据）必须保持可读：samples_meta_json 读出 NULL，
/// learning_goal 读出 048 的默认空串——不回填、不报错。
#[tokio::test]
async fn legacy_course_rows_read_back_without_sampling_meta() {
    let pool = setup().await;
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '旧课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT samples_meta_json, learning_goal FROM learning_courses WHERE course_id = ?")
        .bind(COURSE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    let meta: Option<String> = sqlx::Row::try_get(&row, "samples_meta_json").unwrap();
    let goal: String = sqlx::Row::try_get(&row, "learning_goal").unwrap();
    assert!(meta.is_none(), "legacy rows must stay NULL");
    assert_eq!(goal, "", "048 default: old rows read as an empty goal");
}

/// 新行必须存 JSON 对象（不是数组/标量），且原样读回；
/// 目标文本走既有的 learning_goal 列（NOT NULL），与采样元数据互不干扰。
#[tokio::test]
async fn sampling_meta_round_trips_as_an_object() {
    let pool = setup().await;
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, learning_goal, samples_meta_json, created_at, updated_at) \
         VALUES (?, '目标课程', '', 'general', 1, ?, ?, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .bind("零基础学 Python 数据分析")
    .bind(r#"{"total_files":9,"sampled_files":4,"coherence_files":1,"tree":["01-a.md"],"low_score_files":["z.md"]}"#)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT samples_meta_json, learning_goal FROM learning_courses WHERE course_id = ?")
        .bind(COURSE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    let meta: String = sqlx::Row::try_get(&row, "samples_meta_json").unwrap();
    assert!(meta.contains("\"sampled_files\":4"), "{meta}");
    let goal: String = sqlx::Row::try_get(&row, "learning_goal").unwrap();
    assert_eq!(goal, "零基础学 Python 数据分析");

    // 数组 / 标量 / 非法 JSON 都必须被 CHECK 拒绝。
    for bad in ["[1]", "\"text\"", "42", "not json"] {
        let result = sqlx::query("UPDATE learning_courses SET samples_meta_json = ? WHERE course_id = ?")
            .bind(bad)
            .bind(COURSE_ID)
            .execute(&pool)
            .await;
        assert!(result.is_err(), "{bad} must be rejected by the CHECK");
    }
}

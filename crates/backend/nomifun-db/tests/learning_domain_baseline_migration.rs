//! Q8 fixture（x05/x02）：v1.7.0 形状的存量用户库升级到基线后，学习者行
//! 守恒、图课程数据按 ADR-0009 清除、九种 kind 自建题全存活；DB 只保留数值
//! 域约束（枚举集与跨列规则已移出，ADR-0015）；孤儿表与死列退役。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000002";
const GRAPH: &str = "0190f5fe-7c00-7a00-8abc-000000000003";
const MOD_TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000004";
const MOD_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-000000000005";
const LESSON_A: &str = "0190f5fe-7c00-7a00-8abc-000000000006";
const LESSON_B: &str = "0190f5fe-7c00-7a00-8abc-000000000007";
const LESSON_C: &str = "0190f5fe-7c00-7a00-8abc-000000000008";
const LESSON_D: &str = "0190f5fe-7c00-7a00-8abc-000000000009";
const LESSON_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-00000000000a";
const ACTIVITY: &str = "0190f5fe-7c00-7a00-8abc-00000000000b";
const ACTIVITY_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-00000000000c";
const USER: &str = "0190f5fe-7c00-7a00-8abc-000000000001";
const ENROLL: &str = "0190f5fe-7c00-7a00-8abc-00000000000d";
const ENROLL_TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000013";
const REVIEW_ITEM: &str = "0190f5fe-7c00-7a00-8abc-00000000000e";
const TAG: &str = "0190f5fe-7c00-7a00-8abc-00000000000f";
const CONCEPT: &str = "0190f5fe-7c00-7a00-8abc-000000000010";
const KB: &str = "0190f5fe-7c00-7a00-8abc-000000000011";

async fn pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap()
}

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

async fn run(pool: &SqlitePool, sql: &str) {
    sqlx::query(sql).execute(pool).await.unwrap();
}

/// v1.7.0 形状：传统课程（四态进度、九种自建题、复习流水、标签、概念绑定）
/// + 图课程全家桶（ADR-0009 升级时清除）。
async fn seed_v170(pool: &SqlitePool) {
    run(pool, &format!(
        "INSERT INTO users (user_id, username, email, password_hash, created_at, updated_at) \
         VALUES ('{USER}', 'seed', 'seed@example.com', 'x', 1000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, source_kb_id, version, course_kind, teaching_style, created_at, updated_at) \
         VALUES ('{TRAD}', '传统课程', '', 'general', '{KB}', 1, 'traditional', 'socratic', 1000, 1000), \
                ('{GRAPH}', '图课程', '', 'general', NULL, 1, 'learning_graph', 'standard', 1000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_modules (module_id, course_id, title, position) \
         VALUES ('{MOD_TRAD}', '{TRAD}', 'm', 0), ('{MOD_GRAPH}', '{GRAPH}', 'm', 0)"
    )).await;
    for (lesson, module, position) in [
        (LESSON_A, MOD_TRAD, 0),
        (LESSON_B, MOD_TRAD, 1),
        (LESSON_C, MOD_TRAD, 2),
        (LESSON_D, MOD_TRAD, 3),
        (LESSON_GRAPH, MOD_GRAPH, 0),
    ] {
        run(pool, &format!(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, position, estimated_minutes) \
             VALUES ('{lesson}', '{module}', 'l', '', {position}, 10)"
        )).await;
    }
    run(pool, &format!(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, position, created_at, updated_at, visual, degraded) \
         VALUES ('s1', '{LESSON_A}', 'concept', 't', 0, 1000, 1000, '', 0), \
                ('s1', '{LESSON_GRAPH}', 'concept', 't', 0, 1000, 1000, '', 0)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_activities \
         (activity_id, lesson_id, kind, prompt, config_json, section_key, position) \
         VALUES ('{ACTIVITY}', '{LESSON_A}', 'numeric', 'q', '{{\"answer\":42}}', 's1', 0), \
                ('{ACTIVITY_GRAPH}', '{LESSON_GRAPH}', 'single_choice', 'q', '{{}}', NULL, 0)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_enrollments (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES ('{ENROLL}', '{USER}', '{GRAPH}', 1000, 1000), \
                ('{ENROLL_TRAD}', '{USER}', '{TRAD}', 1000, 1000)"
    )).await;
    for (lesson, enrollment, status, started, completed) in [
        (LESSON_A, ENROLL_TRAD, "completed", "1500", "1600"),
        (LESSON_B, ENROLL_TRAD, "in_progress", "1500", "NULL"),
        (LESSON_C, ENROLL_TRAD, "skipped", "NULL", "NULL"),
        (LESSON_D, ENROLL_TRAD, "not_started", "NULL", "NULL"),
        (LESSON_GRAPH, ENROLL, "completed", "1500", "1600"),
    ] {
        run(pool, &format!(
            "INSERT INTO learning_lesson_progress \
             (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
             VALUES ('{enrollment}', '{lesson}', '{status}', {started}, {completed}, 1600)"
        )).await;
    }
    run(pool, &format!(
        "INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, activity_id, due_at, updated_at) \
         VALUES ('{REVIEW_ITEM}', '{ENROLL}', '{ACTIVITY_GRAPH}', 2000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_review_log \
         (log_id, user_id, source, item_id, rating, rating_source, elapsed_days, review_day, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-000000000012', '{USER}', 'course', '{REVIEW_ITEM}', 0, 'synthetic', 0, 20261102, 1600)"
    )).await;
    for (index, kind) in [
        "single_choice",
        "true_false",
        "reflection",
        "fill_in_blank",
        "multi_choice",
        "numeric",
        "ordering",
        "matching",
        "open_question",
    ]
    .iter()
    .enumerate()
    {
        run(pool, &format!(
            "INSERT INTO learning_custom_questions \
             (custom_question_id, user_id, kind, prompt, due_at, created_at, updated_at, concept_id) \
             VALUES ('0190f5fe-7c00-7a00-8abc-0000000000{:02x}', '{USER}', '{kind}', 'q', 2000, 1000, 1000, '{CONCEPT}')",
            0x20 + index
        )).await;
    }
    run(pool, &format!(
        "INSERT INTO learning_tags (tag_id, name, created_at) VALUES ('{TAG}', '标签', 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_question_tags (question_id, source, tag_id) \
         VALUES ('{ACTIVITY}', 'course', '{TAG}')"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_course_tags (course_id, tag_id) VALUES ('{TRAD}', '{TAG}')"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_concepts (concept_id, course_id, concept_key, title) \
         VALUES ('{CONCEPT}', '{TRAD}', 'k', '概念')"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id) VALUES ('{LESSON_A}', '{CONCEPT}')"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_activity_concepts (activity_id, concept_id) VALUES ('{ACTIVITY}', '{CONCEPT}')"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_course_jobs \
         (job_id, user_id, source, kb_id, request_json, status, created_at, updated_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-000000000030', '{USER}', 'http', '{KB}', '{{}}', 'completed', 1000, 1000)"
    )).await;
}

async fn count(pool: &SqlitePool, table: &str, filter: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE {filter}"))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn v170_fixture_upgrades_with_rows_conserved_and_enum_checks_gone() {
    let pool = pool().await;
    migrate_range(&pool, 0, 69).await;
    seed_v170(&pool).await;

    let before = [
        ("learning_lessons", format!("module_id = '{MOD_TRAD}'")),
        ("learning_lesson_sections", format!("lesson_id = '{LESSON_A}'")),
        ("learning_activities", format!("lesson_id = '{LESSON_A}'")),
        ("learning_lesson_progress", format!("enrollment_id = '{ENROLL_TRAD}'")),
        ("learning_custom_questions", "1 = 1".to_owned()),
        ("learning_question_tags", "1 = 1".to_owned()),
        ("learning_course_tags", "1 = 1".to_owned()),
    ];
    let mut counts = Vec::new();
    for (table, filter) in &before {
        counts.push((table, filter.clone(), count(&pool, table, filter).await));
    }

    migrate_range(&pool, 69, i64::MAX).await;

    // 学习者状态行数守恒（图课程的行按 ADR-0009 清除，不在守恒面）。
    for (table, filter, expected) in counts {
        assert_eq!(count(&pool, table, &filter).await, expected, "{table} row count drifted");
    }
    // 图课程全家桶清零。
    assert_eq!(count(&pool, "learning_courses", &format!("course_id = '{GRAPH}'")).await, 0);
    assert_eq!(count(&pool, "learning_modules", &format!("course_id = '{GRAPH}'")).await, 0);
    assert_eq!(count(&pool, "learning_lessons", &format!("lesson_id = '{LESSON_GRAPH}'")).await, 0);
    assert_eq!(count(&pool, "learning_activities", &format!("activity_id = '{ACTIVITY_GRAPH}'")).await, 0);
    assert_eq!(count(&pool, "learning_enrollments", &format!("course_id = '{GRAPH}'")).await, 0);

    // 九种 kind 自建题全存活。
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM learning_custom_questions ORDER BY kind",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(kinds.len(), 9, "nine custom-question kinds survive: {kinds:?}");

    // 枚举集 CHECK 移除（ADR-0015）：约束外取值可写入。
    for sql in [
        "UPDATE learning_activities SET kind = 'essay' WHERE activity_id = '0190f5fe-7c00-7a00-8abc-00000000000b'",
        "UPDATE learning_lesson_sections SET kind = 'interactive', status = 'archived' WHERE section_key = 's1'",
        "UPDATE learning_lesson_progress SET status = 'paused' WHERE lesson_id = '0190f5fe-7c00-7a00-8abc-000000000006'",
        "UPDATE learning_courses SET course_kind = 'hybrid', teaching_style = 'montessori' WHERE course_id = '0190f5fe-7c00-7a00-8abc-000000000002'",
    ] {
        run(&pool, sql).await;
    }

    // 跨列形态规则也移出 DB（ADR-0015）：completed 但 started_at 空现在能写；
    // 拒绝责任在 progress.rs::validate_progress_shape。
    run(
        &pool,
        "UPDATE learning_lesson_progress SET status = 'completed', started_at = NULL \
         WHERE lesson_id = '0190f5fe-7c00-7a00-8abc-000000000006'",
    )
    .await;

    // 数值/JSON 形状 CHECK 仍生效（保留的 DB 底线）。
    let bad_config = sqlx::query(
        "UPDATE learning_activities SET config_json = '[]' \
         WHERE activity_id = '0190f5fe-7c00-7a00-8abc-00000000000b'",
    )
    .execute(&pool)
    .await;
    assert!(
        bad_config.is_err(),
        "json_valid/config_json shape CHECK must remain"
    );

    // 图课程的复习卡与日志随图课程清除（069 的级联 DELETE）。
    assert_eq!(
        count(&pool, "learning_review_items", &format!("enrollment_id = '{ENROLL}'")).await,
        0
    );
    assert_eq!(
        count(&pool, "learning_review_log", "1 = 1").await,
        0,
        "graph-course review log is removed with its cards"
    );

    // x02 死物退役：孤儿表消失、两处死列消失。
    let retired: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'learning_course_jobs'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retired, 0, "learning_course_jobs is retired");
    let progress_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('learning_lesson_progress')")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!progress_columns.contains(&"extra_json".to_owned()), "extra_json is gone");
    let course_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('learning_courses')")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!course_columns.contains(&"graph_meta_json".to_owned()), "graph_meta_json is gone");

    // 依赖 x02 索引的真实查询仍可跑（契约表结构完整）。
    let row = sqlx::query("SELECT COUNT(*) AS c FROM learning_xp_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("c"), 0);
}

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
// m2 失联子树两形态：课程行缺席（learning_courses 无行）但子树行齐全。
const ORPHAN_COURSE_TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000033";
const ORPHAN_COURSE_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-000000000034";
const MOD_ORPHAN_TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000014";
const MOD_ORPHAN_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-000000000015";
const LESSON_ORPHAN_TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000016";
const LESSON_ORPHAN_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-000000000017";
const ACTIVITY_ORPHAN_TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000018";
const ACTIVITY_ORPHAN_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-000000000019";
const ATTEMPT_ORPHAN_TRAD: &str = "0190f5fe-7c00-7a00-8abc-000000000031";
const ATTEMPT_ORPHAN_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-000000000032";
const ENROLL_ORPHAN_TRAD: &str = "0190f5fe-7c00-7a00-8abc-00000000001a";
const ENROLL_ORPHAN_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-00000000001b";
const REVIEW_ITEM_ORPHAN_TRAD: &str = "0190f5fe-7c00-7a00-8abc-00000000001c";
const REVIEW_ITEM_ORPHAN_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-00000000001d";

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
    seed_v170_orphan_subtrees(pool).await;
    seed_v170_kept_history(pool).await;
}

/// m2 失联子树两形态：课程行已不在 `learning_courses`，子树行齐全——
/// ① 传统删课保留复习（main 时代 `delete_course(delete_reviews=false)` 只
/// 硬删课程行的落库残影）；② 升级前已删图课程的残余（070 §1 按
/// `course_kind` 键定，键不上它们）。两形态在库里形状相同，这正是清理段
/// 必须 kind 无关的原因。注意：失联 question_tags / course_tags 不在此处
/// 播种（既有守恒断言按全表计数），由第二个测试函数自行补种。
async fn seed_v170_orphan_subtrees(pool: &SqlitePool) {
    run(pool, &format!(
        "INSERT INTO learning_modules (module_id, course_id, title, position) \
         VALUES ('{MOD_ORPHAN_TRAD}', '{ORPHAN_COURSE_TRAD}', 'm', 0), \
                ('{MOD_ORPHAN_GRAPH}', '{ORPHAN_COURSE_GRAPH}', 'm', 0)"
    )).await;
    for (lesson, module, position) in [
        (LESSON_ORPHAN_TRAD, MOD_ORPHAN_TRAD, 0),
        (LESSON_ORPHAN_GRAPH, MOD_ORPHAN_GRAPH, 0),
    ] {
        run(pool, &format!(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, position, estimated_minutes) \
             VALUES ('{lesson}', '{module}', '失联课时', '', {position}, 10)"
        )).await;
    }
    run(pool, &format!(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, position, created_at, updated_at, visual, degraded) \
         VALUES ('s1', '{LESSON_ORPHAN_TRAD}', 'concept', 't', 0, 1000, 1000, '', 0), \
                ('s1', '{LESSON_ORPHAN_GRAPH}', 'concept', 't', 0, 1000, 1000, '', 0)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_activities \
         (activity_id, lesson_id, kind, prompt, config_json, section_key, position) \
         VALUES ('{ACTIVITY_ORPHAN_TRAD}', '{LESSON_ORPHAN_TRAD}', 'numeric', 'q', \
                 '{{\"answer\":7}}', 's1', 0), \
                ('{ACTIVITY_ORPHAN_GRAPH}', '{LESSON_ORPHAN_GRAPH}', 'single_choice', 'q', \
                 '{{}}', NULL, 0)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_enrollments (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES ('{ENROLL_ORPHAN_TRAD}', '{USER}', '{ORPHAN_COURSE_TRAD}', 1000, 1000), \
                ('{ENROLL_ORPHAN_GRAPH}', '{USER}', '{ORPHAN_COURSE_GRAPH}', 1000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_lesson_progress \
         (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
         VALUES ('{ENROLL_ORPHAN_TRAD}', '{LESSON_ORPHAN_TRAD}', 'completed', 1500, 1600, 1600), \
                ('{ENROLL_ORPHAN_GRAPH}', '{LESSON_ORPHAN_GRAPH}', 'completed', 1500, 1600, 1600)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, activity_id, due_at, updated_at) \
         VALUES ('{REVIEW_ITEM_ORPHAN_TRAD}', '{ENROLL_ORPHAN_TRAD}', '{ACTIVITY_ORPHAN_TRAD}', 2000, 1000), \
                ('{REVIEW_ITEM_ORPHAN_GRAPH}', '{ENROLL_ORPHAN_GRAPH}', '{ACTIVITY_ORPHAN_GRAPH}', 2000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_attempts \
         (attempt_id, enrollment_id, activity_id, response_json, score, passed, feedback, created_at) \
         VALUES ('{ATTEMPT_ORPHAN_TRAD}', '{ENROLL_ORPHAN_TRAD}', '{ACTIVITY_ORPHAN_TRAD}', \
                 '{{\"value\":7}}', 1.0, 1, '', 1500), \
                ('{ATTEMPT_ORPHAN_GRAPH}', '{ENROLL_ORPHAN_GRAPH}', '{ACTIVITY_ORPHAN_GRAPH}', \
                 '{{\"choice\":0}}', 1.0, 1, '', 1500)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_review_log \
         (log_id, user_id, source, item_id, rating, rating_source, elapsed_days, review_day, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-00000000001e', '{USER}', 'course', '{REVIEW_ITEM_ORPHAN_TRAD}', 3, 'self', 0, 20261102, 1600), \
                ('0190f5fe-7c00-7a00-8abc-00000000001f', '{USER}', 'course', '{REVIEW_ITEM_ORPHAN_GRAPH}', 0, 'synthetic', 0, 20261102, 1600)"
    )).await;
}

/// 打卡统计面（070 既有契约：`learning_review_events` 与 `learning_checkins`
/// 保留，打卡统计不随卡删）——卡片被清后这些行必须原样存活。
async fn seed_v170_kept_history(pool: &SqlitePool) {
    run(pool, &format!(
        "INSERT INTO learning_review_events (event_id, user_id, source, item_id, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-000000000035', '{USER}', 'course', '{REVIEW_ITEM}', 1600), \
                ('0190f5fe-7c00-7a00-8abc-000000000036', '{USER}', 'course', '{REVIEW_ITEM_ORPHAN_TRAD}', 1600), \
                ('0190f5fe-7c00-7a00-8abc-000000000037', '{USER}', 'course', '{REVIEW_ITEM_ORPHAN_GRAPH}', 1600)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_checkins \
         (checkin_id, user_id, review_day, goal, reviewed_count, completed_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-000000000038', '{USER}', 20261102, 5, 3, 1600)"
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

/// m2：课程行失联孤儿子树在基线内一次性清空（070 §8）。两形态——main 时代
/// 传统删课保留复习的残影、升级前已删图课程的残余——库里形状相同（课程行
/// 缺席、子树齐全），清理与 `course_kind` 无关地按「课程行失联」键定。断言
/// 面：失联子树 11 表归零、活课程传统行守恒、打卡统计面（review_events /
/// checkins）行数不变、清理后课时→模块→课程链无悬空。
#[tokio::test]
async fn v170_orphan_course_subtrees_are_swept_regardless_of_kind() {
    let pool = pool().await;
    migrate_range(&pool, 0, 69).await;
    seed_v170(&pool).await;

    // 失联标签行（question_tags / course_tags 的既有守恒断言按全表计数，
    // 不入 seed_v170，由本函数补种）。
    run(&pool, &format!(
        "INSERT INTO learning_question_tags (question_id, source, tag_id) \
         VALUES ('{ACTIVITY_ORPHAN_TRAD}', 'course', '{TAG}'), \
                ('{ACTIVITY_ORPHAN_GRAPH}', 'course', '{TAG}')"
    ))
    .await;
    run(&pool, &format!(
        "INSERT INTO learning_course_tags (course_id, tag_id) \
         VALUES ('{ORPHAN_COURSE_TRAD}', '{TAG}'), ('{ORPHAN_COURSE_GRAPH}', '{TAG}')"
    ))
    .await;

    // 打卡统计面升级前基线：卡片被清后行数必须原样保留。
    let review_events_before: i64 = count(&pool, "learning_review_events", "1 = 1").await;
    let checkins_before: i64 = count(&pool, "learning_checkins", "1 = 1").await;
    assert_eq!(review_events_before, 3);
    assert_eq!(checkins_before, 1);

    migrate_range(&pool, 69, i64::MAX).await;

    // 失联子树 11 表归零（两形态一起断言，kind 无关）。
    let swept = [
        (
            "learning_enrollments",
            format!(
                "course_id IN ('{ORPHAN_COURSE_TRAD}', '{ORPHAN_COURSE_GRAPH}')"
            ),
        ),
        (
            "learning_modules",
            format!(
                "course_id IN ('{ORPHAN_COURSE_TRAD}', '{ORPHAN_COURSE_GRAPH}')"
            ),
        ),
        (
            "learning_lessons",
            format!(
                "lesson_id IN ('{LESSON_ORPHAN_TRAD}', '{LESSON_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_lesson_sections",
            format!(
                "lesson_id IN ('{LESSON_ORPHAN_TRAD}', '{LESSON_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_activities",
            format!(
                "activity_id IN ('{ACTIVITY_ORPHAN_TRAD}', '{ACTIVITY_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_attempts",
            format!(
                "attempt_id IN ('{ATTEMPT_ORPHAN_TRAD}', '{ATTEMPT_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_review_items",
            format!(
                "review_item_id IN ('{REVIEW_ITEM_ORPHAN_TRAD}', '{REVIEW_ITEM_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_review_log",
            format!(
                "source = 'course' AND item_id IN ('{REVIEW_ITEM_ORPHAN_TRAD}', '{REVIEW_ITEM_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_lesson_progress",
            format!(
                "enrollment_id IN ('{ENROLL_ORPHAN_TRAD}', '{ENROLL_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_question_tags",
            format!(
                "source = 'course' AND question_id IN ('{ACTIVITY_ORPHAN_TRAD}', '{ACTIVITY_ORPHAN_GRAPH}')"
            ),
        ),
        (
            "learning_course_tags",
            format!(
                "course_id IN ('{ORPHAN_COURSE_TRAD}', '{ORPHAN_COURSE_GRAPH}')"
            ),
        ),
    ];
    for (table, filter) in &swept {
        assert_eq!(
            count(&pool, table, filter).await,
            0,
            "{table} must not keep rows of a course whose row is gone"
        );
    }

    // 活课程传统行守恒：清理只碰失联子树，现存课程的行原样保留。
    assert_eq!(
        count(&pool, "learning_lessons", &format!("module_id = '{MOD_TRAD}'")).await,
        4
    );
    assert_eq!(
        count(&pool, "learning_activities", &format!("activity_id = '{ACTIVITY}'")).await,
        1
    );
    assert_eq!(
        count(&pool, "learning_enrollments", &format!("enrollment_id = '{ENROLL_TRAD}'")).await,
        1
    );
    assert_eq!(
        count(&pool, "learning_lesson_progress", &format!("enrollment_id = '{ENROLL_TRAD}'"))
            .await,
        4
    );
    assert_eq!(
        count(&pool, "learning_question_tags", &format!("question_id = '{ACTIVITY}'")).await,
        1
    );
    assert_eq!(
        count(&pool, "learning_course_tags", &format!("course_id = '{TRAD}'")).await,
        1
    );

    // 打卡统计面行数不变（070 既有契约：review_events / checkins 不随卡删）。
    assert_eq!(
        count(&pool, "learning_review_events", "1 = 1").await,
        review_events_before,
        "review events are kept history even when their cards are swept"
    );
    assert_eq!(count(&pool, "learning_checkins", "1 = 1").await, checkins_before);

    // 清理后链条干净：课时→模块→课程无悬空引用。
    let dangling_lessons: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons l \
         LEFT JOIN learning_modules m ON m.module_id = l.module_id \
         WHERE m.module_id IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dangling_lessons, 0, "no lesson may outlive its module");
    let dangling_modules: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_modules m \
         LEFT JOIN learning_courses c ON c.course_id = m.course_id \
         WHERE c.course_id IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dangling_modules, 0, "no module may outlive its course");
    let dangling_enrollments: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_enrollments e \
         LEFT JOIN learning_courses c ON c.course_id = e.course_id \
         WHERE c.course_id IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dangling_enrollments, 0, "no enrollment may outlive its course");
}

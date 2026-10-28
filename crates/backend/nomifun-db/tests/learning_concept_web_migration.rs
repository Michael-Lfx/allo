//! 学习域合并基线（068）的迁移契约：原 068–084 十七段学习迁移按序合并为一个
//! 版本。分段升级路径没有真实用户库承载（分支内演进，未随正式版本发布），所以
//! 这里不再逐段推进中间世界，而是从 065 世界的旧学习表出发，一次合并迁移后
//! 断言最终世界：旧图课程退役、概念网重建、终点锚退役、批行状态机就位。

use nomifun_db::sqlx::{self, SqlitePool};

/// 合并迁移之前的全部学习域迁移（按序），拼出与生产一致的旧世界基线。
const LEGACY_MIGRATIONS: &[&str] = &[
    include_str!("../migrations/015_learning_engine.sql"),
    include_str!("../migrations/029_learning_custom_questions.sql"),
    include_str!("../migrations/036_learning_tags.sql"),
    include_str!("../migrations/037_learning_course_jobs.sql"),
    include_str!("../migrations/038_learning_fill_in_blank.sql"),
    include_str!("../migrations/039_learning_review_question_level.sql"),
    include_str!("../migrations/040_learning_on_demand_courses.sql"),
    include_str!("../migrations/042_learning_checkins.sql"),
    include_str!("../migrations/043_learning_archive.sql"),
    include_str!("../migrations/044_learning_edit_pending.sql"),
    include_str!("../migrations/048_learning_graph.sql"),
    include_str!("../migrations/050_learning_sections_and_question_kinds.sql"),
    include_str!("../migrations/051_learning_section_visual.sql"),
    include_str!("../migrations/052_learning_review_log.sql"),
    include_str!("../migrations/053_learning_section_degraded.sql"),
];
const M069: &str = include_str!("../migrations/069_learning_domain_rebuild.sql");

const GRAPH_COURSE: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const TRAD_COURSE: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const MODULE_GRAPH: &str = "0190f5fe-7c00-7a00-8abc-012345678903";
const MODULE_TRAD: &str = "0190f5fe-7c00-7a00-8abc-012345678904";
const GRAPH_LESSON: &str = "0190f5fe-7c00-7a00-8abc-012345678905";
const TRAD_LESSON: &str = "0190f5fe-7c00-7a00-8abc-012345678906";
const GRAPH_ACTIVITY: &str = "0190f5fe-7c00-7a00-8abc-012345678907";
const TRAD_ACTIVITY: &str = "0190f5fe-7c00-7a00-8abc-012345678908";
const ENROLLMENT: &str = "0190f5fe-7c00-7a00-8abc-012345678909";
const USER_ID: &str = "0190f5fe-7c00-7a00-8abc-01234567890a";
const CONCEPT: &str = "0190f5fe-7c00-7a00-8abc-01234567890b";
const CUSTOM_QUESTION: &str = "0190f5fe-7c00-7a00-8abc-01234567890c";
const REVIEW_ITEM: &str = "0190f5fe-7c00-7a00-8abc-01234567890d";

/// 建 015 + 048 的旧世界：一门图课程（含全套派生数据）与一门传统课程
/// （含概念体系与自建题），供合并迁移做破坏性清理与新建表验证。
async fn setup_legacy_world() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    for migration in LEGACY_MIGRATIONS {
        sqlx::query(migration).execute(&pool).await.unwrap();
    }

    for (course_id, module_id, kind, title) in [
        (GRAPH_COURSE, MODULE_GRAPH, "learning_graph", "旧图课程"),
        (TRAD_COURSE, MODULE_TRAD, "traditional", "传统课程"),
    ] {
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, created_at, updated_at) \
             VALUES (?, ?, '', 'general', 1, ?, 1000, 1000)",
        )
        .bind(course_id)
        .bind(title)
        .bind(kind)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules (module_id, course_id, title, position) \
             VALUES (?, ?, 'm', 0)",
        )
        .bind(module_id)
        .bind(course_id)
        .execute(&pool)
        .await
        .unwrap();
    }

    for (lesson_id, module_id) in [(GRAPH_LESSON, MODULE_GRAPH), (TRAD_LESSON, MODULE_TRAD)] {
        sqlx::query(
            "INSERT INTO learning_lessons (lesson_id, module_id, title, position, estimated_minutes) \
             VALUES (?, ?, 'l', 0, 10)",
        )
        .bind(lesson_id)
        .bind(module_id)
        .execute(&pool)
        .await
        .unwrap();
    }

    for activity_id in [GRAPH_ACTIVITY, TRAD_ACTIVITY] {
        sqlx::query(
            "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, position) \
             VALUES (?, ?, 'single_choice', 'q', 0)",
        )
        .bind(activity_id)
        .bind(if activity_id == GRAPH_ACTIVITY { GRAPH_LESSON } else { TRAD_LESSON })
        .execute(&pool)
        .await
        .unwrap();
    }

    sqlx::query(
        "INSERT INTO learning_enrollments (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES (?, ?, ?, 1000, 1000)",
    )
    .bind(ENROLLMENT)
    .bind(USER_ID)
    .bind(GRAPH_COURSE)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_progress \
         (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
         VALUES (?, ?, 'completed', 1500, 1600, 1600)",
    )
    .bind(ENROLLMENT)
    .bind(GRAPH_LESSON)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_attempts \
         (attempt_id, enrollment_id, activity_id, response_json, score, passed, created_at) \
         VALUES (?, ?, ?, '\"a\"', 1.0, 1, 1600)",
    )
    .bind("0190f5fe-7c00-7a00-8abc-01234567890e")
    .bind(ENROLLMENT)
    .bind(GRAPH_ACTIVITY)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, activity_id, due_at, updated_at) \
         VALUES (?, ?, ?, 2000, 1000)",
    )
    .bind(REVIEW_ITEM)
    .bind(ENROLLMENT)
    .bind(GRAPH_ACTIVITY)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_review_log \
         (log_id, user_id, source, item_id, rating, rating_source, elapsed_days, review_day, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-01234567890f', ?, 'course', ?, 3, 'auto', 1, 20260928, 1600)",
    )
    .bind(USER_ID)
    .bind(REVIEW_ITEM)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO learning_concepts (concept_id, course_id, concept_key, title) \
         VALUES (?, ?, 'k', '概念')",
    )
    .bind(CONCEPT)
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id) VALUES (?, ?)",
    )
    .bind(TRAD_LESSON)
    .bind(CONCEPT)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_activity_concepts (activity_id, concept_id) VALUES (?, ?)",
    )
    .bind(TRAD_ACTIVITY)
    .bind(CONCEPT)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_mastery_states (enrollment_id, concept_id, mastery, updated_at) \
         VALUES (?, ?, 0.8, 1000)",
    )
    .bind(ENROLLMENT)
    .bind(CONCEPT)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_custom_questions \
         (custom_question_id, user_id, kind, prompt, due_at, created_at, updated_at, concept_id) \
         VALUES (?, ?, 'single_choice', 'q', 2000, 1000, 1000, ?)",
    )
    .bind(CUSTOM_QUESTION)
    .bind(USER_ID)
    .bind(CONCEPT)
    .execute(&pool)
    .await
    .unwrap();

    pool
}

async fn migrated_pool() -> SqlitePool {
    let pool = setup_legacy_world().await;
    sqlx::query(M069).execute(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn migration_068_removes_graph_world_and_concept_tables() {
    let pool = migrated_pool().await;

    // 图课程全套派生数据清零。
    for table in [
        "learning_courses",
        "learning_modules",
        "learning_lessons",
        "learning_activities",
        "learning_lesson_progress",
        "learning_attempts",
        "learning_review_items",
        "learning_review_log",
        "learning_enrollments",
    ] {
        let sql = match table {
            "learning_courses" | "learning_modules" | "learning_enrollments" => {
                format!("SELECT COUNT(*) FROM {table} WHERE course_id = '{GRAPH_COURSE}'")
            }
            "learning_lessons" => {
                format!("SELECT COUNT(*) FROM {table} WHERE lesson_id = '{GRAPH_LESSON}'")
            }
            "learning_activities" => {
                format!("SELECT COUNT(*) FROM {table} WHERE activity_id = '{GRAPH_ACTIVITY}'")
            }
            _ => format!("SELECT COUNT(*) FROM {table}"),
        };
        let count: i64 = sqlx::query_scalar(&sql).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0, "{table} must be empty after 068");
    }

    // 旧 per-course 概念体系退役；learning_lesson_concepts 由概念网重建，
    // 不在 DROP 名单里。
    let dropped: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN \
         ('learning_graph_prerequisites', 'learning_concepts', 'learning_concept_prerequisites', \
          'learning_activity_concepts', 'learning_mastery_states')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(dropped.is_empty(), "retired tables must be dropped: {dropped:?}");

    // 传统课程本体与内容保留。
    let kept: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons WHERE lesson_id = ?",
    )
    .bind(TRAD_LESSON)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kept, 1, "traditional course content survives");

    // 自建题保留但 concept_id 列已移除。
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('learning_custom_questions')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        !columns.contains(&"concept_id".to_owned()),
        "concept_id column must be gone: {columns:?}"
    );
    let prompts: Vec<String> =
        sqlx::query_scalar("SELECT prompt FROM learning_custom_questions")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(prompts, vec!["q".to_owned()], "custom question row survives");
    // 重建后的 kind CHECK 保持 038 的三题型宽口径：填空自建题必须可落库。
    let fill = sqlx::query(
        "INSERT INTO learning_custom_questions          (custom_question_id, user_id, kind, prompt, due_at, created_at, updated_at)          VALUES ('0190f5fe-7c00-7a00-8abc-012345678917', ?, 'fill_in_blank', 'q', 2000, 1000, 1000)",
    )
    .bind(USER_ID)
    .execute(&pool)
    .await;
    assert!(fill.is_ok(), "fill_in_blank custom question must insert: {:?}", fill.err());
}

#[tokio::test]
async fn migration_068_builds_concept_web_tables() {
    let pool = migrated_pool().await;

    // 登记表：canonical 必填。
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES (?, '因式分解', '[\"因式拆解\"]', '', 1000, 1000)",
    )
    .bind("0190f5fe-7c00-7a00-8abc-012345678910")
    .execute(&pool)
    .await
    .unwrap();
    let empty = sqlx::query(
        "INSERT INTO learning_concept_registry (concept_id, canonical, created_at, updated_at) \
         VALUES (?, '   ', 1000, 1000)",
    )
    .bind("0190f5fe-7c00-7a00-8abc-012345678911")
    .execute(&pool)
    .await;
    assert!(empty.is_err(), "blank canonical must be rejected");

    // 概念网：role/tier CHECK 生效。
    let insert_edge = |role: &'static str, tier: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO learning_lesson_concepts (lesson_id, concept_id, role, tier) \
                 VALUES (?, '0190f5fe-7c00-7a00-8abc-012345678910', ?, ?)",
            )
            .bind(TRAD_LESSON)
            .bind(role)
            .bind(tier)
            .execute(&pool)
            .await
        }
    };
    insert_edge("teaches", "know").await.unwrap();
    assert!(insert_edge("teaches", "know").await.is_err(), "duplicate edge must be rejected");
    assert!(insert_edge("implies", "know").await.is_err(), "unknown role must be rejected");
    assert!(insert_edge("teaches", "mastered").await.is_err(), "unknown tier must be rejected");

    // 生长批次：seq 唯一、node_ids_json 必须是数组。
    sqlx::query(
        "INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678914', ?, 1, '[\"x\"]', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();
    let dup = sqlx::query(
        "INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678915', ?, 1, '[]', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await;
    assert!(dup.is_err(), "duplicate (course, seq) must be rejected");
    let non_array = sqlx::query(
        "INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678916', ?, 2, '{}', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await;
    assert!(non_array.is_err(), "node_ids_json must be an array");
}

/// 合并迁移的终点锚退役段（ADR-0010）：终点表与 compass 列不再落地，
/// structure_json 快照列存在（NULL = 未生成，CHECK 只收 JSON 对象）。
#[tokio::test]
async fn migration_068_retires_endpoints_compass_and_lands_structure_snapshot() {
    let pool = migrated_pool().await;

    let endpoints_table: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'learning_course_endpoints'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(endpoints_table, 0, "the endpoint table must not survive 068");

    let compass_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('learning_courses') \
         WHERE name IN ('compass_md', 'compass_updated_at')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(compass_columns, 0, "compass columns must be dropped");

    let structure_json: Option<String> =
        sqlx::query_scalar("SELECT structure_json FROM learning_courses WHERE course_id = ?")
            .bind(TRAD_COURSE)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(structure_json, None, "structure starts ungenerated (Missing)");

    let snapshot = r#"{"goal":"能独立求解","doors":[]}"#;
    sqlx::query("UPDATE learning_courses SET structure_json = ? WHERE course_id = ?")
        .bind(snapshot)
        .bind(TRAD_COURSE)
        .execute(&pool)
        .await
        .unwrap();
    let bad_snapshot = sqlx::query("UPDATE learning_courses SET structure_json = 'not json' WHERE course_id = ?")
        .bind(TRAD_COURSE)
        .execute(&pool)
        .await;
    assert!(bad_snapshot.is_err(), "structure_json must hold a JSON object");

    // 课程本体与学习目标原样保留（终点只是目标语句的派生）。
    let kept: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_courses WHERE course_id = ?")
            .bind(TRAD_COURSE)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kept, 1, "the course row survives the destructive migration");
}

#[tokio::test]
async fn migration_068_adds_batch_status_lifecycle() {
    let pool = migrated_pool().await;

    // 缺省落 applied（历史批行口径）。
    sqlx::query(
        "INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, created_at)          VALUES ('0190f5fe-7c00-7a00-8abc-01234567891b', ?, 1, '[\"x\"]', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();

    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_growth_batches WHERE status = 'applied'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied, 1, "rows without explicit status default to applied");

    // pending 行合法（生长开始即落）。
    sqlx::query(
        "INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, status, created_at)          VALUES ('0190f5fe-7c00-7a00-8abc-012345678918', ?, 3, '[]', 'pending', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();

    // failed 行合法。
    sqlx::query(
        "INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, status, created_at)          VALUES ('0190f5fe-7c00-7a00-8abc-012345678919', ?, 4, '[]', 'failed', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();

    // 非法：未知状态。
    let invalid = sqlx::query(
        "INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, status, created_at)          VALUES ('0190f5fe-7c00-7a00-8abc-01234567891a', ?, 5, '[]', 'running', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "unknown status must be rejected");
}

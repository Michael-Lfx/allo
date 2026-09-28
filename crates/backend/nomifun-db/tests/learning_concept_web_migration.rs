use nomifun_db::sqlx::{self, SqlitePool};

/// 066 之前的全部学习域迁移（按序），拼出与生产一致的旧世界基线。
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
const M066: &str = include_str!("../migrations/066_retire_graph_dag_and_concepts.sql");
const M067: &str = include_str!("../migrations/067_concept_web_growth.sql");

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
/// （含概念体系与自建题），供 066/067 做破坏性清理与新建表验证。
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

#[tokio::test]
async fn migration_066_removes_graph_world_and_concept_tables() {
    let pool = setup_legacy_world().await;
    sqlx::query(M066).execute(&pool).await.unwrap();

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
        assert_eq!(count, 0, "{table} must be empty after 066");
    }

    // 先修边表与概念体系五表消失。
    let dropped: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN \
         ('learning_graph_prerequisites', 'learning_concepts', 'learning_concept_prerequisites', \
          'learning_lesson_concepts', 'learning_activity_concepts', 'learning_mastery_states')",
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
async fn migration_067_creates_concept_web_tables() {
    let pool = setup_legacy_world().await;
    sqlx::query(M066).execute(&pool).await.unwrap();
    sqlx::query(M067).execute(&pool).await.unwrap();

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

    // 终点锚：course/lesson 引用与完成位。
    sqlx::query(
        "INSERT INTO learning_lessons (lesson_id, module_id, title, position, estimated_minutes) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678912', ?, '终点', 99, 1)",
    )
    .bind(MODULE_TRAD)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_course_endpoints \
         (endpoint_id, course_id, lesson_id, title, goal_note, declared_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678913', ?, '0190f5fe-7c00-7a00-8abc-012345678912', \
                 '独立交易', '能独立完成一笔交易', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();
    let bad_tier = sqlx::query(
        "UPDATE learning_course_endpoints SET completed = 2 WHERE endpoint_id = '0190f5fe-7c00-7a00-8abc-012345678913'",
    )
    .execute(&pool)
    .await;
    assert!(bad_tier.is_err(), "completed must be boolean");

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

    // 罗盘列存在。
    let compass: Option<String> =
        sqlx::query_scalar("SELECT compass_md FROM learning_courses WHERE course_id = ?")
            .bind(TRAD_COURSE)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(compass, None, "compass starts empty");
}

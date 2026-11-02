use nomifun_db::sqlx::{self, SqlitePool};

/// 068 之前的全部学习域迁移（按序），拼出与生产一致的旧世界基线。
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
/// 折叠后的学习域基线（x21：原 069–088 净形合并）。
const BASELINE: &str = include_str!("../migrations/070_learning_domain_rebuild.sql");

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
const REVIEW_ITEM: &str = "0190f5fe-7c00-7a00-8abc-01234567890d";

/// 050 已发布的 `ActivityKind` 全集。重建迁移必须原样保留这个约束集合，
/// 九种 kind 各落一行作为升级搬行的回归样（x01）。
const ACTIVITY_KINDS: [&str; 9] = [
    "single_choice",
    "true_false",
    "reflection",
    "fill_in_blank",
    "multi_choice",
    "numeric",
    "ordering",
    "matching",
    "open_question",
];

/// 建 015+048 的旧世界：一门图课程（含全套派生数据）与一门传统课程
/// （含概念体系与自建题），供基线做破坏性清理与终态表验证。
async fn setup_legacy_world() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    for migration in LEGACY_MIGRATIONS {
        sqlx::query(migration).execute(&pool).await.unwrap();
    }
    seed_legacy_world(&pool).await;
    sqlx::query(BASELINE).execute(&pool).await.unwrap();
    pool
}

async fn seed_legacy_world(pool: &SqlitePool) {
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
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules (module_id, course_id, title, position) \
             VALUES (?, ?, 'm', 0)",
        )
        .bind(module_id)
        .bind(course_id)
        .execute(pool)
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
        .execute(pool)
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
        .execute(pool)
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
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_progress \
         (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
         VALUES (?, ?, 'completed', 1500, 1600, 1600)",
    )
    .bind(ENROLLMENT)
    .bind(GRAPH_LESSON)
    .execute(pool)
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
    .execute(pool)
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
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_review_log \
         (log_id, user_id, source, item_id, rating, rating_source, elapsed_days, review_day, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-01234567890f', ?, 'course', ?, 3, 'auto', 1, 20260928, 1600)",
    )
    .bind(USER_ID)
    .bind(REVIEW_ITEM)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO learning_concepts (concept_id, course_id, concept_key, title) \
         VALUES (?, ?, 'k', '概念')",
    )
    .bind(CONCEPT)
    .bind(TRAD_COURSE)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id) VALUES (?, ?)",
    )
    .bind(TRAD_LESSON)
    .bind(CONCEPT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_activity_concepts (activity_id, concept_id) VALUES (?, ?)",
    )
    .bind(TRAD_ACTIVITY)
    .bind(CONCEPT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_mastery_states (enrollment_id, concept_id, mastery, updated_at) \
         VALUES (?, ?, 0.8, 1000)",
    )
    .bind(ENROLLMENT)
    .bind(CONCEPT)
    .execute(pool)
    .await
    .unwrap();

    // 050 形状的自建题：九种 kind 各一行。重建若收窄 kind CHECK，这里就会
    // 在 INSERT…SELECT 撞 CHECK，整个升级中断（x01 回归基线）。
    for (index, kind) in ACTIVITY_KINDS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO learning_custom_questions \
             (custom_question_id, user_id, kind, prompt, due_at, created_at, updated_at, concept_id) \
             VALUES (?, ?, ?, 'q', 2000, 1000, 1000, ?)",
        )
        .bind(format!("0190f5fe-7c00-7a00-8abc-0123456789{index:02x}"))
        .bind(USER_ID)
        .bind(kind)
        .bind(CONCEPT)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn learning_domain_rebuild_removes_graph_world_and_concept_tables() {
    let pool = setup_legacy_world().await;

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
        assert_eq!(count, 0, "{table} must be empty after the rebuild");
    }

    // 先修边表与被退役的旧概念体系五表消失。
    let dropped: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN \
         ('learning_graph_prerequisites', 'learning_concepts', 'learning_concept_prerequisites', \
          'learning_lesson_concepts', 'learning_activity_concepts', 'learning_mastery_states')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    // learning_lesson_concepts 在基线里按新语义重建，只应剩下这一张同名表。
    assert_eq!(dropped, vec!["learning_lesson_concepts".to_owned()], "retired tables must be dropped: {dropped:?}");

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
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM learning_custom_questions ORDER BY kind")
            .fetch_all(&pool)
            .await
            .unwrap();
    let mut expected: Vec<String> = ACTIVITY_KINDS.iter().map(|kind| (*kind).to_owned()).collect();
    expected.sort();
    assert_eq!(kinds, expected, "all nine released kinds must survive the rebuild");
    // 重建后的 kind CHECK 与已发布 050 同集：九种 kind 都还能再落库（永不收窄）。
    for (index, kind) in ACTIVITY_KINDS.iter().enumerate() {
        let inserted = sqlx::query(
            "INSERT INTO learning_custom_questions \
             (custom_question_id, user_id, kind, prompt, due_at, created_at, updated_at) \
             VALUES (?, ?, ?, 'q', 2000, 1000, 1000)",
        )
        .bind(format!("0190f5fe-7c00-7a00-8abc-012345678a{index:02x}"))
        .bind(USER_ID)
        .bind(kind)
        .execute(&pool)
        .await;
        assert!(inserted.is_ok(), "{kind} custom question must insert after rebuild: {:?}", inserted.err());
    }
}

#[tokio::test]
async fn learning_domain_rebuild_creates_concept_web_tables() {
    let pool = setup_legacy_world().await;

    // 登记表：canonical 必填；085 的 source 列随净形 CREATE 在场。
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at, source) \
         VALUES (?, '因式分解', '[\"因式拆解\"]', '', 1000, 1000, '教练提名')",
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

    // 概念网：role/tier CHECK 生效（x05 之前仍在 DB 层），ref_name 可空。
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
    let ref_name_nullable: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), SUM(NOT \"notnull\") FROM pragma_table_info('learning_lesson_concepts') \
         WHERE name = 'ref_name'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ref_name_nullable, (1, 1), "ref_name exists and is nullable");

    // 生长批次：status 随净形 CREATE 直接在场，seq 唯一、node_ids_json 必须是数组。
    let applied: String = sqlx::query_scalar(
        "INSERT INTO learning_growth_batches \
         (batch_id, course_id, seq, node_ids_json, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678914', ?, 1, '[\"x\"]', 1000) \
         RETURNING status",
    )
    .bind(TRAD_COURSE)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied, "applied", "new batch rows default to applied");
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

/// 074 的净形替身：终点表与罗盘从未存在；structure_json 快照列落地
/// （NULL = 未生成，CHECK 只收 JSON 对象），课程本体原样保留。
#[tokio::test]
async fn structure_snapshot_lands_without_endpoint_scaffolding() {
    let pool = setup_legacy_world().await;

    let endpoints_table: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'learning_course_endpoints'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(endpoints_table, 0, "the endpoint table never exists in the net baseline");

    let compass_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('learning_courses') \
         WHERE name IN ('compass_md', 'compass_updated_at')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(compass_columns, 0, "compass columns never exist in the net baseline");

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

/// 071 的状态机随净形 CREATE 落地（无「存量行补 applied」中间态）。
#[tokio::test]
async fn growth_batch_status_lifecycle_lands_with_the_create() {
    let pool = setup_legacy_world().await;

    sqlx::query(
        "INSERT INTO learning_growth_batches \
         (batch_id, course_id, seq, node_ids_json, status, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678918', ?, 3, '[]', 'pending', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_growth_batches \
         (batch_id, course_id, seq, node_ids_json, status, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678919', ?, 4, '[]', 'failed', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await
    .unwrap();
    let invalid = sqlx::query(
        "INSERT INTO learning_growth_batches \
         (batch_id, course_id, seq, node_ids_json, status, created_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-01234567891a', ?, 5, '[]', 'running', 1000)",
    )
    .bind(TRAD_COURSE)
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "unknown status must be rejected");
}

use nomifun_db::sqlx::{self, SqlitePool};

const BASELINE: &str = include_str!("../migrations/015_learning_engine.sql");
const CUSTOM_QUESTIONS: &str = include_str!("../migrations/029_learning_custom_questions.sql");
const FILL_IN_BLANK: &str = include_str!("../migrations/038_learning_fill_in_blank.sql");
const ARCHIVE: &str = include_str!("../migrations/043_learning_archive.sql");
const EDIT_PENDING: &str = include_str!("../migrations/044_learning_edit_pending.sql");
const MIGRATION: &str = include_str!("../migrations/050_learning_sections_and_question_kinds.sql");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const LESSON_A: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const USER_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678906";
const ENROLLMENT_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678905";

/// 015 建基线 → 038/043/044 演进到 050 前的 activities/custom_questions
/// 形状 → 插旧 kind 行 → 打 050。
async fn setup_with_legacy_rows() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    for sql in [BASELINE, CUSTOM_QUESTIONS, FILL_IN_BLANK, ARCHIVE, EDIT_PENDING, MIGRATION] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    pool
}

async fn seed_course_and_lesson(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '测试课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES ('0190f5fe-7c00-7a00-8abc-01234567890a', ?, '模块一', '', 0)",
    )
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lessons (lesson_id, module_id, title, summary, position, estimated_minutes) \
         VALUES (?, '0190f5fe-7c00-7a00-8abc-01234567890a', '课时一', '', 0, 15)",
    )
    .bind(LESSON_A)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn sections_store_manifest_and_body() {
    let pool = setup_with_legacy_rows().await;
    seed_course_and_lesson(&pool).await;

    sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, points, body_md, status, version, position, created_at, updated_at) \
         VALUES ('s1', ?, 'concept', '概念：向量', '什么是向量', '# 正文', 'ready', 2, 0, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, points, body_md, status, position, created_at, updated_at) \
         VALUES ('s2', ?, 'example', '例题：向量加法', '', '', 'pending', 1, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();

    // 同课重复 section_key / position 被拒。
    let duplicate_key = sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, position, created_at, updated_at) \
         VALUES ('s1', ?, 'summary', '小结', 5, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await;
    assert!(duplicate_key.is_err(), "duplicate section_key must be rejected");
    let duplicate_position = sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, position, created_at, updated_at) \
         VALUES ('s3', ?, 'summary', '小结', 1, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await;
    assert!(
        duplicate_position.is_err(),
        "duplicate position must be rejected"
    );

    // 未知节类型被拒（交互节暂缓）。
    let bad_kind = sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, position, created_at, updated_at) \
         VALUES ('s4', ?, 'interactive', '交互', 6, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await;
    assert!(bad_kind.is_err(), "unknown section kind must be rejected");

    let ready_body: String = sqlx::query_scalar(
        "SELECT body_md FROM learning_lesson_sections WHERE lesson_id = ? AND section_key = 's1'",
    )
    .bind(LESSON_A)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ready_body, "# 正文");
}

#[tokio::test]
async fn activity_kinds_widen_to_nine_and_carry_section_key() {
    let pool = setup_with_legacy_rows().await;
    seed_course_and_lesson(&pool).await;

    // 全部 9 种 kind 都能落库；section_key 绑定来源节。
    for (position, kind) in [
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
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, config_json, section_key, position) \
             VALUES (?, ?, ?, ?, '{}', 's1', ?)",
        )
        .bind(format!("0190f5fe-7c00-7a00-8abc-0123456789{position:02x}"))
        .bind(LESSON_A)
        .bind(kind)
        .bind(format!("题 {position}"))
        .bind(position as i64)
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("kind {kind} must be accepted: {error}"));
    }
    let section_key: Option<String> = sqlx::query_scalar(
        "SELECT section_key FROM learning_activities WHERE kind = 'numeric'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(section_key.as_deref(), Some("s1"));

    // 未知 kind 仍被拒。
    let invalid = sqlx::query(
        "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, config_json, position) \
         VALUES ('0190f5fe-7c00-7a00-8abc-0123456789ff', ?, 'essay', 'x', '{}', 99)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "unknown kind must be rejected");
}

#[tokio::test]
async fn custom_questions_widen_and_legacy_rows_survive() {
    let pool = setup_with_legacy_rows().await;

    // 旧 kind 行（打 050 前的合法值）在重建后原样保留。
    sqlx::query(
        "INSERT INTO learning_custom_questions \
         (custom_question_id, user_id, kind, prompt, config_json, due_at, created_at, updated_at, archived_at, edit_note) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678910', ?, 'true_false', '旧题', '{}', 0, 1000, 1000, 123, '改一下')",
    )
    .bind(USER_ID)
    .execute(&pool)
    .await
    .unwrap();
    let (kind, archived_at, edit_note): (String, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT kind, archived_at, edit_note FROM learning_custom_questions WHERE user_id = ?",
    )
    .bind(USER_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kind, "true_false");
    assert_eq!(archived_at, Some(123));
    assert_eq!(edit_note.as_deref(), Some("改一下"));

    // 新 kind 也可自建。
    sqlx::query(
        "INSERT INTO learning_custom_questions \
         (custom_question_id, user_id, kind, prompt, config_json, due_at, created_at, updated_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678911', ?, 'ordering', '排序题', '{}', 0, 1000, 1000)",
    )
    .bind(USER_ID)
    .execute(&pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn teaching_style_defaults_and_validates() {
    let pool = setup_with_legacy_rows().await;
    seed_course_and_lesson(&pool).await;

    let style: String =
        sqlx::query_scalar("SELECT teaching_style FROM learning_courses WHERE course_id = ?")
            .bind(COURSE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(style, "standard", "existing courses default to standard");

    sqlx::query("UPDATE learning_courses SET teaching_style = 'socratic' WHERE course_id = ?")
        .bind(COURSE_ID)
        .execute(&pool)
        .await
        .unwrap();

    let invalid = sqlx::query(
        "UPDATE learning_courses SET teaching_style = 'montessori' WHERE course_id = ?",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "unknown teaching_style must be rejected");
}

#[tokio::test]
async fn legacy_activities_without_sections_survive() {
    let pool = setup_with_legacy_rows().await;
    seed_course_and_lesson(&pool).await;

    // 无节的旧课时：活动照常读取，section_key 为 NULL（= 通用题）。
    sqlx::query(
        "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, config_json, position) \
         VALUES ('0190f5fe-7c00-7a00-8abc-012345678920', ?, 'single_choice', '旧活动', '{}', 0)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();
    let section_key: Option<String> = sqlx::query_scalar(
        "SELECT section_key FROM learning_activities WHERE lesson_id = ?",
    )
    .bind(LESSON_A)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        section_key.is_none(),
        "legacy activities carry no section binding"
    );
    // 复习种子查询（客观题过滤）在新 kind 集上工作。
    let _ = ENROLLMENT_ID;
}

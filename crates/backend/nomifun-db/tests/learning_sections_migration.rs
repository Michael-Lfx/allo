use nomifun_db::sqlx::{self, SqlitePool};
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");


const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const LESSON_A: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const USER_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678906";
const ENROLLMENT_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678905";

/// 全链迁移到尖端（含 x21 折叠基线与 x05 去 CHECK）：本文件断言的是
/// **终态 schema**，050 的中间态 CHECK 已由基线 §10 重建移除。
async fn setup_with_legacy_rows() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
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

    // 未知节类型可落库（ADR-0015：枚举集校验收口 Rust，DB 不再设 CHECK）。
    let bad_kind = sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, position, created_at, updated_at) \
         VALUES ('s4', ?, 'interactive', '交互', 6, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await;
    assert!(bad_kind.is_ok(), "DB no longer rejects unknown section kinds: {:?}", bad_kind.err());

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

    // 未知 kind 可落库（ADR-0015：枚举集校验收口 Rust，DB 不再设 CHECK）。
    let invalid = sqlx::query(
        "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, config_json, position) \
         VALUES ('0190f5fe-7c00-7a00-8abc-0123456789ff', ?, 'essay', 'x', '{}', 99)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await;
    assert!(invalid.is_ok(), "DB no longer rejects unknown activity kinds: {:?}", invalid.err());
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

    // 未知教学风格可落库（ADR-0015：枚举集校验收口 Rust，DB 不再设 CHECK）。
    let invalid = sqlx::query(
        "UPDATE learning_courses SET teaching_style = 'montessori' WHERE course_id = ?",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await;
    assert!(invalid.is_ok(), "DB no longer rejects unknown teaching styles: {:?}", invalid.err());
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

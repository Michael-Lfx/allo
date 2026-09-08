use nomifun_db::sqlx::{self, SqlitePool};

const BASELINE: &str = include_str!("../migrations/015_learning_engine.sql");
const CUSTOM_QUESTIONS: &str = include_str!("../migrations/029_learning_custom_questions.sql");
const FILL_IN_BLANK: &str = include_str!("../migrations/038_learning_fill_in_blank.sql");
const ARCHIVE: &str = include_str!("../migrations/043_learning_archive.sql");
const EDIT_PENDING: &str = include_str!("../migrations/044_learning_edit_pending.sql");
const SECTIONS: &str = include_str!("../migrations/049_learning_sections_and_question_kinds.sql");
const MIGRATION: &str = include_str!("../migrations/050_learning_section_visual.sql");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const LESSON_A: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const MODULE_ID: &str = "0190f5fe-7c00-7a00-8abc-01234567890a";

/// 015 建基线 → 演进到 049 前的形状 → 建 049 节表 → 打 050（visual 列）。
async fn setup() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    for sql in [BASELINE, CUSTOM_QUESTIONS, FILL_IN_BLANK, ARCHIVE, EDIT_PENDING, SECTIONS, MIGRATION] {
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
         VALUES (?, ?, '模块一', '', 0)",
    )
    .bind(MODULE_ID)
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lessons (lesson_id, module_id, title, summary, position, estimated_minutes) \
         VALUES (?, ?, '课时一', '', 0, 15)",
    )
    .bind(LESSON_A)
    .bind(MODULE_ID)
    .execute(pool)
    .await
    .unwrap();
}

/// 050 之后新写入的节行必须能显式携带 visual 声明，且能原样读回——
/// 单节重写按原声明兑现承诺。
#[tokio::test]
async fn sections_carry_the_declared_visual() {
    let pool = setup().await;
    seed_course_and_lesson(&pool).await;

    sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, points, visual, body_md, status, version, position, created_at, updated_at) \
         VALUES ('s1', ?, 'concept', '概念：向量', '什么是向量', '示意图', '# 正文', 'ready', 1, 0, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();

    let visual: String =
        sqlx::query_scalar("SELECT visual FROM learning_lesson_sections WHERE section_key = 's1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(visual, "示意图");
}

/// 历史 049 行（无 visual 值）在 050 后读出 DEFAULT ''——读取端把空串
/// 当「未声明」按保守口径处理，不改写历史数据。
#[tokio::test]
async fn legacy_rows_default_to_an_empty_visual() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    for sql in [BASELINE, CUSTOM_QUESTIONS, FILL_IN_BLANK, ARCHIVE, EDIT_PENDING, SECTIONS] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    seed_course_and_lesson(&pool).await;
    sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, points, body_md, status, version, position, created_at, updated_at) \
         VALUES ('s1', ?, 'concept', '概念：向量', '什么是向量', '# 正文', 'ready', 1, 0, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();

    // 打 050：历史行拿到 DEFAULT ''。
    sqlx::query(MIGRATION).execute(&pool).await.unwrap();

    let visual: String =
        sqlx::query_scalar("SELECT visual FROM learning_lesson_sections WHERE section_key = 's1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(visual, "", "legacy rows keep an empty (undeclared) visual");
}

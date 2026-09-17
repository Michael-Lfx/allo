use nomifun_db::sqlx::{self, SqlitePool};

const BASELINE: &str = include_str!("../migrations/015_learning_engine.sql");
const CUSTOM_QUESTIONS: &str = include_str!("../migrations/029_learning_custom_questions.sql");
const FILL_IN_BLANK: &str = include_str!("../migrations/038_learning_fill_in_blank.sql");
const ARCHIVE: &str = include_str!("../migrations/043_learning_archive.sql");
const EDIT_PENDING: &str = include_str!("../migrations/044_learning_edit_pending.sql");
const SECTIONS: &str = include_str!("../migrations/050_learning_sections_and_question_kinds.sql");
const VISUAL: &str = include_str!("../migrations/051_learning_section_visual.sql");
const REVIEW_LOG: &str = include_str!("../migrations/052_learning_review_log.sql");
const MIGRATION: &str = include_str!("../migrations/053_learning_section_degraded.sql");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const LESSON_A: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const MODULE_ID: &str = "0190f5fe-7c00-7a00-8abc-01234567890a";

/// 015 建基线 → 演进到 052 前的形状 → 逐个打到 053（degraded 列）。
async fn setup() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    for sql in [
        BASELINE,
        CUSTOM_QUESTIONS,
        FILL_IN_BLANK,
        ARCHIVE,
        EDIT_PENDING,
        SECTIONS,
        VISUAL,
        REVIEW_LOG,
        MIGRATION,
    ] {
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

/// 053 之后节行必须能显式携带 degraded 标记并原样读回；省略该列的插入
/// 拿 DEFAULT 0（生成期正常落库的节不是降级产物）。
#[tokio::test]
async fn sections_carry_the_degraded_flag() {
    let pool = setup().await;
    seed_course_and_lesson(&pool).await;

    sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, points, visual, body_md, degraded, status, version, position, created_at, updated_at) \
         VALUES ('s1', ?, 'concept', '概念：向量', '什么是向量', '示意图', '# 正文', 1, 'ready', 1, 0, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_sections \
         (section_key, lesson_id, kind, title, points, visual, body_md, degraded, status, version, position, created_at, updated_at) \
         VALUES ('s2', ?, 'practice', '练习：向量判断', '自测', '无', '# 练习', 0, 'ready', 1, 1, 1000, 1000)",
    )
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();

    let degraded: Vec<i64> = sqlx::query_scalar(
        "SELECT degraded FROM learning_lesson_sections ORDER BY position",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(degraded, vec![1, 0], "the degraded flag survives the round trip");
}

/// 历史 052 行（无 degraded 值）在 053 后读出 DEFAULT 0——历史节不是降级
/// 产物，读取端不把旧课时误标为降级。
#[tokio::test]
async fn legacy_rows_default_to_not_degraded() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    for sql in [
        BASELINE,
        CUSTOM_QUESTIONS,
        FILL_IN_BLANK,
        ARCHIVE,
        EDIT_PENDING,
        SECTIONS,
        VISUAL,
        REVIEW_LOG,
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
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

    // 打 053：历史行拿到 DEFAULT 0。
    sqlx::query(MIGRATION).execute(&pool).await.unwrap();

    let degraded: i64 =
        sqlx::query_scalar("SELECT degraded FROM learning_lesson_sections WHERE section_key = 's1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(degraded, 0, "legacy rows are not marked degraded");
}

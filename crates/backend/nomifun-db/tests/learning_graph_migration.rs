use nomifun_db::sqlx::{self, Row, SqlitePool};

const BASELINE: &str = include_str!("../migrations/015_learning_engine.sql");
const MIGRATION: &str = include_str!("../migrations/048_learning_graph.sql");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const LESSON_A: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const LESSON_B: &str = "0190f5fe-7c00-7a00-8abc-012345678903";
const LESSON_C: &str = "0190f5fe-7c00-7a00-8abc-012345678904";
const ENROLLMENT_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678905";
const USER_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678906";

/// 015 建出旧版 learning_lesson_progress，插入旧 CHECK 下的行，再打 048 做 rebuild。
async fn setup_with_legacy_rows() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(BASELINE).execute(&pool).await.unwrap();

    for (index, lesson_id) in [LESSON_A, LESSON_B, LESSON_C].into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO learning_lessons (lesson_id, module_id, title, summary, position, estimated_minutes) \
             VALUES (?, '0190f5fe-7c00-7a00-8abc-01234567890a', ?, '', ?, 10)",
        )
        .bind(lesson_id)
        .bind(lesson_id)
        .bind(index as i64)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO learning_enrollments (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES (?, ?, ?, 1000, 1000)",
    )
    .bind(ENROLLMENT_ID)
    .bind(USER_ID)
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();
    // 旧行：not_started / in_progress / completed 各一条（旧 CHECK 合法）。
    let legacy: &[(&str, &str, Option<i64>, Option<i64>)] = &[
        (LESSON_A, "not_started", None, None),
        (LESSON_B, "in_progress", Some(1500), None),
        (LESSON_C, "completed", Some(1600), Some(1700)),
    ];
    for (lesson_id, status, started_at, completed_at) in legacy {
        sqlx::query(
            "INSERT INTO learning_lesson_progress \
             (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 1800)",
        )
        .bind(ENROLLMENT_ID)
        .bind(lesson_id)
        .bind(status)
        .bind(started_at)
        .bind(completed_at)
        .execute(&pool)
        .await
        .unwrap();
    }

    sqlx::query(MIGRATION).execute(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn rebuild_preserves_legacy_progress_rows() {
    let pool = setup_with_legacy_rows().await;
    let rows: Vec<(String, String, Option<i64>, Option<i64>, i64)> = sqlx::query_as(
        "SELECT lesson_id, status, started_at, completed_at, updated_at \
         FROM learning_lesson_progress ORDER BY lesson_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 3, "rebuild must not lose rows");
    assert_eq!(rows[0], (LESSON_A.to_owned(), "not_started".into(), None, None, 1800));
    assert_eq!(
        rows[1],
        (LESSON_B.to_owned(), "in_progress".into(), Some(1500), None, 1800)
    );
    assert_eq!(
        rows[2],
        (LESSON_C.to_owned(), "completed".into(), Some(1600), Some(1700), 1800)
    );
}

#[tokio::test]
async fn skipped_rows_requires_completed_at_null() {
    let pool = setup_with_legacy_rows().await;
    // 合法：not_started 行跳过，时间戳全空。
    sqlx::query(
        "UPDATE learning_lesson_progress SET status = 'skipped', updated_at = 2000 \
         WHERE enrollment_id = ? AND lesson_id = ?",
    )
    .bind(ENROLLMENT_ID)
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();
    // 合法：in_progress 行跳过，保留 started_at 历史。
    sqlx::query(
        "UPDATE learning_lesson_progress SET status = 'skipped', completed_at = NULL, updated_at = 2000 \
         WHERE enrollment_id = ? AND lesson_id = ?",
    )
    .bind(ENROLLMENT_ID)
    .bind(LESSON_B)
    .execute(&pool)
    .await
    .unwrap();
    // 非法：completed 行跳过但 completed_at 仍在。
    let invalid = sqlx::query(
        "UPDATE learning_lesson_progress SET status = 'skipped', updated_at = 2000 \
         WHERE enrollment_id = ? AND lesson_id = ?",
    )
    .bind(ENROLLMENT_ID)
    .bind(LESSON_C)
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "skipped with completed_at must be rejected");

    // 非法：未知状态。
    let unknown = sqlx::query(
        "UPDATE learning_lesson_progress SET status = 'mastered', started_at = NULL, completed_at = NULL, updated_at = 2000 \
         WHERE enrollment_id = ? AND lesson_id = ?",
    )
    .bind(ENROLLMENT_ID)
    .bind(LESSON_C)
    .execute(&pool)
    .await;
    assert!(unknown.is_err(), "unknown status must be rejected");
}

#[tokio::test]
async fn progress_rows_carry_extra_json_extension_column() {
    let pool = setup_with_legacy_rows().await;
    // 旧行拷贝后 extra_json 取默认 '{}'。
    let extra: String = sqlx::query_scalar(
        "SELECT extra_json FROM learning_lesson_progress WHERE lesson_id = ?",
    )
    .bind(LESSON_B)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(extra, "{}");

    // 未来按用户的学习度量（掌握程度/遗忘曲线）先落 extra_json。
    sqlx::query(
        "UPDATE learning_lesson_progress SET extra_json = ? WHERE lesson_id = ?",
    )
    .bind(r#"{"mastery":0.9,"fsrs":{"stability_days":3.5}}"#)
    .bind(LESSON_B)
    .execute(&pool)
    .await
    .unwrap();
    let extra: String = sqlx::query_scalar(
        "SELECT extra_json FROM learning_lesson_progress WHERE lesson_id = ?",
    )
    .bind(LESSON_B)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(extra, r#"{"mastery":0.9,"fsrs":{"stability_days":3.5}}"#);

    // 非法：extra_json 必须是 object。
    let invalid = sqlx::query(
        "UPDATE learning_lesson_progress SET extra_json = '[1]' WHERE lesson_id = ?",
    )
    .bind(LESSON_B)
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "non-object extra_json must be rejected");
}

#[tokio::test]
async fn prerequisite_edges_accept_kind_and_extra_defaults() {
    let pool = setup_with_legacy_rows().await;
    sqlx::query(
        "INSERT INTO learning_graph_prerequisites \
         (course_id, lesson_id, prerequisite_lesson_id, reason) \
         VALUES (?, ?, ?, '缺了它不行')",
    )
    .bind(COURSE_ID)
    .bind(LESSON_B)
    .bind(LESSON_A)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT kind, extra_json, reason FROM learning_graph_prerequisites \
         WHERE lesson_id = ? AND prerequisite_lesson_id = ?",
    )
    .bind(LESSON_B)
    .bind(LESSON_A)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("kind"), "prerequisite");
    assert_eq!(row.get::<String, _>("extra_json"), "{}");
    assert_eq!(row.get::<String, _>("reason"), "缺了它不行");
}

#[tokio::test]
async fn prerequisite_edges_reject_self_loops_duplicates_and_bad_ids() {
    let pool = setup_with_legacy_rows().await;
    let insert = |lesson_id: &'static str, prerequisite: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO learning_graph_prerequisites \
                 (course_id, lesson_id, prerequisite_lesson_id) VALUES (?, ?, ?)",
            )
            .bind(COURSE_ID)
            .bind(lesson_id)
            .bind(prerequisite)
            .execute(&pool)
            .await
        }
    };

    assert!(
        insert(LESSON_A, LESSON_A).await.is_err(),
        "self loop must be rejected"
    );
    insert(LESSON_B, LESSON_A).await.unwrap();
    assert!(
        insert(LESSON_B, LESSON_A).await.is_err(),
        "duplicate edge must be rejected"
    );
    assert!(
        insert("not-a-uuid", LESSON_A).await.is_err(),
        "non-UUIDv7 lesson_id must be rejected"
    );
}

#[tokio::test]
async fn course_kind_defaults_to_traditional_and_validates() {
    let pool = setup_with_legacy_rows().await;
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '默认课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();
    let kind: String =
        sqlx::query_scalar("SELECT course_kind FROM learning_courses WHERE course_id = ?")
            .bind(COURSE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind, "traditional", "existing courses stay traditional");

    // 非法：未知 kind。
    let invalid = sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, course_kind, created_at, updated_at) \
         VALUES (?, 'x', '', 'general', 1, 'dag', 1000, 1000)",
    )
    .bind("0190f5fe-7c00-7a00-8abc-012345678907")
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "unknown course_kind must be rejected");
}

#[tokio::test]
async fn graph_meta_json_must_be_an_object() {
    let pool = setup_with_legacy_rows().await;
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, learning_scope, graph_meta_json, created_at, updated_at) \
         VALUES (?, '学习图', '', 'general', 1, 'learning_graph', '目标', '范围', ?, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .bind(r#"{"audit":{"findings":[]},"generation":{"expected_units":42}}"#)
    .execute(&pool)
    .await
    .unwrap();
    let meta: String =
        sqlx::query_scalar("SELECT graph_meta_json FROM learning_courses WHERE course_id = ?")
            .bind(COURSE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(meta, r#"{"audit":{"findings":[]},"generation":{"expected_units":42}}"#);

    // 非法：graph_meta_json 必须是 object。
    let invalid = sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, learning_scope, graph_meta_json, created_at, updated_at) \
         VALUES (?, '学习图2', '', 'general', 1, 'learning_graph', '目标', '范围', '[]', 1000, 1000)",
    )
    .bind("0190f5fe-7c00-7a00-8abc-012345678908")
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "non-object graph_meta_json must be rejected");
}

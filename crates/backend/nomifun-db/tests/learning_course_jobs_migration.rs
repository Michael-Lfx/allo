use nomifun_db::sqlx::{self, Row, SqlitePool};

const MIGRATION: &str = include_str!("../migrations/037_learning_course_jobs.sql");

const JOB_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const USER_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678902";
const KB_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678903";

async fn setup() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(MIGRATION).execute(&pool).await.unwrap();
    pool
}

fn request_json() -> String {
    r#"{"knowledge_base_id":"0190f5fe-7c00-7a00-8abc-012345678903","domain":"trading"}"#.to_owned()
}

const INSERT_JOB: &str = concat!(
    "INSERT INTO learning_course_jobs ",
    "(job_id, user_id, source, kb_id, request_json, created_at, updated_at) ",
    "VALUES (?, ?, 'http', ?, ?, 1000, 1000)",
);

const INSERT_JOB_WITH_STATUS: &str = concat!(
    "INSERT INTO learning_course_jobs ",
    "(job_id, user_id, source, kb_id, request_json, status, created_at, updated_at) ",
    "VALUES (?, ?, 'http', ?, ?, ?, 1000, 1000)",
);

#[tokio::test]
async fn migration_creates_table_and_accepts_a_job_row() {
    let pool = setup().await;
    sqlx::query(INSERT_JOB)
        .bind(JOB_ID)
        .bind(USER_ID)
        .bind(KB_ID)
        .bind(request_json())
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT job_id, status, current_module, current_lesson, total_lessons, cancel_requested FROM learning_course_jobs WHERE job_id = ?")
        .bind(JOB_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("job_id"), JOB_ID);
    assert_eq!(row.get::<String, _>("status"), "queued");
    assert_eq!(row.get::<i64, _>("current_module"), 0);
    assert_eq!(row.get::<i64, _>("cancel_requested"), 0);
}

#[tokio::test]
async fn migration_rejects_unknown_status_and_source_values() {
    let pool = setup().await;
    let invalid_status = sqlx::query(INSERT_JOB_WITH_STATUS)
        .bind(JOB_ID)
        .bind(USER_ID)
        .bind(KB_ID)
        .bind(request_json())
        .bind("running")
        .execute(&pool)
        .await;
    assert!(invalid_status.is_err(), "unknown status must be rejected");

    let invalid_source = sqlx::query(
        "INSERT INTO learning_course_jobs (job_id, user_id, source, kb_id, request_json, created_at, updated_at) VALUES (?, ?, 'api', ?, ?, 1000, 1000)",
    )
    .bind(JOB_ID)
    .bind(USER_ID)
    .bind(KB_ID)
    .bind(request_json())
    .execute(&pool)
    .await;
    assert!(invalid_source.is_err(), "unknown source must be rejected");
}

#[tokio::test]
async fn migration_rejects_non_uuidv7_job_id() {
    let pool = setup().await;
    let invalid = sqlx::query(
        "INSERT INTO learning_course_jobs (job_id, user_id, source, kb_id, request_json, created_at, updated_at) VALUES ('not-a-uuid', ?, 'http', ?, ?, 1000, 1000)",
    )
    .bind(USER_ID)
    .bind(KB_ID)
    .bind(request_json())
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "non-UUIDv7 job_id must be rejected");
}

#[tokio::test]
async fn migration_persists_progress_and_artifacts_columns() {
    let pool = setup().await;
    sqlx::query(
        "INSERT INTO learning_course_jobs (job_id, user_id, session_id, source, kb_id, request_json, status, current_module, current_lesson, total_lessons, samples_json, blueprint_json, lesson_outputs_json, cancel_requested, created_at, updated_at) VALUES (?, ?, '0190f5fe-7c00-7a00-8abc-012345678904', 'agent', ?, ?, 'lessons', 1, 2, 9, '[[\"a.md\",\"# A\"]]', '{}', '[{\"summary\":\"s\"}]', 1, 1000, 2000)",
    )
    .bind(JOB_ID)
    .bind(USER_ID)
    .bind(KB_ID)
    .bind(request_json())
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT status, session_id, current_module, current_lesson, total_lessons, samples_json, blueprint_json, lesson_outputs_json, cancel_requested, updated_at FROM learning_course_jobs WHERE job_id = ?",
    )
    .bind(JOB_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("status"), "lessons");
    assert_eq!(
        row.get::<Option<String>, _>("session_id").as_deref(),
        Some("0190f5fe-7c00-7a00-8abc-012345678904")
    );
    assert_eq!(row.get::<i64, _>("current_module"), 1);
    assert_eq!(row.get::<i64, _>("current_lesson"), 2);
    assert_eq!(row.get::<i64, _>("total_lessons"), 9);
    assert_eq!(row.get::<i64, _>("cancel_requested"), 1);
    assert_eq!(row.get::<i64, _>("updated_at"), 2000);
}

#[tokio::test]
async fn migration_requires_user_kb_and_request() {
    let pool = setup().await;
    let missing_kb = sqlx::query(
        "INSERT INTO learning_course_jobs (job_id, user_id, source, request_json, created_at, updated_at) VALUES (?, ?, 'http', ?, 1000, 1000)",
    )
    .bind(JOB_ID)
    .bind(USER_ID)
    .bind(request_json())
    .execute(&pool)
    .await;
    assert!(missing_kb.is_err(), "missing kb_id must be rejected");

    let missing_request = sqlx::query(
        "INSERT INTO learning_course_jobs (job_id, user_id, source, kb_id, created_at, updated_at) VALUES (?, ?, 'http', ?, 1000, 1000)",
    )
    .bind(JOB_ID)
    .bind(USER_ID)
    .bind(KB_ID)
    .execute(&pool)
    .await;
    assert!(missing_request.is_err(), "missing request_json must be rejected");
}

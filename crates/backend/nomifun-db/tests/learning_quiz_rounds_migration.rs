//! 迁移 084（quiz 回合记录表，q2）的契约：终态三态形状 CHECK（active 零
//! 结算面 / completed 必带作答对账与总额 / abandoned 只留情境）、round_id/
//! user_id/course_id 的 UUIDv7 形状地板、同课程至多一个 active 回合的部分
//! 唯一索引（supersede 流的并发地板）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const USER: &str = "0190f5fe-7c00-7a00-8abc-111111111111";
const USER_2: &str = "0190f5fe-7c00-7a00-8abc-121111111111";
const COURSE: &str = "0190f5fe-7c00-7a00-8abc-222222222222";
const ROUND: &str = "0190f5fe-7c00-7a00-8abc-333333333333";
const ROUND_2: &str = "0190f5fe-7c00-7a00-8abc-343333333333";

async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn accepts_active_then_completed_transition_shapes() {
    let pool = migrated_pool().await;
    sqlx::query(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, status, plan_json) \
         VALUES (?, ?, ?, 1, 'active', '[{\"node\":\"n\",\"node_name\":\"N\",\"activity\":\"a\",\"kind\":\"single_choice\"}]')",
    )
    .bind(ROUND)
    .bind(USER)
    .bind(COURSE)
    .execute(&pool)
    .await
    .unwrap();
    // completed：必带 finished/answers/report/earned（首写为终的形状面）。
    sqlx::query(
        "UPDATE learning_quiz_rounds SET status = 'completed', finished_at = 2, \
         answers_json = '{\"a\":\"B\"}', report_json = '[{\"node\":\"n\",\"earned_xp\":3}]', \
         earned_xp = 3 WHERE round_id = ?",
    )
    .bind(ROUND)
    .execute(&pool)
    .await
    .unwrap();
    let (status, earned): (String, i64) =
        sqlx::query_as("SELECT status, earned_xp FROM learning_quiz_rounds")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), earned), ("completed", 3));
}

#[tokio::test]
async fn rejects_incomplete_terminal_shapes() {
    let pool = migrated_pool().await;
    // completed 缺 answers/report/earned → CHECK 拒绝。
    let result = sqlx::query(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, finished_at, status, plan_json, earned_xp) \
         VALUES (?, ?, ?, 1, 2, 'completed', '[]', 5)",
    )
    .bind(ROUND)
    .bind(USER)
    .bind(COURSE)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "completed 缺作答/对账面必须被 CHECK 拦截");
    // abandoned 带 earned → CHECK 拒绝（放弃零结算）。
    let result = sqlx::query(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, finished_at, status, plan_json, earned_xp) \
         VALUES (?, ?, ?, 1, 2, 'abandoned', '[]', 5)",
    )
    .bind(ROUND)
    .bind(USER)
    .bind(COURSE)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "abandoned 必须零结算零作答面");
    // 非法 status → CHECK 拒绝。
    let result = sqlx::query(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, status, plan_json) \
         VALUES (?, ?, ?, 1, 'fetched', '[]')",
    )
    .bind(ROUND)
    .bind(USER)
    .bind(COURSE)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "status 三态白名单");
    // plan_json 非数组 → CHECK 拒绝。
    let result = sqlx::query(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, status, plan_json) \
         VALUES (?, ?, ?, 1, 'active', '{\"node\":1}')",
    )
    .bind(ROUND)
    .bind(USER)
    .bind(COURSE)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "plan_json 必为数组（选题快照）");
}

#[tokio::test]
async fn rejects_malformed_uuid_shapes() {
    let pool = migrated_pool().await;
    let cases = [
        (
            "0190f5fe-7c00-7a00-8abc-11111111111g",
            COURSE,
            "user_id 须全 hex",
        ),
        (
            ROUND,
            "0190f5fe-7c00-6a00-8abc-222222222222",
            "course_id 须 version 7",
        ),
    ];
    for (user, course, why) in cases {
        let result = sqlx::query(
            "INSERT INTO learning_quiz_rounds \
             (round_id, user_id, course_id, started_at, status, plan_json) \
             VALUES (?, ?, ?, 1, 'active', '[]')",
        )
        .bind(ROUND_2)
        .bind(user)
        .bind(course)
        .execute(&pool)
        .await;
        assert!(result.is_err(), "{why}");
    }
}

#[tokio::test]
async fn one_active_round_per_user_and_course() {
    let pool = migrated_pool().await;
    let insert = async |pool: &SqlitePool, round: &str| {
        sqlx::query(
            "INSERT INTO learning_quiz_rounds \
             (round_id, user_id, course_id, started_at, status, plan_json) \
             VALUES (?, ?, ?, 1, 'active', '[]')",
        )
        .bind(round)
        .bind(USER)
        .bind(COURSE)
        .execute(pool)
        .await
    };
    insert(&pool, ROUND).await.unwrap();
    let second = insert(&pool, ROUND_2).await;
    assert!(
        second.is_err(),
        "同用户同课程第二个 active 回合必须被部分唯一索引拦截"
    );
    // supersede 后可再开：旧回合标 abandoned，新回合落位。
    sqlx::query("UPDATE learning_quiz_rounds SET status = 'abandoned', finished_at = 2 WHERE round_id = ?")
        .bind(ROUND)
        .execute(&pool)
        .await
        .unwrap();
    insert(&pool, ROUND_2).await.unwrap();
    // 他用户不受限。
    sqlx::query(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, status, plan_json) \
         VALUES ('0190f5fe-7c00-7a00-8abc-353333333333', ?, ?, 1, 'active', '[]')",
    )
    .bind(USER_2)
    .bind(COURSE)
    .execute(&pool)
    .await
    .unwrap();
}

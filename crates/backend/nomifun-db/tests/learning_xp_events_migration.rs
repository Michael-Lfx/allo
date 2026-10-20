//! 迁移 068 学习域合并基线的 XP 段（结算账本 + 复习作答流水补列）的契约（三轮 t1/t3，j05）：
//! `learning_xp_events` 四形态写侧条款的数据库地板（full 非负 / partial 必带
//! ratio / trimmed 恰 0 / guess-penalty 必为负 / quiz 恒非负 / bonus 仅 full）
//! 与幂等锚 UNIQUE(user_id, anchor)；`learning_review_events` 补
//! elapsed_ms/response/session_id/forgot/predicted 五列（forgot 缺省 0）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const USER: &str = "0190f5fe-7c00-7a00-8abc-111111111111";
const COURSE: &str = "0190f5fe-7c00-7a00-8abc-222222222222";
const NODE: &str = "0190f5fe-7c00-7a00-8abc-333333333333";
const EVENT: &str = "0190f5fe-7c00-7a00-8abc-444444444444";
// 36 字符 UUIDv7 形状（CHECK：小写、version 7、variant [89ab]）。
const EVENT_2: &str = "0190f5fe-7c00-7a00-8abc-555555555555";
const EVENT_3: &str = "0190f5fe-7c00-7a00-8abc-666666666666";
const EVENT_4: &str = "0190f5fe-7c00-7a00-8abc-777777777777";
const EVENT_5: &str = "0190f5fe-7c00-7a00-8abc-888888888888";
const EVENT_6: &str = "0190f5fe-7c00-7a00-8abc-999999999999";
const EVENT_7: &str = "0190f5fe-7c00-7a00-8abc-aaaa99999999";
const EVENT_8: &str = "0190f5fe-7c00-7a00-8abc-bbbb99999999";

async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

/// 一条合法 full 结算事件（earned 含 bonus 分量、无 ratio）可入库。
#[tokio::test]
async fn accepts_well_formed_full_event() {
    let pool = migrated_pool().await;
    let result = sqlx::query(
        "INSERT INTO learning_xp_events \
         (event_id, user_id, ts, course_id, node_id, kind, earned_xp, xp_bonus, basis, ratio, origin, anchor) \
         VALUES (?, ?, 1, ?, ?, 'node-completed', 27, 2, 'full', NULL, 'lesson-progress', 'node:lesson-1')",
    )
    .bind(EVENT)
    .bind(USER)
    .bind(COURSE)
    .bind(NODE)
    .execute(&pool)
    .await;
    assert!(result.is_ok(), "legal full settlement rejected: {result:?}");
}

/// 条款地板逐条拒绝违约行：guess-penalty 必为负、非 guess-penalty 必非负、
/// trimmed 恰 0、bonus 仅 full、partial 必带 ratio、quiz 恒非负。
#[tokio::test]
async fn clause_floor_rejects_violations() {
    let pool = migrated_pool().await;
    async fn insert(
        pool: &SqlitePool,
        event_id: &str,
        kind: &str,
        earned: i64,
        bonus: i64,
        basis: &str,
        ratio: Option<f64>,
    ) -> sqlx::Result<sqlx::sqlite::SqliteQueryResult> {
        sqlx::query(
            "INSERT INTO learning_xp_events \
             (event_id, user_id, ts, kind, earned_xp, xp_bonus, basis, ratio, anchor) \
             VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?)",
        )
        .bind(event_id)
        .bind(USER)
        .bind(kind)
        .bind(earned)
        .bind(bonus)
        .bind(basis)
        .bind(ratio)
        .bind(event_id)
        .execute(pool)
        .await
    }
    assert!(
        insert(&pool, EVENT, "node-completed", 5, 0, "guess-penalty", None)
            .await
            .is_err(),
        "guess-penalty must be negative"
    );
    assert!(
        insert(&pool, EVENT_2, "node-completed", -1, 0, "partial", Some(0.5))
            .await
            .is_err(),
        "non-penalty forms settle non-negative"
    );
    assert!(
        insert(&pool, EVENT_3, "node-completed", 3, 0, "trimmed", None)
            .await
            .is_err(),
        "trimmed settles exactly 0"
    );
    // full 允许零奖励分量（零练习课时走 full、bonus 0）；bonus 出现在
    // 其余形态才是违约。
    let full_no_bonus = insert(&pool, EVENT_4, "node-completed", 27, 0, "full", None).await;
    assert!(full_no_bonus.is_ok(), "full without a bonus component stays legal: {full_no_bonus:?}");
    assert!(
        insert(&pool, EVENT_5, "node-completed", 1, 2, "partial", Some(0.5))
            .await
            .is_err(),
        "bonus is full-only"
    );
    assert!(
        insert(&pool, EVENT_6, "node-completed", 1, 0, "partial", None)
            .await
            .is_err(),
        "partial requires its ratio"
    );
    assert!(
        insert(&pool, EVENT_7, "quiz-completed", -1, 0, "partial", Some(0.5))
            .await
            .is_err(),
        "quiz settlements are never negative"
    );
    // ratio 越界同样被拒（条款 [0,1]）。
    assert!(
        insert(&pool, EVENT_8, "node-completed", 1, 0, "partial", Some(1.5))
            .await
            .is_err(),
        "ratio is clamped to [0,1]"
    );
}

/// 幂等锚：同用户同 anchor 的第二笔被 UNIQUE(user_id, anchor) 拒绝。
#[tokio::test]
async fn anchor_is_unique_per_user() {
    let pool = migrated_pool().await;
    async fn insert(pool: &SqlitePool, event_id: &str, user: &str, anchor: &str) -> sqlx::Result<sqlx::sqlite::SqliteQueryResult> {
        sqlx::query(
            "INSERT INTO learning_xp_events \
             (event_id, user_id, ts, kind, earned_xp, xp_bonus, basis, ratio, anchor) \
             VALUES (?, ?, 1, 'node-completed', 10, 0, 'partial', 0.5, ?)",
        )
        .bind(event_id)
        .bind(user)
        .bind(anchor)
        .execute(pool)
        .await
    }
    insert(&pool, EVENT, USER, "node:lesson-1").await.unwrap();
    assert!(
        insert(&pool, EVENT_2, USER, "node:lesson-1")
            .await
            .is_err(),
        "same anchor must not settle twice"
    );
    // 同锚不同用户互不干扰：账是按用户回放的。
    let other = "0190f5fe-7c00-7a00-8abc-777788888888";
    let result = insert(&pool, EVENT_3, other, "node:lesson-1").await;
    assert!(result.is_ok(), "a distinct user may hold the same anchor: {result:?}");
}

/// 作答流水五行补列到位：缺省 NULL/forgot=0，forgot 是 0/1 旗标。
#[tokio::test]
async fn review_events_answer_flow_columns() {
    let pool = migrated_pool().await;
    sqlx::query(
        "INSERT INTO learning_review_events (event_id, user_id, source, item_id, created_at) \
         VALUES (?, ?, 'course', 'item-1', 1)",
    )
    .bind(EVENT)
    .bind(USER)
    .execute(&pool)
    .await
    .unwrap();
    let (elapsed_ms, response, session_id, forgot, predicted): (
        Option<i64>,
        Option<String>,
        Option<String>,
        i64,
        Option<f64>,
    ) = sqlx::query_as(
        "SELECT elapsed_ms, response, session_id, forgot, predicted \
         FROM learning_review_events WHERE event_id = ?",
    )
    .bind(EVENT)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (elapsed_ms, response.as_deref(), session_id.as_deref(), forgot, predicted),
        (None, None, None, 0, None)
    );
    let result = sqlx::query("UPDATE learning_review_events SET forgot = 2 WHERE event_id = ?")
        .bind(EVENT)
        .execute(&pool)
        .await;
    assert!(result.is_err(), "forgot is a 0/1 flag");
}

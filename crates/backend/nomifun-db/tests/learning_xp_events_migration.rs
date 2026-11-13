//! 迁移 081（XP 结算账本 + 复习作答流水补列）的契约（三轮 t1/t3，j05）：
//! 跨列结算条款已移出 DB（ADR-0015，写侧 `validate_settlement` 执法），DB
//! 只保留数值域 CHECK（xp_bonus >= 0 / ratio ∈ [0,1]）与幂等锚
//! UNIQUE(user_id, anchor)；`learning_review_events` 补
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

/// 跨列结算条款（full 非负 / partial 必带 ratio / trimmed 恰 0 / guess-penalty
/// 必为负 / quiz 恒非负 / bonus 仅 full）已移出 DB（ADR-0015），由
/// `settle.rs::validate_settlement` 写侧执法并有逐条单测；DB 只保留两条数值域
/// 硬 CHECK：`xp_bonus >= 0` 与 `ratio ∈ [0,1]`。
#[tokio::test]
async fn numeric_domain_checks_survive_while_clause_floor_moves_to_rust() {
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
    // 保留的数值域 CHECK：负的 bonus 分量被拒。
    assert!(
        insert(&pool, EVENT, "node-completed", 5, -1, "full", None)
            .await
            .is_err(),
        "negative xp_bonus must still be rejected"
    );
    // 保留的数值域 CHECK：ratio 越界被拒（条款 [0,1]）。
    assert!(
        insert(&pool, EVENT_2, "node-completed", 1, 0, "partial", Some(1.5))
            .await
            .is_err(),
        "ratio is clamped to [0,1]"
    );
    // 跨列条款已移出 DB：guess-penalty 携正分现在能落库，拒绝责任在
    // settle.rs::validate_settlement（其单测逐条覆盖四形态）。
    assert!(
        insert(&pool, EVENT_3, "node-completed", 5, 0, "guess-penalty", None)
            .await
            .is_ok(),
        "DB no longer enforces the cross-column settlement clause"
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

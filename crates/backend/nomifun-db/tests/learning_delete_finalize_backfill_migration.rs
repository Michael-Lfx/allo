//! t3 R2（ADR-0017 §4）：删除收尾存量背填——墓碑课（`deleted_at IS NOT
//! NULL`）上的 active 幽灵回合废弃（supersede 同款形态），activity 行已删
//! 的死卡打 `archived_at`。断言面：收尾只碰存量残影、在学课（活课程）的
//! active 回合与在学卡原样；回望账本面（completed 回合历史）原样保留；
//! 背填幂等（重放零副作用）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const USER: &str = "0190f5fe-7c00-7a00-8abc-000000000001";
// 墓碑课（存量残影宿主）：课程行 deleted_at 已置位。
const TOMB: &str = "0190f5fe-7c00-7a00-8abc-000000000002";
// 在学课（负对照）：删除收尾不得越界。
const KEPT: &str = "0190f5fe-7c00-7a00-8abc-000000000003";
const MOD_TOMB: &str = "0190f5fe-7c00-7a00-8abc-000000000004";
const MOD_KEPT: &str = "0190f5fe-7c00-7a00-8abc-000000000005";
const LESSON_TOMB: &str = "0190f5fe-7c00-7a00-8abc-000000000006";
const LESSON_KEPT: &str = "0190f5fe-7c00-7a00-8abc-000000000007";
const ACT_TOMB: &str = "0190f5fe-7c00-7a00-8abc-000000000008";
const ACT_KEPT: &str = "0190f5fe-7c00-7a00-8abc-000000000009";
const ENROLL_TOMB: &str = "0190f5fe-7c00-7a00-8abc-00000000000a";
const ENROLL_KEPT: &str = "0190f5fe-7c00-7a00-8abc-00000000000b";
const CARD_DEAD: &str = "0190f5fe-7c00-7a00-8abc-00000000000c";
const CARD_TOMB_SERVABLE: &str = "0190f5fe-7c00-7a00-8abc-00000000000d";
const CARD_KEPT: &str = "0190f5fe-7c00-7a00-8abc-00000000000e";
const ROUND_TOMB_ACTIVE: &str = "0190f5fe-7c00-7a00-8abc-00000000000f";
const ROUND_TOMB_COMPLETED: &str = "0190f5fe-7c00-7a00-8abc-000000000010";
const ROUND_KEPT_ACTIVE: &str = "0190f5fe-7c00-7a00-8abc-000000000011";
// 死卡指到的 activity 行已不存在（uuid 形状合规即可，无物理外键）。
const ACT_MISSING: &str = "0190f5fe-7c00-7a00-8abc-000000000012";

async fn pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap()
}

async fn migrate_range(pool: &SqlitePool, from: i64, to: i64) {
    for migration in MIGRATOR.iter() {
        if migration.version <= from || migration.version > to {
            continue;
        }
        sqlx::raw_sql(&migration.sql)
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("migration {} failed: {error}", migration.version));
    }
}

async fn run(pool: &SqlitePool, sql: &str) {
    sqlx::query(sql).execute(pool).await.unwrap();
}

/// 071 落地前的存量形状（v70）：一门墓碑课（正测一半——active 幽灵回合 +
/// completed 账本历史 + 死卡/可服务卡各一张）、一门在学课（负对照——
/// active 回合 + 在学卡）。enabled 缺省 1：旧墓碑不带 enabled = 0，正是
/// 运行时 t3 R1③ 之前的存量形态。
async fn seed_v70(pool: &SqlitePool) {
    run(pool, &format!(
        "INSERT INTO users (user_id, username, email, password_hash, created_at, updated_at) \
         VALUES ('{USER}', 'seed', 'seed@example.com', 'x', 1000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at, deleted_at) \
         VALUES ('{TOMB}', '删了半截的课', '', 'general', 1, 1000, 1000, 1500), \
                ('{KEPT}', '在学的课', '', 'general', 1, 1000, 1000, NULL)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_modules (module_id, course_id, title, position) \
         VALUES ('{MOD_TOMB}', '{TOMB}', 'm', 0), ('{MOD_KEPT}', '{KEPT}', 'm', 0)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, position, estimated_minutes) \
         VALUES ('{LESSON_TOMB}', '{MOD_TOMB}', 'l', '', 0, 10), \
                ('{LESSON_KEPT}', '{MOD_KEPT}', 'l', '', 0, 10)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_activities \
         (activity_id, lesson_id, kind, prompt, config_json, position) \
         VALUES ('{ACT_TOMB}', '{LESSON_TOMB}', 'single_choice', 'q', '{{}}', 0), \
                ('{ACT_KEPT}', '{LESSON_KEPT}', 'single_choice', 'q', '{{}}', 0)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_enrollments \
         (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES ('{ENROLL_TOMB}', '{USER}', '{TOMB}', 1000, 1000), \
                ('{ENROLL_KEPT}', '{USER}', '{KEPT}', 1000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, activity_id, due_at, updated_at) \
         VALUES ('{CARD_DEAD}', '{ENROLL_TOMB}', '{ACT_MISSING}', 2000, 1000), \
                ('{CARD_TOMB_SERVABLE}', '{ENROLL_TOMB}', '{ACT_TOMB}', 2000, 1000), \
                ('{CARD_KEPT}', '{ENROLL_KEPT}', '{ACT_KEPT}', 2000, 1000)"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, status, doc_version, plan_json) \
         VALUES ('{ROUND_TOMB_ACTIVE}', '{USER}', '{TOMB}', 1600, 'active', 1, '[]'), \
                ('{ROUND_KEPT_ACTIVE}', '{USER}', '{KEPT}', 1600, 'active', 1, '[]')"
    )).await;
    run(pool, &format!(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, finished_at, status, doc_version, \
          plan_json, answers_json, report_json, earned_xp) \
         VALUES ('{ROUND_TOMB_COMPLETED}', '{USER}', '{TOMB}', 1600, 1700, 'completed', \
                 1, '[]', '{{}}', '[]', 3)"
    )).await;
}

async fn one(pool: &SqlitePool, sql: &str) -> (String, Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    sqlx::query_as(sql).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn tombstone_ghost_rounds_are_abandoned_and_dead_cards_archived() {
    let pool = pool().await;
    migrate_range(&pool, 0, 70).await;
    seed_v70(&pool).await;

    migrate_range(&pool, 70, i64::MAX).await;

    // ① 墓碑课的 active 幽灵回合 → abandoned（supersede 形态：finished_at
    //    在，answers/report/earned 保持空）。
    let (status, finished_at, answers, report, earned) = one(
        &pool,
        &format!(
            "SELECT status, finished_at, answers_json, report_json, earned_xp \
             FROM learning_quiz_rounds WHERE round_id = '{ROUND_TOMB_ACTIVE}'"
        ),
    )
    .await;
    assert_eq!(status, "abandoned", "ghost active round on a tombstone must be abandoned");
    assert!(
        finished_at.is_some() && answers.is_none() && report.is_none() && earned.is_none(),
        "abandoned shape: finished_at set, payloads empty, got {finished_at:?} {answers:?} \
         {report:?} {earned:?}"
    );
    // 回望账本面：completed 回合历史原样（ADR-0017 §2，零改写）。
    let (status, finished_at, answers, report, earned): (
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = sqlx::query_as(&format!(
        "SELECT status, finished_at, answers_json, report_json, earned_xp \
         FROM learning_quiz_rounds WHERE round_id = '{ROUND_TOMB_COMPLETED}'"
    ))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (status.as_str(), finished_at, answers.as_deref(), report.as_deref(), earned),
        ("completed", Some(1700), Some("{}"), Some("[]"), Some(3)),
        "settled round history is ledger face, untouched"
    );
    // 负对照：在学课的 active 回合不受影响。
    let (status, finished_at, _answers, _report, _earned) = one(
        &pool,
        &format!(
            "SELECT status, finished_at, answers_json, report_json, earned_xp \
             FROM learning_quiz_rounds WHERE round_id = '{ROUND_KEPT_ACTIVE}'"
        ),
    )
    .await;
    assert_eq!(
        (status.as_str(), finished_at),
        ("active", None),
        "the kept course's active round must stay active"
    );

    // ② 死卡（activity 行已删）→ archived_at 置位。
    let (archived_at, _updated): (Option<i64>, Option<i64>) = sqlx::query_as(&format!(
        "SELECT archived_at, updated_at FROM learning_review_items \
         WHERE review_item_id = '{CARD_DEAD}'"
    ))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(archived_at.is_some(), "a card whose activity row is gone must carry archived_at");
    // 负对照：activity 行仍在的卡（墓碑课遗留可服务卡、在学卡）不打标。
    for (card, label) in [
        (CARD_TOMB_SERVABLE, "a card whose activity row survives must stay unarchived"),
        (CARD_KEPT, "a live course's card must stay unarchived"),
    ] {
        let (archived_at, _updated): (Option<i64>, Option<i64>) = sqlx::query_as(&format!(
            "SELECT archived_at, updated_at FROM learning_review_items \
             WHERE review_item_id = '{card}'"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(archived_at.is_none(), "{label}");
    }

    // 账本面零接触：注册与进度无人被删（迁移只有两条 UPDATE，无 DELETE）。
    let enrollments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_enrollments").fetch_one(&pool).await.unwrap();
    assert_eq!(enrollments, 2, "backfill must not touch the enrollment ledger");
}

/// 背填幂等（t3 R4）：重放 071 全文零副作用——abandoned 非 active 不再
/// 命中、archived_at IS NULL 过滤不重打标；首背填的时间戳原样保留。
#[tokio::test]
async fn backfill_is_idempotent_on_replay() {
    let pool = pool().await;
    migrate_range(&pool, 0, 70).await;
    seed_v70(&pool).await;
    migrate_range(&pool, 70, i64::MAX).await;

    let snapshot = |pool: &SqlitePool| {
        let pool = pool.clone();
        async move {
            let round: (String, Option<i64>) = sqlx::query_as(&format!(
                "SELECT status, finished_at FROM learning_quiz_rounds \
                 WHERE round_id = '{ROUND_TOMB_ACTIVE}'"
            ))
            .fetch_one(&pool)
            .await
            .unwrap();
            let dead: (Option<i64>, Option<i64>) = sqlx::query_as(&format!(
                "SELECT archived_at, updated_at FROM learning_review_items \
                 WHERE review_item_id = '{CARD_DEAD}'"
            ))
            .fetch_one(&pool)
            .await
            .unwrap();
            (round, dead)
        }
    };
    let before = snapshot(&pool).await;

    // 重放 071 全文（迁移器本身只跑一次，幂等由语句形态保证）。
    let sql_071 = MIGRATOR
        .iter()
        .find(|migration| migration.version == 71)
        .expect("migration 071 must exist")
        .sql
        .to_string();
    sqlx::raw_sql(&sql_071).execute(&pool).await.unwrap();

    let after = snapshot(&pool).await;
    assert_eq!(before, after, "replaying 071 must be a no-op");
}

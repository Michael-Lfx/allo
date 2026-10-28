//! 迁移 071 学习维护水位表（`learning_maintenance_state`）的契约：一行一课、
//! 水位默认 0（行缺失 = 全量重读一次）、非负、PK 唯一。水位是派生记账面，
//! 课程删除随删（逻辑引用 Cascade，见 id_schema_contract）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";

async fn setup() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn seed_course(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '水位课', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn maintenance_state_row_defaults_to_zero_watermark() {
    let pool = setup().await;
    seed_course(&pool).await;
    sqlx::query(
        "INSERT INTO learning_maintenance_state (course_id, updated_at) VALUES (?, 2000)",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT findings_watermark, updated_at FROM learning_maintenance_state WHERE course_id = ?",
    )
    .bind(COURSE_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("findings_watermark"), 0);
    assert_eq!(row.get::<i64, _>("updated_at"), 2000);
}

#[tokio::test]
async fn maintenance_state_rejects_duplicate_course_and_negative_watermark() {
    let pool = setup().await;
    seed_course(&pool).await;
    let insert = |watermark: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO learning_maintenance_state \
                 (course_id, findings_watermark, updated_at) VALUES (?, ?, 2000)",
            )
            .bind(COURSE_ID)
            .bind(watermark)
            .execute(&pool)
            .await
        }
    };
    insert(5).await.unwrap();
    assert!(insert(6).await.is_err(), "一课一行（PK 唯一）");

    let other_course = "0190f5fe-7c00-7a00-8abc-012345678902";
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '课二', '', 'general', 1, 1000, 1000)",
    )
    .bind(other_course)
    .execute(&pool)
    .await
    .unwrap();
    let negative = sqlx::query(
        "INSERT INTO learning_maintenance_state \
         (course_id, findings_watermark, updated_at) VALUES (?, -1, 2000)",
    )
    .bind(other_course)
    .execute(&pool)
    .await;
    assert!(negative.is_err(), "水位非负");
}

/// v3 契约注册：新表进 PRODUCT_TABLES + 逻辑引用（Cascade 派生面），
/// 整库契约校验全绿（表 → 宿主列 → 具名索引三面对账）。
#[tokio::test]
async fn maintenance_state_is_registered_in_the_id_schema_contract() {
    let pool = setup().await;
    nomifun_db::validate_id_schema_contract(&pool)
        .await
        .expect("id schema contract (含 learning_maintenance_state)");
}

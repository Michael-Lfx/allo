//! 迁移 076（`learning_courses.enabled`）的契约：课程级「现存且启用」口径
//! （三轮 g3）——列默认 1（存量课程全部回填为启用，缺席列 = 启用的兼容
//! 语义），归档/恢复只翻转该列（数据零删零改）。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";

/// 只跑 `(from, to]` 区间内的迁移——回填类迁移必须在有老数据的库上验证。
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

async fn setup() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn insert_course(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '存量课程', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
}

/// 存量行迁移后默认启用（DEFAULT 1 回填）：归档是显式动作，不是迁移副作用。
#[tokio::test]
async fn existing_rows_default_to_enabled() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrate_range(&pool, 0, 75).await;
    insert_course(&pool).await;
    migrate_range(&pool, 75, 76).await;
    let enabled: i64 = sqlx::query_scalar("SELECT enabled FROM learning_courses WHERE course_id = ?")
        .bind(COURSE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1);
}

/// 新行缺省启用；归档/恢复是 enabled 的一对翻转，其他列不动。
#[tokio::test]
async fn archive_and_restore_flip_enabled_only() {
    let pool = setup().await;
    insert_course(&pool).await;
    let enabled: i64 = sqlx::query_scalar("SELECT enabled FROM learning_courses WHERE course_id = ?")
        .bind(COURSE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1, "新行缺省启用");

    sqlx::query("UPDATE learning_courses SET enabled = 0 WHERE course_id = ?")
        .bind(COURSE_ID)
        .execute(&pool)
        .await
        .unwrap();
    let row = sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT enabled, deleted_at FROM learning_courses WHERE course_id = ?",
    )
    .bind(COURSE_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row, (0, None), "归档 = enabled 翻转，不碰软删墓碑位");

    sqlx::query("UPDATE learning_courses SET enabled = 1 WHERE course_id = ?")
        .bind(COURSE_ID)
        .execute(&pool)
        .await
        .unwrap();
    let enabled: i64 = sqlx::query_scalar("SELECT enabled FROM learning_courses WHERE course_id = ?")
        .bind(COURSE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1, "恢复 = 翻回 1");
}

/// 活跃课程集单源（读侧口径钉住）：deleted_at IS NULL AND enabled = 1。
#[tokio::test]
async fn active_set_is_existing_and_enabled() {
    let pool = setup().await;
    insert_course(&pool).await;
    sqlx::query("INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at, enabled) \
                 VALUES (?, '归档课', '', 'general', 1, 1000, 1000, 0)")
        .bind("0190f5fe-7c00-7a00-8abc-012345678902")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at, deleted_at) \
                 VALUES (?, '墓碑课', '', 'general', 1, 1000, 1000, 2000)")
        .bind("0190f5fe-7c00-7a00-8abc-012345678903")
        .execute(&pool)
        .await
        .unwrap();
    let titles: Vec<String> = sqlx::query(
        "SELECT title FROM learning_courses WHERE deleted_at IS NULL AND enabled = 1 ORDER BY title",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|row: &sqlx::sqlite::SqliteRow| row.get::<String, _>("title"))
    .collect();
    assert_eq!(titles, vec!["存量课程"], "归档课与墓碑课都不在活跃集");
}

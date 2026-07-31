use nomifun_db::sqlx::{self, SqlitePool};

#[tokio::test]
async fn preset_skill_binding_migration_renames_legacy_name_without_losing_value() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE preset_skill_bindings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            preset_id TEXT NOT NULL,
            skill_name TEXT NOT NULL,
            binding TEXT NOT NULL,
            required INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0,
            UNIQUE (preset_id, skill_name, binding)
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO preset_skill_bindings (preset_id, skill_name, binding, required, sort_order)
         VALUES ('preset-1', 'writer', 'include', 1, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(include_str!("../migrations/028_preset_skill_binding_ids.sql"))
        .execute(&pool)
        .await
        .unwrap();

    let skill_id: String = sqlx::query_scalar(
        "SELECT skill_id FROM preset_skill_bindings WHERE preset_id = 'preset-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(skill_id, "writer");
}

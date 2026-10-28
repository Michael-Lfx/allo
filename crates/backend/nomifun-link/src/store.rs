use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

const SCHEMA: &str = include_str!("../../nomifun-db/migrations/068_linked_devices.sql");

#[derive(Debug, Clone)]
pub struct DeviceRow {
    pub linked_device_id: String,
    pub display_name: String,
    pub platform: String,
    pub app_version: String,
    pub wake_enabled: bool,
    pub revoked: bool,
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(SCHEMA).execute(pool).await?;
    Ok(())
}

pub fn hash_refresh(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub async fn insert_device(
    pool: &SqlitePool,
    user_id: &str,
    display_name: &str,
    platform: &str,
    public_key_fingerprint: &str,
    refresh_token_hash: &str,
    app_version: &str,
    now_ms: i64,
) -> Result<String, sqlx::Error> {
    let id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO linked_devices (
            linked_device_id, user_id, display_name, platform, public_key_fingerprint,
            refresh_token_hash, wake_enabled, app_version, created_at_ms, last_seen_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(display_name)
    .bind(platform)
    .bind(public_key_fingerprint)
    .bind(refresh_token_hash)
    .bind(app_version)
    .bind(now_ms)
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn find_by_refresh(
    pool: &SqlitePool,
    refresh_hash: &str,
) -> Result<Option<DeviceRow>, sqlx::Error> {
    let row = sqlx::query_as::<_, (String, String, String, String, i64, Option<i64>)>(
        "SELECT linked_device_id, display_name, platform, app_version, wake_enabled, revoked_at_ms
         FROM linked_devices WHERE refresh_token_hash = ?",
    )
    .bind(refresh_hash)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_row))
}

pub async fn find_by_id(pool: &SqlitePool, device_id: &str) -> Result<Option<DeviceRow>, sqlx::Error> {
    let row = sqlx::query_as::<_, (String, String, String, String, i64, Option<i64>)>(
        "SELECT linked_device_id, display_name, platform, app_version, wake_enabled, revoked_at_ms
         FROM linked_devices WHERE linked_device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_row))
}

pub async fn touch(pool: &SqlitePool, device_id: &str, now_ms: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE linked_devices SET last_seen_at_ms = ? WHERE linked_device_id = ? AND revoked_at_ms IS NULL")
        .bind(now_ms)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_wake(pool: &SqlitePool, device_id: &str, enabled: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE linked_devices SET wake_enabled = ? WHERE linked_device_id = ? AND revoked_at_ms IS NULL")
        .bind(i64::from(enabled))
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn revoke_ids(pool: &SqlitePool, ids: &[String], now_ms: i64) -> Result<(), sqlx::Error> {
    for id in ids {
        sqlx::query(
            "UPDATE linked_devices SET revoked_at_ms = ? WHERE linked_device_id = ? AND revoked_at_ms IS NULL",
        )
        .bind(now_ms)
        .bind(id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn list_wake_device_ids(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT linked_device_id FROM linked_devices WHERE wake_enabled = 1 AND revoked_at_ms IS NULL",
    )
    .fetch_all(pool)
    .await
}

pub async fn list_revoked_device_ids(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT linked_device_id FROM linked_devices WHERE revoked_at_ms IS NOT NULL")
        .fetch_all(pool)
        .await
}

fn map_row(row: (String, String, String, String, i64, Option<i64>)) -> DeviceRow {
    DeviceRow {
        linked_device_id: row.0,
        display_name: row.1,
        platform: row.2,
        app_version: row.3,
        wake_enabled: row.4 == 1,
        revoked: row.5.is_some(),
    }
}

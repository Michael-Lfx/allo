//! Percentage rollout for Tauri updater manifests.
//!
//! `latest.json` may include an optional `rollout` field (0–100). The stock
//! tauri-plugin-updater ignores unknown fields, so the desktop shell fetches
//! the same endpoint and decides whether this install is in the cohort.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

const INSTALL_ID_FILE: &str = "update-rollout-id";

/// Parse `rollout` from a Tauri updater manifest. Accepts a bare integer
/// (`"rollout": 10`) or `{ "percent": 10 }`. Missing / invalid → full rollout.
pub fn parse_rollout_percent(manifest: &Value) -> u8 {
    match manifest.get("rollout") {
        Some(Value::Number(n)) => n
            .as_u64()
            .map(|v| v.min(100) as u8)
            .unwrap_or(100),
        Some(Value::Object(obj)) => obj
            .get("percent")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(100) as u8)
            .unwrap_or(100),
        _ => 100,
    }
}

/// Sticky bucket in `0..100` for `(install_id, version)`.
pub fn rollout_bucket(install_id: &str, version: &str) -> u8 {
    let mut hasher = DefaultHasher::new();
    install_id.hash(&mut hasher);
    b':'.hash(&mut hasher);
    version.hash(&mut hasher);
    (hasher.finish() % 100) as u8
}

pub fn device_in_rollout(install_id: &str, version: &str, percent: u8) -> bool {
    if percent >= 100 {
        return true;
    }
    if percent == 0 {
        return false;
    }
    rollout_bucket(install_id, version) < percent
}

pub fn install_id_path(data_dir: &Path) -> PathBuf {
    data_dir.join(INSTALL_ID_FILE)
}

pub fn load_or_create_install_id(data_dir: &Path) -> String {
    let path = install_id_path(data_dir);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    let id = format!(
        "{:016x}-{:016x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
        hasher.finish()
    );
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(&path, &id);
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_rollout_accepts_number_and_object() {
        assert_eq!(parse_rollout_percent(&json!({})), 100);
        assert_eq!(parse_rollout_percent(&json!({ "rollout": 10 })), 10);
        assert_eq!(
            parse_rollout_percent(&json!({ "rollout": { "percent": 25 } })),
            25
        );
        assert_eq!(parse_rollout_percent(&json!({ "rollout": 250 })), 100);
    }

    #[test]
    fn sticky_bucket_expands_with_percent() {
        let id = "install-a";
        let version = "1.5.9";
        let bucket = rollout_bucket(id, version);
        assert!(device_in_rollout(id, version, 100));
        assert!(!device_in_rollout(id, version, 0));
        assert_eq!(device_in_rollout(id, version, bucket), false);
        assert!(device_in_rollout(id, version, bucket.saturating_add(1)));
    }

    #[test]
    fn install_id_persists() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create_install_id(dir.path());
        let second = load_or_create_install_id(dir.path());
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }
}

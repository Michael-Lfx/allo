//! Local persisted paths for server client state.

use std::path::{Path, PathBuf};

pub fn server_state_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("server")
}

pub fn profile_cache_path(data_dir: &Path) -> PathBuf {
    server_state_dir(data_dir).join("profile.json")
}

pub fn device_state_path(data_dir: &Path) -> PathBuf {
    server_state_dir(data_dir).join("device_state.json")
}

pub fn client_id_path(data_dir: &Path) -> PathBuf {
    server_state_dir(data_dir).join("client_id")
}

/// Stable anonymous install id for presence / package / product analytics.
pub fn load_or_create_client_id(data_dir: &Path) -> String {
    let path = client_id_path(data_dir);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, format!("{id}\n"));
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_is_stable_for_a_data_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = load_or_create_client_id(dir.path());
        let second = load_or_create_client_id(dir.path());
        assert_eq!(first, second);
        assert!(first.len() >= 32);
    }
}

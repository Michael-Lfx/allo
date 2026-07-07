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

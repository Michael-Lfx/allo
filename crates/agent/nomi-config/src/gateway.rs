//! Agent data directory paths and top-level gateway config.

use std::path::{Path, PathBuf};

use nomifun_common::storage_paths;
use serde::{Deserialize, Serialize};

use crate::insights::InsightsConfig;
use crate::interest::InterestConfig;
use crate::media::MediaGenConfig;
use crate::server::ServerConfig;

/// Top-level agent/gateway configuration persisted in `config.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GatewayConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<String>,

    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub media: MediaGenConfig,

    #[serde(default)]
    pub insights: InsightsConfig,

    #[serde(default)]
    pub interest: InterestConfig,
}

/// Resolve the agent data directory (backend `data_dir` / `Flowy/Nomi` unless overridden).
pub fn data_dir() -> PathBuf {
    if let Some(home) = storage_paths::resolve_home_from_env() {
        return home;
    }
    default_data_dir()
}

pub fn default_data_dir() -> PathBuf {
    storage_paths::default_data_dir("")
}

pub fn config_yaml_path(config_dir: Option<&Path>) -> PathBuf {
    config_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(data_dir)
        .join("config.yaml")
}

pub fn load_config(config_dir: Option<&Path>) -> Result<GatewayConfig, String> {
    let path = config_yaml_path(config_dir);
    if !path.exists() {
        return Ok(GatewayConfig::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_yaml::from_str(&raw).map_err(|e| e.to_string())
}

pub fn load_user_config_file(path: &Path) -> Result<GatewayConfig, String> {
    if !path.exists() {
        return Ok(GatewayConfig::default());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_yaml::from_str(&raw).map_err(|e| e.to_string())
}

pub fn save_config_yaml(path: &Path, config: &GatewayConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let yaml = serde_yaml::to_string(config).map_err(|e| e.to_string())?;
    std::fs::write(path, yaml).map_err(|e| e.to_string())
}

pub fn env_var_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Like [`env_var_enabled`], but defaults to `true` when the variable is unset.
pub fn env_var_enabled_default_true(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_data_dir_uses_flowy_nomi_not_dot_nomifun() {
        let dir = default_data_dir();
        let s = dir.to_string_lossy();
        assert!(!s.contains(".nomifun"), "agent home must not default to ~/.nomifun, got {s}");
        assert!(
            dir.ends_with("Flowy/Nomi") || dir.ends_with("nomifun-data/Nomi"),
            "expected Flowy/Nomi default, got {dir:?}"
        );
    }
}

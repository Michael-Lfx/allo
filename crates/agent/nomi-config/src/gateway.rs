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
    let raw = read_config_yaml(&path)?;
    serde_yaml::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_user_config_file(path: &Path) -> Result<GatewayConfig, String> {
    if !path.exists() {
        return Ok(GatewayConfig::default());
    }
    let raw = read_config_yaml(path)?;
    serde_yaml::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_config_yaml(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Load the gateway config for a **boot-path** caller, failing soft.
///
/// A `config.yaml` that exists but cannot be read or parsed used to abort
/// startup: [`PoiService::new`], `InsightsService::new`, `MediaApiService::new`
/// and `CloudHttpService::new_with_host` all propagate the YAML error up to the
/// host's `main`, so one typo while hand-editing the file left the whole product
/// unable to start — with a message that did not even name the file.
///
/// The unreadable file is **moved aside**, not defaulted in place. Defaulting
/// alone would keep startup alive only until the next settings write, which
/// would overwrite whatever the user had (defaults are indistinguishable from
/// "never configured"), and the original bytes are exactly what they need to fix
/// the typo. This always returns a config.
pub fn load_config_for_boot(config_dir: Option<&Path>) -> GatewayConfig {
    let path = config_yaml_path(config_dir);
    match load_config(config_dir) {
        Ok(config) => config,
        Err(error) => default_after_quarantine(&path, &error),
    }
}

/// [`load_user_config_file`] for a **boot-path** caller that already knows the
/// exact path. See [`load_config_for_boot`] for why this cannot fail.
pub fn load_user_config_file_for_boot(path: &Path) -> GatewayConfig {
    match load_user_config_file(path) {
        Ok(config) => config,
        Err(error) => default_after_quarantine(path, &error),
    }
}

fn default_after_quarantine(path: &Path, error: &str) -> GatewayConfig {
    let backup = quarantine_unreadable_config(path);
    match backup.as_ref() {
        Some(backup) => tracing::error!(
            target: "nomi_config",
            path = %path.display(),
            backup = %backup.display(),
            error = %error,
            "config.yaml is unreadable; continuing with defaults and moved the bad file aside",
        ),
        None => tracing::error!(
            target: "nomi_config",
            path = %path.display(),
            error = %error,
            "config.yaml is unreadable and could not be moved aside; continuing with defaults, \
             but a later settings write will overwrite the file",
        ),
    }
    GatewayConfig::default()
}

/// Move an unreadable config next to itself (`config.yaml.invalid-<unix-secs>`)
/// so a later write cannot destroy the user's bytes. Best effort: a failed move
/// is reported by the caller and startup still continues.
fn quarantine_unreadable_config(path: &Path) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let backup = path.with_extension(format!("yaml.invalid-{stamp}"));
    match std::fs::rename(path, &backup) {
        Ok(()) => Some(backup),
        Err(error) => {
            tracing::warn!(
                target: "nomi_config",
                path = %path.display(),
                backup = %backup.display(),
                error = %error,
                "failed to move the unreadable config aside",
            );
            None
        }
    }
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

    /// A fresh machine has no `config.yaml`; that must be defaults, silently.
    #[test]
    fn missing_config_is_defaults_and_creates_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");

        let loaded = load_config_for_boot(Some(dir.path()));

        assert_eq!(loaded.server.base_url, GatewayConfig::default().server.base_url);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(leftovers.is_empty(), "nothing should be written, got {leftovers:?}");
        assert!(!path.exists());
    }

    /// A typo while hand-editing must not brick startup — and must not silently
    /// throw the user's bytes away either.
    #[test]
    fn corrupt_config_fails_soft_and_preserves_the_bad_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let broken = "server:\n  port: [unclosed\nmodel: *undefined_alias\n";
        std::fs::write(&path, broken).unwrap();

        let loaded = load_config_for_boot(Some(dir.path()));

        assert_eq!(loaded.server.base_url, GatewayConfig::default().server.base_url);
        assert!(!path.exists(), "the unreadable file must be moved aside, not left in place");
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| candidate != &path)
            .collect();
        assert_eq!(backups.len(), 1, "exactly one quarantine file, got {backups:?}");
        assert!(
            backups[0].to_string_lossy().contains("config.yaml.invalid-"),
            "quarantine name should keep the original name, got {:?}",
            backups[0]
        );
        assert_eq!(
            std::fs::read_to_string(&backups[0]).unwrap(),
            broken,
            "the user's bytes must survive verbatim",
        );
    }

    /// The strict loader stays strict — callers that can report a real error keep
    /// getting one — but the message has to name the file they must fix.
    #[test]
    fn strict_loader_reports_the_path_in_its_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "server: [unclosed\n").unwrap();

        let error = load_user_config_file(&path).unwrap_err();

        assert!(
            error.contains(&path.display().to_string()),
            "the error must name the file, got: {error}"
        );
    }
}

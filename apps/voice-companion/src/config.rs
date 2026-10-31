//! AI Mate Voice Companion Configuration (Wave 0 C7 Schema)
//!
//! Provides the `[models]` section schema and configuration loading logic
//! according to the AI Mate architecture baseline and model-management specification:
//!
//! ```toml
//! [models]
//! models_dir = "/var/lib/ai-mate/models"
//! auto_download = true           # Auto-trigger download on initial boot when models are missing
//! probe_timeout_ms = 1500        # Multi-source probe timeout in milliseconds
//! prefer_mirror = "auto"         # "auto" | "modelscope" | "github"
//! ```
//!
//! Also supports runtime override via the `AI_MATE_MODEL_DIR` environment variable.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Primary environment variable used to override the models directory path.
pub const ENV_VOICE_COMPANION_MODEL_DIR: &str = "VOICE_COMPANION_MODEL_DIR";

/// Backwards-compatible legacy environment variable used to override the models directory path.
pub const ENV_AI_MATE_MODEL_DIR: &str = "AI_MATE_MODEL_DIR";

/// Default system models directory for Linux companion host / system service.
pub const DEFAULT_MODELS_DIR: &str = "/var/lib/voice-companion/models";

/// Default auto-download setting for headless companion host.
pub const DEFAULT_AUTO_DOWNLOAD: bool = true;

/// Default probe timeout in milliseconds for dual-source probing.
pub const DEFAULT_PROBE_TIMEOUT_MS: u64 = 1500;

/// Default path for the Voice Companion configuration file.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/voice-companion/config.toml";

/// Preferred model download mirror source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PreferMirror {
    /// Auto probing: test ModelScope first, fallback to GitHub / HuggingFace.
    #[default]
    #[serde(rename = "auto")]
    Auto,

    /// Primary domestic source: ModelScope official CDN mirror.
    #[serde(rename = "modelscope")]
    ModelScope,

    /// Primary global source: GitHub Releases / HuggingFace.
    #[serde(rename = "github")]
    GitHub,
}

impl PreferMirror {
    /// Return the string identifier of the mirror option.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ModelScope => "modelscope",
            Self::GitHub => "github",
        }
    }
}

impl fmt::Display for PreferMirror {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for PreferMirror {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "modelscope" => Ok(Self::ModelScope),
            "github" => Ok(Self::GitHub),
            other => Err(ConfigError::InvalidPreferMirror(other.to_string())),
        }
    }
}

/// Default function for serde `models_dir` field (platform-specific).
pub fn default_models_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let app_models = PathBuf::from(&local_app_data)
                .join("Flowy")
                .join("models");
            if app_models.join("asr").exists() {
                return app_models;
            }
            let primary = PathBuf::from(&local_app_data)
                .join("Flowy")
                .join("Voice-Companion")
                .join("models");
            if primary.join("asr").exists() {
                return primary;
            }
            if app_models.exists() {
                return app_models;
            }
            return primary;
        }
        let primary = PathBuf::from("C:\\ProgramData\\Flowy\\Voice-Companion\\models");
        if primary.exists() {
            return primary;
        }
        let legacy = PathBuf::from("C:\\ProgramData\\Flowy\\AI-Mate\\models");
        if legacy.exists() {
            return legacy;
        }
        primary
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from(DEFAULT_MODELS_DIR)
    }
}

/// Default path for the Voice Companion configuration file (platform-specific).
pub fn default_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let primary = PathBuf::from(&local_app_data)
                .join("Flowy")
                .join("Voice-Companion")
                .join("config.toml");
            if primary.exists() {
                return primary;
            }
            let legacy = PathBuf::from(&local_app_data)
                .join("Flowy")
                .join("AI-Mate")
                .join("config.toml");
            if legacy.exists() {
                return legacy;
            }
            return primary;
        }
        let primary = PathBuf::from("C:\\ProgramData\\Flowy\\Voice-Companion\\config.toml");
        if primary.exists() {
            return primary;
        }
        let legacy = PathBuf::from("C:\\ProgramData\\Flowy\\AI-Mate\\config.toml");
        if legacy.exists() {
            return legacy;
        }
        primary
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from(DEFAULT_CONFIG_PATH)
    }
}

/// Default function for serde `auto_download` field.
pub fn default_auto_download() -> bool {
    DEFAULT_AUTO_DOWNLOAD
}

/// Default function for serde `probe_timeout_ms` field.
pub fn default_probe_timeout_ms() -> u64 {
    DEFAULT_PROBE_TIMEOUT_MS
}

/// Configuration section `[models]` governing model asset management.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelsConfig {
    /// Path to root models directory holding `kws/`, `vad/`, `asr/`, and `checksums.sha256`.
    #[serde(default = "default_models_dir")]
    pub models_dir: PathBuf,

    /// Automatically trigger background download on boot if models are missing or corrupted.
    #[serde(default = "default_auto_download")]
    pub auto_download: bool,

    /// Timeout in milliseconds when probing download mirror latency.
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,

    /// Preferred mirror selection policy: `"auto"`, `"modelscope"`, or `"github"`.
    #[serde(default)]
    pub prefer_mirror: PreferMirror,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            models_dir: default_models_dir(),
            auto_download: default_auto_download(),
            probe_timeout_ms: default_probe_timeout_ms(),
            prefer_mirror: PreferMirror::default(),
        }
    }
}

/// Configuration section `[cloud]` governing Flowy Cloud and LLM provider options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CloudConfig {
    /// Optional Flowy Cloud completions endpoint URL (defaults to "https://server.flowyaipc.com/claw/v1/chat/completions").
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Optional Cloud user token (overrides autodetected session token).
    #[serde(default)]
    pub token: Option<String>,
    /// Decision routing model name (defaults to "qwen3.8-flash").
    #[serde(default)]
    pub model: Option<String>,
}

/// Top-level AI Mate companion configuration schema.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AiMateConfig {
    /// Supervisor-only credential; never persisted in the public TOML schema.
    #[serde(skip)]
    pub control_token: Option<String>,
    /// Model asset management configuration section (`[models]`).
    #[serde(default)]
    pub models: ModelsConfig,
    /// Cloud provider and decision routing section (`[cloud]`).
    #[serde(default)]
    pub cloud: CloudConfig,
}

impl fmt::Debug for AiMateConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiMateConfig")
            .field("models", &self.models)
            .field("cloud_endpoint", &self.cloud.endpoint)
            .field("cloud_model", &self.cloud.model)
            .field("has_control_token", &self.control_token.is_some())
            .field("has_cloud_token", &self.cloud.token.is_some())
            .finish()
    }
}

/// Type aliases for convenient usage.
pub type Config = AiMateConfig;
pub type VoiceCompanionConfig = AiMateConfig;

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to parse TOML configuration: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("Failed to serialize TOML configuration: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Failed to read configuration file at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Invalid prefer_mirror value: '{0}' (expected 'auto', 'modelscope', or 'github')")]
    InvalidPreferMirror(String),
}

impl AiMateConfig {
    /// Parse configuration from a TOML string without applying environment overrides.
    pub fn parse_toml(toml_str: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(toml_str)?;
        Ok(config)
    }

    /// Alias to `parse_toml` for direct deserialization from TOML strings.
    pub fn from_toml_str(toml_str: &str) -> Result<Self, ConfigError> {
        Self::parse_toml(toml_str)
    }

    /// Apply environment variable overrides using the standard environment.
    pub fn apply_env_overrides(&mut self) {
        self.apply_env_overrides_from(|key| std::env::var(key).ok());
    }

    /// Apply environment variable overrides using a custom environment lookup closure.
    ///
    /// This enables thread-safe and isolated testing without mutating process-global state.
    pub fn apply_env_overrides_from<F>(&mut self, get_env: F)
    where
        F: Fn(&str) -> Option<String>,
    {
        self.control_token = get_env("VOICE_COMPANION_CONTROL_TOKEN")
            .or_else(|| get_env("AI_MATE_CONTROL_TOKEN"))
            .filter(|value| !value.is_empty());
        if let Some(val) = get_env(ENV_VOICE_COMPANION_MODEL_DIR).or_else(|| get_env(ENV_AI_MATE_MODEL_DIR)) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                self.models.models_dir = PathBuf::from(trimmed);
            }
        }
        if let Some(val) = get_env("NOMIFUN_SERVER_TOKEN").or_else(|| get_env("FLOWY_SERVER_TOKEN"))
        {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                self.cloud.token = Some(trimmed.to_string());
            }
        }
        if let Some(val) = get_env("FLOWY_COMPLETIONS_URL") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                self.cloud.endpoint = Some(trimmed.to_string());
            }
        }
        if let Some(val) = get_env("VOICE_COMPANION_LLM_MODEL").or_else(|| get_env("AI_MATE_LLM_MODEL")) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                self.cloud.model = Some(trimmed.to_string());
            }
        }
    }

    /// Parse configuration from a TOML string and apply environment variable overrides.
    pub fn load_from_str(toml_str: &str) -> Result<Self, ConfigError> {
        let mut config = Self::parse_toml(toml_str)?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Load configuration from a specified file path and apply environment variable overrides.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|err| ConfigError::Io {
            path: path_ref.to_path_buf(),
            source: err,
        })?;
        Self::load_from_str(&content)
    }

    /// Load configuration from the default system path or fallback to defaults,
    /// then apply environment variable overrides.
    pub fn load() -> Result<Self, ConfigError> {
        let default_path = default_config_path();
        if default_path.exists() {
            Self::load_from_file(&default_path)
        } else {
            let mut config = Self::default();
            config.apply_env_overrides();
            Ok(config)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefer_mirror_traits() {
        assert_eq!(PreferMirror::Auto.as_str(), "auto");
        assert_eq!(PreferMirror::ModelScope.as_str(), "modelscope");
        assert_eq!(PreferMirror::GitHub.as_str(), "github");

        assert_eq!(PreferMirror::from_str("auto").unwrap(), PreferMirror::Auto);
        assert_eq!(
            PreferMirror::from_str("modelscope").unwrap(),
            PreferMirror::ModelScope
        );
        assert_eq!(
            PreferMirror::from_str("github").unwrap(),
            PreferMirror::GitHub
        );
        assert!(PreferMirror::from_str("unknown").is_err());
    }

    #[test]
    fn test_models_config_defaults() {
        let models = ModelsConfig::default();
        assert_eq!(models.models_dir, default_models_dir());
        assert_eq!(models.auto_download, true);
        assert_eq!(models.probe_timeout_ms, 1500);
        assert_eq!(models.prefer_mirror, PreferMirror::Auto);
    }

    #[test]
    fn test_env_override_empty_ignored() {
        let mut config = AiMateConfig::default();
        config.apply_env_overrides_from(|_| Some("   ".to_string()));
        assert_eq!(config.models.models_dir, default_models_dir());
    }
}

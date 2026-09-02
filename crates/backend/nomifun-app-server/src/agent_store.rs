//! `~/.agent-store/config.toml` — the local agent-store settings file.
//!
//! The App Server treats this file as an *optional* integration surface so it
//! can drive a real Nomi conversation without first creating providers through
//! the full Allo UI. The file lives next to the user's other agent tooling
//! (Claude Code / Codex style `config.toml`), and its `[providers.<name>]` +
//! `[models."<provider>/<model>"]` tables map onto the App Server's provider
//! model selection:
//!
//! ```toml
//! default_model = "opencode/mimo-v2.5-free"
//!
//! [providers.opencode]
//! type = "openai"
//! api_key = "sk-..."
//! base_url = "https://opencode.ai/zen/v1"
//!
//! [models."opencode/mimo-v2.5-free"]
//! provider = "opencode"
//! model = "mimo-v2.5-free"
//! display_name = "MiMo V2.5 Free"
//! max_context_size = 200000
//! ```
//!
//! Unknown top-level and nested keys are tolerated (the file belongs to the
//! user and intentionally carries settings that Allo does not consume).
//! Secrets are never logged: `api_key` is only read for the encrypted
//! provider registration and is dropped before any tracing payload.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreConfig {
    /// `"<provider>/<model>"` selection used when the caller sends no model.
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub providers: HashMap<String, AgentStoreProvider>,
    /// Keyed by `"<provider>/<model>"`.
    #[serde(default)]
    pub models: HashMap<String, AgentStoreModel>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreProvider {
    /// e.g. `"openai"`/`"anthropic"` — maps to the provider `platform`.
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreModel {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub max_context_size: Option<i64>,
    #[serde(default)]
    pub max_output_size: Option<i64>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub reasoning_key: Option<String>,
}

impl AgentStoreConfig {
    /// Parse the file at `path`. Returns a human-readable error for logging.
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        toml::from_str(&raw).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Default location: `~/.agent-store/config.toml` (Windows then POSIX).
    pub fn default_path() -> Option<PathBuf> {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
        Some(home.join(".agent-store").join("config.toml"))
    }

    /// `default_model` split into `(provider_key, model)`.
    pub fn default_selection(&self) -> Option<(String, String)> {
        let value = self.default_model.as_deref()?.trim();
        let (provider, model) = value.split_once('/')?;
        let provider = provider.trim();
        let model = model.trim();
        if provider.is_empty() || model.is_empty() {
            return None;
        }
        Some((provider.to_owned(), model.to_owned()))
    }

    /// Sorted model names whose `[models."<provider>/<model>"]` entry points at
    /// the given provider key.
    pub fn models_for_provider(&self, provider_key: &str) -> Vec<String> {
        let mut names = self
            .models
            .values()
            .filter(|entry| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|entry| {
                entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    /// Context-window map for `provider_models` (`model -> max_context_size`).
    pub fn context_limits_for_provider(&self, provider_key: &str) -> HashMap<String, i64> {
        self.models
            .iter()
            .filter(|(_, entry)| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|(key, entry)| {
                let model = entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                let limit = entry.max_context_size?;
                let _ = key;
                Some((model.to_owned(), limit))
            })
            .collect()
    }

    /// Display-name map for `provider_models` (`model -> display_name`).
    pub fn display_names_for_provider(&self, provider_key: &str) -> HashMap<String, String> {
        self.models
            .iter()
            .filter(|(_, entry)| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|(_, entry)| {
                let model = entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                let display_name = entry
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some((model.to_owned(), display_name.to_owned()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
default_plan_mode = false
default_model = "opencode/mimo-v2.5-free"

[providers.opencode]
type = "openai"
api_key = "sk-test-not-a-real-key"
base_url = "https://opencode.ai/zen/v1"

[models."opencode/laguna-s-2.1-free"]
provider = "opencode"
model = "laguna-s-2.1-free"
max_context_size = 256000
display_name = "Laguna S 2.1 Free"

[models."opencode/mimo-v2.5-free"]
provider = "opencode"
model = "mimo-v2.5-free"
max_context_size = 200000
display_name = "MiMo V2.5 Free"
reasoning_key = "reasoning_content"
"#;

    #[test]
    fn parses_provider_and_models_from_the_agent_store_file() {
        let config = AgentStoreConfig::load_from_str(SAMPLE);
        let (provider, model) = config
            .default_selection()
            .expect("default_model must resolve to provider/model");
        assert_eq!(provider, "opencode");
        assert_eq!(model, "mimo-v2.5-free");

        let opencode = config
            .providers
            .get("opencode")
            .expect("provider table must be present");
        assert_eq!(opencode.r#type.as_deref(), Some("openai"));
        assert_eq!(opencode.api_key.as_deref(), Some("sk-test-not-a-real-key"));
        assert_eq!(opencode.base_url.as_deref(), Some("https://opencode.ai/zen/v1"));

        assert_eq!(
            config.models_for_provider("opencode"),
            vec!["laguna-s-2.1-free", "mimo-v2.5-free"]
        );
        let limits = config.context_limits_for_provider("opencode");
        assert_eq!(limits.get("mimo-v2.5-free"), Some(&200_000));
        assert_eq!(limits.get("laguna-s-2.1-free"), Some(&256_000));
        let names = config.display_names_for_provider("opencode");
        assert_eq!(names.get("mimo-v2.5-free").map(String::as_str), Some("MiMo V2.5 Free"));
    }

    #[test]
    fn unknown_top_level_sections_are_tolerated() {
        let config = AgentStoreConfig::load_from_str(
            r#"
default_model = "octo/coral"
telemetry = false

[experimental]
secondary-model = true

[providers.octo]
type = "openai"
api_key = "sk-test"
base_url = "https://example.test/v1"
"#,
        );
        assert_eq!(config.default_selection().unwrap().0, "octo");
        assert!(config.providers.contains_key("octo"));
    }

    impl AgentStoreConfig {
        fn load_from_str(raw: &str) -> Self {
            toml::from_str(raw).expect("sample config must parse")
        }
    }
}
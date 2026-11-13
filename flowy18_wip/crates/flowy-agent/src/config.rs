use crate::flex::FlexProvider;
use serde_json::{json, Value};

const DEFAULT_BASE_URL: &str = "https://test.flowyaipc.cn/claw/v1";
const DEFAULT_MODEL: &str = "flowy/gml-5.1";
const PROVIDER: &str = "openai";

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub provider: String,
}

impl LlmConfig {
    #[must_use]
    pub fn from_env() -> Self {
        let api_key = std::env::var("FLOWY_LLM_API_KEY")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let base_url = std::env::var("FLOWY_LLM_BASE_URL")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let model = std::env::var("FLOWY_LLM_MODEL")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        Self {
            api_key,
            base_url,
            model,
            provider: PROVIDER.to_owned(),
        }
    }

    #[must_use]
    pub fn from_flex(provider: FlexProvider, api_key: impl Into<String>) -> Self {
        let api_key = api_key.into().trim().to_owned();
        Self {
            api_key: (!api_key.is_empty()).then_some(api_key),
            base_url: provider.base_url().to_owned(),
            model: provider.default_model().to_owned(),
            provider: provider.id().to_owned(),
        }
    }

    #[must_use]
    pub fn uses_live(&self) -> bool {
        self.api_key.is_some()
    }

    #[must_use]
    pub fn openai_section(&self) -> Option<Value> {
        let api_key = self.api_key.as_ref()?;
        Some(json!({
            "base_url": self.base_url,
            "api_key": api_key,
            "model": self.model,
            "providers": [self.provider],
        }))
    }

    #[must_use]
    pub fn agent_provider(&self) -> &str {
        if self.uses_live() {
            self.provider.as_str()
        } else {
            "mock"
        }
    }

    #[must_use]
    pub fn agent_model(&self) -> &str {
        if self.uses_live() {
            self.model.as_str()
        } else {
            "mock-1"
        }
    }
}

use serde::{Deserialize, Serialize};

/// Bring-your-own-key provider presets (OpenAI-compatible endpoints).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlexProvider {
    #[default]
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "openrouter")]
    OpenRouter,
}

impl FlexProvider {
    pub const ALL: [Self; 3] = [Self::OpenAi, Self::DeepSeek, Self::OpenRouter];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::DeepSeek => "deepseek",
            Self::OpenRouter => "openrouter",
        }
    }

    #[must_use]
    pub const fn name_key(self) -> &'static str {
        match self {
            Self::OpenAi => "flex.provider_openai",
            Self::DeepSeek => "flex.provider_deepseek",
            Self::OpenRouter => "flex.provider_openrouter",
        }
    }

    #[must_use]
    pub const fn base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::DeepSeek => "https://api.deepseek.com",
            Self::OpenRouter => "https://openrouter.ai/api/v1",
        }
    }

    #[must_use]
    pub const fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4o-mini",
            Self::DeepSeek => "deepseek-chat",
            Self::OpenRouter => "openrouter/auto",
        }
    }

    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        match id.trim() {
            "openai" => Some(Self::OpenAi),
            "deepseek" => Some(Self::DeepSeek),
            "openrouter" => Some(Self::OpenRouter),
            _ => None,
        }
    }
}

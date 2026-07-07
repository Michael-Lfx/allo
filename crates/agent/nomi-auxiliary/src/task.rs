//! Catalogue of auxiliary side tasks.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AuxiliaryTask {
    Compression,
    Vision,
    WebExtract,
    SessionSearch,
    SkillsHub,
    Mcp,
    FlushMemories,
    Title,
    Classify,
    Custom(String),
}

impl AuxiliaryTask {
    pub fn as_key(&self) -> &str {
        match self {
            Self::Compression => "compression",
            Self::Vision => "vision",
            Self::WebExtract => "web_extract",
            Self::SessionSearch => "session_search",
            Self::SkillsHub => "skills_hub",
            Self::Mcp => "mcp",
            Self::FlushMemories => "flush_memories",
            Self::Title => "title",
            Self::Classify => "classify",
            Self::Custom(name) => name.as_str(),
        }
    }

    pub fn default_timeout(&self) -> Duration {
        match self {
            Self::Vision => Duration::from_secs(60),
            Self::Compression | Self::FlushMemories => Duration::from_secs(45),
            _ => Duration::from_secs(30),
        }
    }
}

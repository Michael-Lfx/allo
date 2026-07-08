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
            AuxiliaryTask::Compression => "compression",
            AuxiliaryTask::Vision => "vision",
            AuxiliaryTask::WebExtract => "web_extract",
            AuxiliaryTask::SessionSearch => "session_search",
            AuxiliaryTask::SkillsHub => "skills_hub",
            AuxiliaryTask::Mcp => "mcp",
            AuxiliaryTask::FlushMemories => "flush_memories",
            AuxiliaryTask::Title => "title",
            AuxiliaryTask::Classify => "classify",
            AuxiliaryTask::Custom(name) => name.as_str(),
        }
    }

    pub fn default_timeout(&self) -> Duration {
        match self {
            AuxiliaryTask::Vision => Duration::from_secs(60),
            AuxiliaryTask::Compression | AuxiliaryTask::FlushMemories => Duration::from_secs(45),
            _ => Duration::from_secs(30),
        }
    }
}

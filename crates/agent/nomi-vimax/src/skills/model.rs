//! Vertical Skill domain types for ViMax Mode × Skill.

use serde::{Deserialize, Serialize};

use crate::domain::WorkflowKind;

/// Where a skill package was loaded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Builtin,
    User,
    Hub,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
            Self::Hub => "hub",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "builtin" => Some(Self::Builtin),
            "user" => Some(Self::User),
            "hub" => Some(Self::Hub),
            _ => None,
        }
    }
}

/// Publication / visibility state for user-authored skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillVisibility {
    /// Only the local user catalog.
    #[default]
    Private,
    /// Listed in the local Skill Hub (shareable on this device).
    Hub,
    /// Kept for import/export packages that are not yet installed.
    Unlisted,
}

impl SkillVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Hub => "hub",
            Self::Unlisted => "unlisted",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "private" | "" => Some(Self::Private),
            "hub" | "published" => Some(Self::Hub),
            "unlisted" => Some(Self::Unlisted),
            _ => None,
        }
    }
}

/// Source-qualified skill identity (`builtin:luxury-tvc`, `user:my-tvc`, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SkillId {
    pub source: SkillSource,
    pub name: String,
}

impl Serialize for SkillId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.qualified())
    }
}

impl<'de> Deserialize<'de> for SkillId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        SkillId::parse(&raw).ok_or_else(|| serde::de::Error::custom(format!("invalid skill id: {raw}")))
    }
}

impl SkillId {
    pub fn new(source: SkillSource, name: impl Into<String>) -> Self {
        Self {
            source,
            name: name.into(),
        }
    }

    pub fn qualified(&self) -> String {
        format!("{}:{}", self.source.as_str(), self.name)
    }

    /// Parse `source:name` or bare `name` (defaults to user).
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        if let Some((source, name)) = raw.split_once(':') {
            let source = SkillSource::parse(source)?;
            let name = sanitize_skill_name(name)?;
            return Some(Self { source, name });
        }
        let name = sanitize_skill_name(raw)?;
        Some(Self {
            source: SkillSource::User,
            name,
        })
    }
}

impl std::fmt::Display for SkillId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.qualified())
    }
}

/// Full vertical skill package used at plan time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerticalSkill {
    pub id: SkillId,
    pub name: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Modes this skill may attach to. Empty = all modes.
    #[serde(default)]
    pub compatible_modes: Vec<WorkflowKind>,
    #[serde(default)]
    pub visibility: SkillVisibility,
    /// Injected into `<USER_REQUIREMENT>` (narrative / structure / QA).
    #[serde(default)]
    pub requirement_overlay: String,
    /// Injected into visual `style` (look / cinematography).
    #[serde(default)]
    pub style_overlay: String,
    /// Director playbook markdown body (also folded into requirement overlay).
    #[serde(default)]
    pub playbook: String,
    /// Absolute directory containing SKILL.md (empty for pure builtins).
    #[serde(default)]
    pub dir: String,
}

impl VerticalSkill {
    pub fn compatible_with(&self, mode: WorkflowKind) -> bool {
        self.compatible_modes.is_empty() || self.compatible_modes.contains(&mode)
    }

    /// Catalog list item (no heavy overlays).
    pub fn to_summary(&self) -> VerticalSkillSummary {
        VerticalSkillSummary {
            id: self.id.qualified(),
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            category: self.category.clone(),
            version: self.version.clone(),
            tags: self.tags.clone(),
            compatible_modes: self
                .compatible_modes
                .iter()
                .map(|m| m.as_str().to_string())
                .collect(),
            source: self.id.source.as_str().to_string(),
            visibility: self.visibility.as_str().to_string(),
            has_style_overlay: !self.style_overlay.trim().is_empty(),
            has_requirement_overlay: !self.requirement_overlay.trim().is_empty()
                || !self.playbook.trim().is_empty(),
        }
    }
}

/// Lightweight catalog entry for UI / API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerticalSkillSummary {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub version: String,
    pub tags: Vec<String>,
    pub compatible_modes: Vec<String>,
    pub source: String,
    pub visibility: String,
    pub has_style_overlay: bool,
    pub has_requirement_overlay: bool,
}

/// Draft payload for creating / updating a user skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerticalSkillDraft {
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub compatible_modes: Vec<String>,
    #[serde(default)]
    pub requirement_overlay: Option<String>,
    #[serde(default)]
    pub style_overlay: Option<String>,
    #[serde(default)]
    pub playbook: Option<String>,
}

/// Result of merging selected skills into plan inputs.
#[derive(Debug, Clone, Default)]
pub struct SkillOverlay {
    pub user_requirement: String,
    pub style: String,
    pub applied_skill_ids: Vec<String>,
}

/// Lowercase kebab-case skill directory / id name.
pub fn sanitize_skill_name(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() || s.len() > 64 {
        return None;
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    if s.starts_with('-') || s.ends_with('-') {
        return None;
    }
    Some(s.replace('_', "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_id_roundtrip() {
        let id = SkillId::parse("builtin:luxury-tvc").unwrap();
        assert_eq!(id.source, SkillSource::Builtin);
        assert_eq!(id.name, "luxury-tvc");
        assert_eq!(id.qualified(), "builtin:luxury-tvc");
    }
}

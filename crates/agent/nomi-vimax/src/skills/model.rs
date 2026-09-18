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
    /// Executable packing / duration policy. Overlay prose cannot change this.
    #[serde(default)]
    pub director: DirectorSpec,
    /// Absolute directory containing SKILL.md (empty for pure builtins).
    #[serde(default)]
    pub dir: String,
}

impl VerticalSkill {
    pub fn compatible_with(&self, mode: WorkflowKind) -> bool {
        // Action imitation has no planning overlay surface — only opt-in skills apply.
        if mode.is_action_imitation() {
            return self.compatible_modes.contains(&mode);
        }
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
    /// LibTV-aligned: when / where to use this skill.
    #[serde(default)]
    pub use_scenario: Option<String>,
    /// LibTV-aligned: how the user should invoke it / required inputs.
    #[serde(default)]
    pub how_to_use: Option<String>,
    /// LibTV-aligned: expected output artifact description.
    #[serde(default)]
    pub output: Option<String>,
    /// Optional cover image URL for Skill Hub listing.
    #[serde(default)]
    pub cover_url: Option<String>,
    /// Optional featured case / demo URL.
    #[serde(default)]
    pub case_url: Option<String>,
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
    pub director: DirectorSpec,
}

/// How adjacent storyboard rows become video jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PackPolicy {
    /// Pack adjacent rows that fit the model window (current default).
    #[default]
    Dense,
    /// Only pack reverse / over-shoulder coverage; every other row is a clip.
    Coverage,
}

impl PackPolicy {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "coverage" | "coverage-first" | "coveragefirst" => Self::Coverage,
            _ => Self::Dense,
        }
    }
}

/// What to do when the planned list exceeds the duration budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OverBudget {
    /// Keep overflow text by folding it into the last kept unit. Never drop the tail.
    #[default]
    Fold,
    /// Keep every unit; the film may run longer than the target.
    Extend,
    /// Drop the tail (legacy). Skills must opt in.
    Truncate,
}

impl OverBudget {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "extend" => Self::Extend,
            "truncate" => Self::Truncate,
            _ => Self::Fold,
        }
    }
}

/// Skill-owned planning policy that actually changes packing and budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct DirectorSpec {
    pub pack_policy: PackPolicy,
    pub over_budget: OverBudget,
}

impl DirectorSpec {
    /// Last skill wins fields it actually set (non-default).
    pub fn merge(self, other: Self) -> Self {
        Self {
            pack_policy: if other.pack_policy != PackPolicy::default() {
                other.pack_policy
            } else {
                self.pack_policy
            },
            over_budget: if other.over_budget != OverBudget::default() {
                other.over_budget
            } else {
                self.over_budget
            },
        }
    }

    /// Scene dir first, then film root. Missing file → default (dense + fold).
    pub fn load_from_dir(dir: &std::path::Path) -> Self {
        for candidate in [dir, dir.parent().unwrap_or(dir)] {
            let path = candidate.join("director_spec.json");
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(spec) = serde_json::from_str(&raw) {
                    return spec;
                }
            }
        }
        Self::default()
    }
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

    #[test]
    fn director_merge_last_non_default_wins() {
        let dense_fold = DirectorSpec::default();
        let coverage = DirectorSpec {
            pack_policy: PackPolicy::Coverage,
            over_budget: OverBudget::Fold,
        };
        let extend = DirectorSpec {
            pack_policy: PackPolicy::Dense,
            over_budget: OverBudget::Extend,
        };
        let merged = dense_fold.merge(coverage).merge(extend);
        assert_eq!(merged.pack_policy, PackPolicy::Coverage);
        assert_eq!(merged.over_budget, OverBudget::Extend);
    }
}

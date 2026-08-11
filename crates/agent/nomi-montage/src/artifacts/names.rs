//! Canonical artifact schema names (one JSON Schema file per name under
//! `assets/schemas/artifacts/`).

/// Every artifact schema this crate ships. Kept as a flat list (rather than an
/// enum) because pipelines and tools reference artifacts by their YAML string
/// name, and new artifact types should not require a Rust code change.
pub const ARTIFACT_NAMES: &[&str] = &[
    "brief",
    "research_brief",
    "proposal_packet",
    "decision_log",
    "script",
    "scene_plan",
    "asset_manifest",
    "edit_decisions",
    "render_report",
    "final_review",
    "publish_log",
    "cost_log",
    "video_analysis_brief",
];

/// A reference to a written artifact — either a schema-validated JSON artifact
/// (`name` = artifact schema name) or a plain file (image/video/audio) produced
/// by a tool. Used in [`crate::tools::contract::ToolResult`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactRef {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub kind: ArtifactRefKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRefKind {
    #[default]
    Json,
    Image,
    Video,
    Audio,
    File,
}

impl ArtifactRef {
    pub fn json(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind: ArtifactRefKind::Json,
        }
    }

    pub fn media(name: impl Into<String>, path: impl Into<String>, kind: ArtifactRefKind) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind,
        }
    }
}

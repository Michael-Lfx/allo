//! Canonical stage → artifact-schema name mapping.
//!
//! Mirrors OpenMontage's `CANONICAL_STAGE_ARTIFACTS` concept: every stage name
//! that appears across the shipped pipelines maps to exactly one artifact
//! schema, so the reviewer/orchestrator can validate "the" output of a stage
//! without re-deriving it from `produces[0]` each time (which stays the
//! source of truth — this table is a fast, testable mirror of it).
pub const CANONICAL_STAGE_ARTIFACTS: &[(&str, &str)] = &[
    ("brief", "brief"),
    ("research", "research_brief"),
    ("video_analysis", "video_analysis_brief"),
    ("proposal", "proposal_packet"),
    ("script", "script"),
    ("scene_plan", "scene_plan"),
    ("assets", "asset_manifest"),
    ("capture_plan", "scene_plan"),
    ("edit", "edit_decisions"),
    ("compose", "render_report"),
    ("avatar_render", "render_report"),
    ("talking_head_render", "render_report"),
    ("review", "final_review"),
    ("publish", "publish_log"),
];

/// Canonical artifact schema name for a stage, if known.
pub fn canonical_artifact_for_stage(stage_name: &str) -> Option<&'static str> {
    CANONICAL_STAGE_ARTIFACTS
        .iter()
        .find(|(s, _)| *s == stage_name)
        .map(|(_, a)| *a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_stages_resolve() {
        assert_eq!(canonical_artifact_for_stage("script"), Some("script"));
        assert_eq!(
            canonical_artifact_for_stage("compose"),
            Some("render_report")
        );
        assert_eq!(canonical_artifact_for_stage("nonexistent"), None);
    }
}

//! Compose Mode × Vertical Skill overlays into plan inputs.

use crate::domain::WorkflowKind;

use super::model::{SkillOverlay, VerticalSkill};

const REQUIREMENT_HEADER: &str = "[VERTICAL_SKILLS]";
const STYLE_SEP: &str = " | ";

/// Merge selected skills into base user_requirement + style for a mode.
pub fn compose_overlays(
    mode: WorkflowKind,
    skills: &[VerticalSkill],
    base_requirement: &str,
    base_style: &str,
) -> SkillOverlay {
    let mut applied = Vec::new();
    let mut req_blocks = Vec::new();
    let mut style_parts = Vec::new();

    let base_style = base_style.trim();
    if !base_style.is_empty() {
        style_parts.push(base_style.to_string());
    }

    for skill in skills {
        if !skill.compatible_with(mode) {
            tracing::warn!(
                skill = %skill.id.qualified(),
                mode = mode.as_str(),
                "skipping skill incompatible with mode"
            );
            continue;
        }
        applied.push(skill.id.qualified());

        let mut block = format!(
            "### {} ({})\n{}",
            skill.display_name,
            skill.id.qualified(),
            skill.description.trim()
        );
        let req = skill.requirement_overlay.trim();
        if !req.is_empty() {
            block.push_str("\n\n");
            block.push_str(req);
        }
        let playbook = skill.playbook.trim();
        if !playbook.is_empty() {
            block.push_str("\n\n");
            block.push_str(playbook);
        }
        req_blocks.push(block);

        let style = skill.style_overlay.trim();
        if !style.is_empty() && !style_parts.iter().any(|s| s == style) {
            style_parts.push(style.to_string());
        }
    }

    let mut user_requirement = base_requirement.trim().to_string();
    if !req_blocks.is_empty() {
        let skills_section = format!(
            "{REQUIREMENT_HEADER}\nFollow these vertical director skills when planning story, shots, assets, and QA:\n\n{}",
            req_blocks.join("\n\n---\n\n")
        );
        if user_requirement.is_empty() {
            user_requirement = skills_section;
        } else if user_requirement.contains(REQUIREMENT_HEADER) {
            // Already composed (e.g. replan) — replace previous skills block if present.
            user_requirement = replace_or_append_skills_block(&user_requirement, &skills_section);
        } else {
            user_requirement = format!("{user_requirement}\n\n{skills_section}");
        }
    }

    SkillOverlay {
        user_requirement,
        style: style_parts.join(STYLE_SEP),
        applied_skill_ids: applied,
    }
}

fn replace_or_append_skills_block(existing: &str, new_section: &str) -> String {
    if let Some(idx) = existing.find(REQUIREMENT_HEADER) {
        let prefix = existing[..idx].trim_end();
        if prefix.is_empty() {
            new_section.to_string()
        } else {
            format!("{prefix}\n\n{new_section}")
        }
    } else {
        format!("{}\n\n{new_section}", existing.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::model::{SkillId, SkillSource, SkillVisibility};

    fn sample_skill() -> VerticalSkill {
        VerticalSkill {
            id: SkillId::new(SkillSource::Builtin, "luxury-tvc"),
            name: "luxury-tvc".into(),
            display_name: "Luxury TVC".into(),
            description: "Premium commercial".into(),
            category: "advertising".into(),
            version: "1.0.0".into(),
            tags: vec![],
            compatible_modes: vec![WorkflowKind::Idea2Video],
            visibility: SkillVisibility::Hub,
            requirement_overlay: "Sparse premium pacing.".into(),
            style_overlay: "luxury commercial light".into(),
            playbook: "Think Hermes campaign.".into(),
            dir: String::new(),
        }
    }

    #[test]
    fn composes_requirement_and_style() {
        let overlay = compose_overlays(
            WorkflowKind::Idea2Video,
            &[sample_skill()],
            "Keep brand logo subtle",
            "cinematic",
        );
        assert!(overlay.user_requirement.contains("VERTICAL_SKILLS"));
        assert!(overlay.user_requirement.contains("Sparse premium"));
        assert!(overlay.style.contains("cinematic"));
        assert!(overlay.style.contains("luxury commercial"));
        assert_eq!(overlay.applied_skill_ids, vec!["builtin:luxury-tvc"]);
    }

    #[test]
    fn skips_incompatible_mode() {
        let overlay = compose_overlays(
            WorkflowKind::Novel2Video,
            &[sample_skill()],
            "",
            "",
        );
        assert!(overlay.applied_skill_ids.is_empty());
        assert!(overlay.user_requirement.is_empty());
    }
}

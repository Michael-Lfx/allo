//! Embedded official vertical skills (shipped with nomi-vimax).

use crate::error::VimaxResult;

use super::model::{SkillId, SkillSource, VerticalSkill};
use super::parse::parse_skill_md;

const BUILTIN_SKILLS: &[(&str, &str)] = &[
    (
        "luxury-tvc",
        include_str!("../../skills/builtin/luxury-tvc/SKILL.md"),
    ),
    (
        "travel-master",
        include_str!("../../skills/builtin/travel-master/SKILL.md"),
    ),
    (
        "fight-fx",
        include_str!("../../skills/builtin/fight-fx/SKILL.md"),
    ),
    (
        "female-drama",
        include_str!("../../skills/builtin/female-drama/SKILL.md"),
    ),
    (
        "wes-anderson",
        include_str!("../../skills/builtin/wes-anderson/SKILL.md"),
    ),
    (
        "product-demo",
        include_str!("../../skills/builtin/product-demo/SKILL.md"),
    ),
    (
        "documentary-observational",
        include_str!("../../skills/builtin/documentary-observational/SKILL.md"),
    ),
    (
        "horror-suspense",
        include_str!("../../skills/builtin/horror-suspense/SKILL.md"),
    ),
    (
        "music-visual",
        include_str!("../../skills/builtin/music-visual/SKILL.md"),
    ),
    (
        "short-drama",
        include_str!("../../skills/builtin/short-drama/SKILL.md"),
    ),
    (
        "character-bible",
        include_str!("../../skills/builtin/character-bible/SKILL.md"),
    ),
    (
        "scene-bible",
        include_str!("../../skills/builtin/scene-bible/SKILL.md"),
    ),
    (
        "script-to-board",
        include_str!("../../skills/builtin/script-to-board/SKILL.md"),
    ),
    (
        "replica-ref",
        include_str!("../../skills/builtin/replica-ref/SKILL.md"),
    ),
    (
        "multi-shot-cut",
        include_str!("../../skills/builtin/multi-shot-cut/SKILL.md"),
    ),
    (
        "tail-to-head",
        include_str!("../../skills/builtin/tail-to-head/SKILL.md"),
    ),
];

/// Qualified id of the default director injected for idea-driven films when
/// the user selected no vertical skill (see `service` plan composition).
pub const DEFAULT_SHORT_DRAMA_SKILL_ID: &str = "builtin:short-drama";

pub fn load_builtin_skills() -> VimaxResult<Vec<VerticalSkill>> {
    let mut out = Vec::with_capacity(BUILTIN_SKILLS.len());
    for (name, raw) in BUILTIN_SKILLS {
        let id = SkillId::new(SkillSource::Builtin, *name);
        let mut skill = parse_skill_md(raw, id, String::new())?;
        skill.visibility = super::model::SkillVisibility::Hub;
        out.push(skill);
    }
    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_parse() {
        let skills = load_builtin_skills().unwrap();
        assert_eq!(skills.len(), 16);
        assert!(skills.iter().any(|s| s.name == "luxury-tvc"));
        assert!(skills.iter().any(|s| s.name == "product-demo"));
        assert!(skills.iter().any(|s| s.name == "character-bible"));
        assert!(skills.iter().all(|s| s.compatible_modes.is_empty()));
    }

    #[test]
    fn default_short_drama_skill_is_requirement_only() {
        let skills = load_builtin_skills().unwrap();
        let skill = skills
            .iter()
            .find(|s| s.id.qualified() == DEFAULT_SHORT_DRAMA_SKILL_ID)
            .expect("short-drama builtin");
        assert!(skill.requirement_overlay.contains("COVERAGE"));
        assert!(!skill.requirement_overlay.contains("NEVER add shots"));
        assert_eq!(skill.director.pack_policy, crate::skills::PackPolicy::Dense);
        assert_eq!(skill.director.over_budget, crate::skills::OverBudget::Fold);
        // Default-injected: it must not hijack the user's visual style.
        assert!(skill.style_overlay.trim().is_empty());
    }

    #[test]
    fn vertical_directors_set_pack_policy() {
        let skills = load_builtin_skills().unwrap();
        let female = skills.iter().find(|s| s.name == "female-drama").unwrap();
        assert_eq!(female.director.over_budget, crate::skills::OverBudget::Extend);
        let horror = skills.iter().find(|s| s.name == "horror-suspense").unwrap();
        assert_eq!(horror.director.pack_policy, crate::skills::PackPolicy::Coverage);
        let fight = skills.iter().find(|s| s.name == "fight-fx").unwrap();
        assert_eq!(fight.director.pack_policy, crate::skills::PackPolicy::Coverage);
    }
}

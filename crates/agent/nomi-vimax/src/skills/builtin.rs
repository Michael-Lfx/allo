//! Embedded official vertical skills (shipped with nomi-vimax).

use crate::error::VimaxResult;

use super::model::{SkillId, SkillSource, VerticalSkill};
use super::parse::parse_skill_md;

/// Workshop-first order: genre directors, scene craft, storyboard manuals,
/// then adjacent (ad / travel / MV) styles kept for canvas and search.
const FEATURED_ORDER: &[&str] = &[
    "short-drama",
    "female-drama",
    "revenge-rise",
    "costume-romance",
    "urban-ceo",
    "xianxia-romance",
    "horror-suspense",
    "scene-directing",
    "fight-fx",
    "character-bible",
    "scene-bible",
    "script-to-board",
    "replica-ref",
    "multi-shot-cut",
    "tail-to-head",
    "luxury-tvc",
    "wes-anderson",
    "product-demo",
    "travel-master",
    "music-visual",
    "documentary-observational",
];

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
        "scene-directing",
        include_str!("../../skills/builtin/scene-directing/SKILL.md"),
    ),
    (
        "revenge-rise",
        include_str!("../../skills/builtin/revenge-rise/SKILL.md"),
    ),
    (
        "costume-romance",
        include_str!("../../skills/builtin/costume-romance/SKILL.md"),
    ),
    (
        "urban-ceo",
        include_str!("../../skills/builtin/urban-ceo/SKILL.md"),
    ),
    (
        "xianxia-romance",
        include_str!("../../skills/builtin/xianxia-romance/SKILL.md"),
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

fn featured_rank(name: &str) -> usize {
    FEATURED_ORDER
        .iter()
        .position(|item| *item == name)
        .unwrap_or(FEATURED_ORDER.len())
}

/// Qualified id of the default director injected for idea-driven films when
/// the user selected no vertical skill (see `service` plan composition).
pub const DEFAULT_SHORT_DRAMA_SKILL_ID: &str = "builtin:short-drama";
/// In-frame craft stacked onto the default short-drama director. Not injected
/// when the user already picked an explicit skill list.
pub const SCENE_DIRECTING_SKILL_ID: &str = "builtin:scene-directing";

/// Idea-driven films with an empty skill picker: density director + scene craft.
pub fn default_idea2video_skill_ids() -> Vec<String> {
    vec![
        DEFAULT_SHORT_DRAMA_SKILL_ID.to_string(),
        SCENE_DIRECTING_SKILL_ID.to_string(),
    ]
}

pub fn load_builtin_skills() -> VimaxResult<Vec<VerticalSkill>> {
    let mut out = Vec::with_capacity(BUILTIN_SKILLS.len());
    for (name, raw) in BUILTIN_SKILLS {
        let id = SkillId::new(SkillSource::Builtin, *name);
        let mut skill = parse_skill_md(raw, id, String::new())?;
        skill.visibility = super::model::SkillVisibility::Hub;
        out.push(skill);
    }
    out.sort_by(|a, b| {
        featured_rank(&a.name)
            .cmp(&featured_rank(&b.name))
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_parse() {
        let skills = load_builtin_skills().unwrap();
        assert_eq!(skills.len(), FEATURED_ORDER.len());
        assert_eq!(skills.len(), 21);
        assert_eq!(skills[0].name, "short-drama");
        let mut seen = std::collections::HashSet::new();
        for name in FEATURED_ORDER {
            assert!(seen.insert(*name), "duplicate featured skill {name}");
        }
        for (name, _) in BUILTIN_SKILLS {
            assert!(FEATURED_ORDER.contains(name), "{name} missing from FEATURED_ORDER");
        }
        assert!(skills.iter().any(|s| s.name == "revenge-rise"));
        assert!(skills.iter().any(|s| s.name == "costume-romance"));
        assert!(skills.iter().any(|s| s.name == "urban-ceo"));
        assert!(skills.iter().any(|s| s.name == "xianxia-romance"));
        assert!(skills.iter().any(|s| s.name == "character-bible"));
        assert!(skills.iter().any(|s| s.name == "scene-directing"));
        assert!(skills.iter().any(|s| s.name == "luxury-tvc"));
        assert!(skills.iter().all(|s| s.compatible_modes.is_empty()));
    }

    #[test]
    fn default_idea2video_stacks_scene_craft_on_short_drama() {
        let ids = default_idea2video_skill_ids();
        assert_eq!(
            ids,
            vec![
                DEFAULT_SHORT_DRAMA_SKILL_ID.to_string(),
                SCENE_DIRECTING_SKILL_ID.to_string()
            ]
        );
    }

    #[test]
    fn scene_directing_is_requirement_only_and_maps_to_storyboard_fields() {
        let skills = load_builtin_skills().unwrap();
        let skill = skills
            .iter()
            .find(|s| s.id.qualified() == SCENE_DIRECTING_SKILL_ID)
            .expect("scene-directing builtin");
        assert!(skill.requirement_overlay.contains("NORTH STAR"));
        assert!(skill.requirement_overlay.contains("PERFORMANCE CHAIN"));
        assert!(skill.requirement_overlay.contains("IN-FRAME LIFE"));
        assert!(skill.requirement_overlay.contains("PROP STATES"));
        assert!(skill.requirement_overlay.contains("visual_desc"));
        assert!(!skill.requirement_overlay.contains("NEVER add shots"));
        assert_eq!(skill.director.pack_policy, crate::skills::PackPolicy::Dense);
        assert_eq!(skill.director.over_budget, crate::skills::OverBudget::Fold);
        assert!(skill.style_overlay.trim().is_empty());
        assert!(!skill.playbook.contains("Leos"));
        assert!(!skill.playbook.contains("5000"));
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
    fn scene_directing_composes_with_short_drama_without_hijacking_style() {
        use crate::domain::WorkflowKind;
        let skills = load_builtin_skills().unwrap();
        let short = skills
            .iter()
            .find(|s| s.id.qualified() == DEFAULT_SHORT_DRAMA_SKILL_ID)
            .cloned()
            .unwrap();
        let scene = skills
            .iter()
            .find(|s| s.id.qualified() == SCENE_DIRECTING_SKILL_ID)
            .cloned()
            .unwrap();
        let overlay = crate::skills::compose_overlays(
            WorkflowKind::Idea2Video,
            &[short, scene],
            "a reunion at a noodle stall",
            "cinematic",
        );
        assert!(overlay.user_requirement.contains("HOOK"));
        assert!(overlay.user_requirement.contains("NORTH STAR"));
        assert!(overlay.user_requirement.contains("IN-FRAME LIFE"));
        assert_eq!(overlay.style, "cinematic");
        assert_eq!(overlay.director.pack_policy, crate::skills::PackPolicy::Dense);
        assert_eq!(
            overlay.applied_skill_ids,
            vec![
                DEFAULT_SHORT_DRAMA_SKILL_ID.to_string(),
                SCENE_DIRECTING_SKILL_ID.to_string()
            ]
        );
    }

    #[test]
    fn vertical_directors_set_pack_policy() {
        let skills = load_builtin_skills().unwrap();
        let female = skills.iter().find(|s| s.name == "female-drama").unwrap();
        assert_eq!(female.director.over_budget, crate::skills::OverBudget::Extend);
        assert!(female.style_overlay.trim().is_empty());
        let horror = skills.iter().find(|s| s.name == "horror-suspense").unwrap();
        assert_eq!(horror.director.pack_policy, crate::skills::PackPolicy::Coverage);
        assert_eq!(horror.display_name, "悬疑短剧");
        let fight = skills.iter().find(|s| s.name == "fight-fx").unwrap();
        assert_eq!(fight.director.pack_policy, crate::skills::PackPolicy::Coverage);
        assert_eq!(fight.display_name, "动作战神");
    }

    #[test]
    fn short_drama_genre_skills_are_requirement_only() {
        let skills = load_builtin_skills().unwrap();
        for name in [
            "revenge-rise",
            "costume-romance",
            "urban-ceo",
            "xianxia-romance",
        ] {
            let skill = skills.iter().find(|s| s.name == name).expect(name);
            assert!(
                skill.style_overlay.trim().is_empty(),
                "{name} must not hijack Look"
            );
            assert_eq!(skill.director.pack_policy, crate::skills::PackPolicy::Dense);
            assert_eq!(skill.director.over_budget, crate::skills::OverBudget::Extend);
        }
        let revenge = skills.iter().find(|s| s.name == "revenge-rise").unwrap();
        assert!(revenge.requirement_overlay.contains("SAME-SPACE PAYOFF"));
        let costume = skills.iter().find(|s| s.name == "costume-romance").unwrap();
        assert!(costume.requirement_overlay.contains("SWEET DENSITY"));
        let urban = skills.iter().find(|s| s.name == "urban-ceo").unwrap();
        assert!(urban.requirement_overlay.contains("STATUS GAP"));
        let xianxia = skills.iter().find(|s| s.name == "xianxia-romance").unwrap();
        assert!(xianxia.requirement_overlay.contains("OATH VS DUTY"));
    }
}

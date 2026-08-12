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
];

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
        assert_eq!(skills.len(), 5);
        assert!(skills.iter().any(|s| s.name == "luxury-tvc"));
    }
}

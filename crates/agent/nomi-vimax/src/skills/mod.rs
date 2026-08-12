//! ViMax vertical skills — Mode × Skill playbooks and overlays.

mod builtin;
mod catalog;
mod model;
mod overlay;
mod parse;

pub use catalog::SkillCatalog;
pub use model::{
    sanitize_skill_name, SkillId, SkillOverlay, SkillSource, SkillVisibility, VerticalSkill,
    VerticalSkillDraft, VerticalSkillSummary,
};
pub use overlay::compose_overlays;
pub use parse::{build_skill_md, load_skill_dir, parse_skill_md, SKILL_MANIFEST};

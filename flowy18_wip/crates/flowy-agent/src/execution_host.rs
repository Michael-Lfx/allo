//! Shared model and skill prompt setup for agent entry points.

use flowy_agent_core::{ModelRoute, PromptTurn};
use flowy_skills::{
    InstalledSkill, SkillId, format_available_skills, format_skill_invocation,
};

use crate::config::LlmConfig;

pub(super) fn model_route(config: &LlmConfig) -> ModelRoute {
    ModelRoute {
        provider: if config.provider.is_empty() {
            "flowy".into()
        } else {
            config.provider.clone()
        },
        model: if config.model.is_empty() {
            "mock".into()
        } else {
            config.model.clone()
        },
        base_url: config.base_url.clone(),
        context_window: config.context_window,
        max_tokens: config.max_tokens,
    }
}

pub(super) fn prepare_skill_prompt(
    text: &str,
    skills: &[InstalledSkill],
) -> Result<(PromptTurn, Vec<SkillId>), &'static str> {
    let Some(command) = text.strip_prefix("/skill:") else {
        return Ok((PromptTurn::plain(text), Vec::new()));
    };
    let (name, args) = command
        .split_once(' ')
        .map_or((command, ""), |(name, args)| (name, args.trim()));
    let name = name.trim();
    if name.is_empty() {
        return Err("notice.skill_not_found");
    }
    let mut matches: Vec<&InstalledSkill> = skills
        .iter()
        .filter(|skill| {
            skill.name == name
                || format!("{}/{}", skill.id.owner_handle, skill.id.slug)
                    == name
        })
        .collect();
    let skill = match matches.len() {
        0 => return Err("notice.skill_not_found"),
        1 => matches.remove(0),
        _ => return Err("notice.skill_ambiguous"),
    };
    let model =
        format_skill_invocation(skill, (!args.is_empty()).then_some(args));
    Ok((
        PromptTurn::with_display(model, text),
        vec![skill.id.clone()],
    ))
}

pub(super) fn append_skill_listing(
    base: &str,
    skills: &[InstalledSkill],
) -> String {
    let listing = format_available_skills(skills);
    match (base.trim().is_empty(), listing.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base.to_owned(),
        (true, false) => listing,
        (false, false) => format!("{base}\n\n{listing}"),
    }
}

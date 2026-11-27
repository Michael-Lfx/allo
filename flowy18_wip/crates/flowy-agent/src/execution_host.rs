//! Shared model and skill prompt setup for agent entry points.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flowy_agent_core::{
    ModelRoute, PromptTurn,
    tools::{ToolDefinition, ToolRegistry},
};
use flowy_skills::{
    InstalledSkill, SkillId, format_available_skills, format_skill_invocation,
};

use crate::config::LlmConfig;

/// Track real body dispatch, after policy, guards, account gate and budgets.
/// The locked pi shell intentionally leaves normally spawned descendants
/// alive, so its returned future is not a writer-quiescence proof. Unknown
/// extension bodies receive the same conservative treatment. Merely exposing
/// a schema (or rejecting a call) does not contaminate this turn.
pub fn track_unproven_process_dispatches(
    registry: &ToolRegistry,
) -> Arc<AtomicBool> {
    let dispatched = Arc::new(AtomicBool::new(false));
    for name in registry.list() {
        if !crate::tool_policy::tool_policy(&name).unproven_process {
            continue;
        }
        let Some(definition) = registry.get(&name) else {
            continue;
        };
        let execute = Arc::clone(&definition.execute);
        let flag = Arc::clone(&dispatched);
        let mut tracked = ToolDefinition::new(
            definition.name.clone(),
            definition.description.clone(),
            definition.parameters.clone(),
            move |args, context| {
                flag.store(true, Ordering::Release);
                execute(args, context)
            },
        );
        tracked.is_concurrency_safe = definition.is_concurrency_safe;
        if definition.supports_artifact_delivery() {
            tracked = tracked.with_artifact_delivery();
        }
        registry.register(Arc::new(tracked));
    }
    dispatched
}

pub fn model_route(config: &LlmConfig) -> ModelRoute {
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

pub fn prepare_skill_prompt(
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

pub fn append_skill_listing(base: &str, skills: &[InstalledSkill]) -> String {
    let listing = format_available_skills(skills);
    match (base.trim().is_empty(), listing.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base.to_owned(),
        (true, false) => listing,
        (false, false) => format!("{base}\n\n{listing}"),
    }
}

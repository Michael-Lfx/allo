//! Assembles the system/user prompts fed to `MediaChat::complete_text`.
//!
//! `MediaChat` is single-shot (`system`, `user`) → `String`, not a stateful
//! chat API, so multi-turn tool-calling is simulated by re-sending the whole
//! system prompt every turn and folding prior turns into the user message as
//! a growing transcript (see `orchestrator::stage_runner`). The strict JSON
//! reply protocol below is what makes that transcript machine-parseable.

use crate::error::MontageResult;
use crate::pipeline::{PipelineManifest, StageSpec};
use crate::tools::ToolSpec;

const PROTOCOL_INSTRUCTIONS: &str = r#"
## Response protocol (strict)

Reply with **exactly one** JSON object per turn — no prose before or after it,
no markdown code fences. Two shapes are allowed:

1. Call a tool:
   {"tool": "<tool_name>", "arguments": { ... }}

2. Declare the stage finished:
   {"stage_complete": true, "summary": "<one paragraph: what you produced and why it satisfies success_criteria>"}

Rules:
- Only call tools listed in "Tools available this stage" below, by their exact name.
- `arguments` must match that tool's input schema.
- Call `write_artifact` for every artifact this stage's manifest lists under
  `produces` before declaring `stage_complete` — a stage is not done until its
  canonical artifact exists and validates.
- If a tool result contains `"ok": false`, do not retry the same call unchanged;
  either fix the arguments or, if the capability itself is missing/unavailable,
  say so honestly in a later `stage_complete.summary` rather than pretending it worked.
- Never fabricate a tool result yourself — only react to results actually returned
  to you in the transcript below.
"#;

fn read_asset_text(rel_path: &str) -> MontageResult<String> {
    let path = crate::assets_root().join(rel_path);
    std::fs::read_to_string(&path)
        .map_err(|e| crate::error::MontageError::msg(format!("reading {}: {e}", path.display())))
}

fn skill_text(skill_ref: &str) -> MontageResult<String> {
    read_asset_text(&format!("skills/{skill_ref}.md"))
}

/// Meta/core/creative skills injected on top of the pipeline-specific EP and
/// stage director skills. `reviewer` and `checkpoint-protocol` are universal
/// mechanism guidance; the rest are selected by stage *shape* (what kind of
/// work this stage name typically does) rather than by pipeline, so the same
/// small skill library serves every pipeline without duplication.
fn shared_skills_for_stage(stage_name: &str) -> Vec<&'static str> {
    let mut refs = vec!["meta/reviewer", "meta/checkpoint-protocol"];
    match stage_name {
        "proposal" | "script" => {
            refs.push("creative/storytelling");
        }
        "scene_plan" | "capture_plan" => {
            refs.push("creative/storytelling");
        }
        "assets" | "avatar_render" | "talking_head_render" => {
            refs.push("creative/video-gen-prompting");
            refs.push("creative/sound-design");
        }
        "edit" => {
            refs.push("creative/video-editing");
            refs.push("core/ffmpeg");
            refs.push("meta/animation-runtime-selector");
        }
        "compose" => {
            refs.push("core/ffmpeg");
            refs.push("core/compose-runtimes");
            refs.push("meta/animation-runtime-selector");
        }
        _ => {}
    }
    refs
}

/// Full system prompt for one stage: contract + EP skill + stage director +
/// shared meta/core/creative skills + Flowy capability overlay + tool catalog
/// + response protocol.
pub fn build_stage_system_prompt(
    manifest: &PipelineManifest,
    stage: &StageSpec,
    tools: &[ToolSpec],
) -> MontageResult<String> {
    let contract = read_asset_text("CONTRACT.md")?;
    let ep_skill = skill_text(&manifest.orchestration.skill)?;
    let stage_skill = skill_text(&stage.skill)?;
    let overlay = skill_text("meta/flowy-capability-overlay").unwrap_or_default();

    let mut shared_skills = String::new();
    for skill_ref in shared_skills_for_stage(&stage.name) {
        if let Ok(text) = skill_text(skill_ref) {
            shared_skills.push_str(&format!("\n## {skill_ref}\n{text}\n"));
        }
    }

    let mut tool_catalog = String::new();
    for spec in tools {
        tool_catalog.push_str(&format!(
            "\n### {}\n- capability: {}\n- runtime: {:?}\n- stability: {:?}\n- input_schema: {}\n",
            spec.name,
            spec.capability,
            spec.runtime,
            spec.stability,
            serde_json::to_string(&spec.input_schema).unwrap_or_default()
        ));
    }

    let review_focus = if stage.review_focus.is_empty() {
        String::from("(none declared)")
    } else {
        stage.review_focus.join("; ")
    };
    let success_criteria = if stage.success_criteria.is_empty() {
        String::from("(none declared)")
    } else {
        stage
            .success_criteria
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(format!(
        "# Product Contract\n{contract}\n\n\
         # Executive Producer Skill ({ep_skill_path})\n{ep_skill}\n\n\
         # Stage Director: {stage_name} ({stage_skill_path})\n{stage_skill}\n\n\
         # Shared Skills\n{shared_skills}\n\n\
         # Flowy Capability Overlay\n{overlay}\n\n\
         # This stage's contract\n\
         Pipeline: {pipeline} (v{pipeline_version})\n\
         Stage: {stage_name}\n\
         Produces: {produces}\n\
         Required artifacts in: {required_in}\n\
         Review focus: {review_focus}\n\
         Success criteria:\n{success_criteria}\n\n\
         # Tools available this stage\n{tool_catalog}\n\
         {protocol}",
        ep_skill_path = manifest.orchestration.skill,
        stage_name = stage.name,
        stage_skill_path = stage.skill,
        pipeline = manifest.name,
        pipeline_version = manifest.version,
        produces = if stage.produces.is_empty() {
            "(none)".to_string()
        } else {
            stage.produces.join(", ")
        },
        required_in = if stage.required_artifacts_in.is_empty() {
            "(none)".to_string()
        } else {
            stage.required_artifacts_in.join(", ")
        },
        protocol = PROTOCOL_INSTRUCTIONS,
    ))
}

/// Per-turn user message: the stage brief once, then the accumulated
/// transcript of tool calls/results, ending with a prompt for the next reply.
pub fn build_turn_user_message(stage_brief: &str, transcript: &str, turn: u32, max_turns: u32) -> String {
    if transcript.is_empty() {
        format!(
            "{stage_brief}\n\n(This is turn 1/{max_turns}. Respond with your first JSON action.)"
        )
    } else {
        format!(
            "{stage_brief}\n\n=== Conversation so far ===\n{transcript}\n\n\
             (This is turn {turn}/{max_turns}. Respond with exactly one JSON action.)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_message_without_transcript_is_first_turn() {
        let msg = build_turn_user_message("Do the thing.", "", 1, 24);
        assert!(msg.contains("turn 1/24"));
        assert!(!msg.contains("Conversation so far"));
    }

    #[test]
    fn turn_message_with_transcript_includes_history() {
        let msg = build_turn_user_message("Do the thing.", "assistant: {...}", 2, 24);
        assert!(msg.contains("Conversation so far"));
        assert!(msg.contains("turn 2/24"));
    }
}

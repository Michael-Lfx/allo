//! Prompt preview for plan-before-run confirmation (no generation API calls).

use nomi_config::MediaGenConfig;
use serde_json::{Value, json};

use crate::backends::FlowyMediaServices;
use crate::llm_refine::refine_with_llm_or_template;
use crate::prompt_refine::{RefineInput, refine_prompt};
use crate::workflows::templates::builtin_template;

pub async fn build_prompt_preview(
    template_id: &str,
    objective: &str,
    inputs: &Value,
    services: Option<&FlowyMediaServices>,
    media_config: &MediaGenConfig,
) -> Value {
    let def = match builtin_template(template_id) {
        Some(d) => d,
        None => return json!({ "error": format!("unknown template {template_id}") }),
    };

    let aspect_ratio = inputs
        .get("aspect_ratio")
        .and_then(|v| v.as_str())
        .unwrap_or("16:9");

    let mut previews = Vec::new();
    for step in &def.steps {
        if step.kind != "prompt_refine" {
            continue;
        }
        let medium = step
            .input
            .get("medium")
            .and_then(|v| v.as_str())
            .unwrap_or("image");
        let has_reference = inputs
            .get("image_url")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
            || medium == "edit"
            || medium == "motion";

        let refined = if media_config.workflows.llm_prompt_refine
            && let Some(svc) = services
        {
            refine_with_llm_or_template(
                svc,
                &RefineInput {
                    prompt: objective,
                    medium,
                    aspect_ratio: Some(aspect_ratio),
                    has_reference_image: has_reference,
                },
            )
            .await
        } else {
            refine_prompt(&RefineInput {
                prompt: objective,
                medium,
                aspect_ratio: Some(aspect_ratio),
                has_reference_image: has_reference,
            })
        };

        previews.push(json!({
            "step_id": step.id,
            "medium": medium,
            "image_prompt": refined.image_prompt,
            "video_prompt": refined.video_prompt,
            "motion_prompt": refined.motion_prompt,
            "negative_prompt": refined.negative_prompt,
            "output": refined.output,
        }));
    }

    let user_block = previews
        .iter()
        .filter_map(|p| p.get("output").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("\n\n");

    json!({
        "refined_prompt_preview": previews,
        "user_prompt_block": if user_block.is_empty() {
            Value::Null
        } else {
            json!(user_block)
        },
        "confirm_hint": "Show user_prompt_block to the user and ask for confirmation before media_workflow_run."
    })
}

//! Optional LLM-based prompt refinement via Flowy `/v1/chat/completions`.

use nomi_types::ToolError;
use serde::Deserialize;

use crate::backends::FlowyMediaServices;
use crate::progress::{report_media_progress, storyboard_planning};
use crate::prompt_refine::{RefineInput, RefineResult, refine_prompt};

const REFINE_SYSTEM: &str = r#"You are an expert prompt engineer for AI image and video models.
Return ONLY valid JSON (no markdown fences) with rich visual detail.
For images: concrete subject, materials/textures, lighting, composition, mood.
For videos: separate detailed scene from camera/subject motion.
For image-to-video motion-only mode: describe motion and changes, not static appearance.

JSON schema:
{
  "image_prompt": "string (detailed still image prompt)",
  "video_prompt": "string (scene + motion combined for text-to-video)",
  "motion_prompt": "string (motion/camera for image-to-video)",
  "negative_prompt": "string",
  "output": "string (primary prompt for the requested medium)"
}"#;

const STORYBOARD_SYSTEM: &str = r#"You are a storyboard director for AI video generation.
Return ONLY valid JSON (no markdown) planning 2-3 cinematic shots with rich visual detail per shot.

{
  "negative_prompt": "string",
  "shots": [
    {
      "scene_prompt": "detailed still frame description",
      "motion_prompt": "camera and subject motion for this shot",
      "duration_secs": 3
    }
  ]
}"#;

#[derive(Debug, Deserialize)]
struct LlmRefineJson {
    #[serde(default)]
    image_prompt: String,
    #[serde(default)]
    video_prompt: String,
    #[serde(default)]
    motion_prompt: String,
    #[serde(default)]
    negative_prompt: String,
    #[serde(default)]
    output: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StoryboardShot {
    pub scene_prompt: String,
    pub motion_prompt: String,
    #[serde(default = "default_shot_duration")]
    pub duration_secs: u32,
}

fn default_shot_duration() -> u32 {
    3
}

#[derive(Debug, Deserialize)]
pub struct StoryboardPlan {
    #[serde(default)]
    pub negative_prompt: String,
    #[serde(default)]
    pub shots: Vec<StoryboardShot>,
}

/// Refine using LLM when enabled, otherwise local templates.
pub async fn refine_with_llm_or_template(
    services: &FlowyMediaServices,
    input: &RefineInput<'_>,
) -> RefineResult {
    if services.media.workflows.llm_prompt_refine
        && let Some(result) = try_llm_refine(services, input).await
    {
        return result;
    }
    refine_prompt(input)
}

/// Plan storyboard shots via LLM or heuristic fallback.
pub async fn plan_storyboard(
    services: &FlowyMediaServices,
    objective: &str,
    max_shots: u32,
) -> StoryboardPlan {
    if services.media.workflows.llm_prompt_refine
        && let Some(plan) = try_llm_storyboard(services, objective, max_shots).await
    {
        return plan;
    }
    heuristic_storyboard(objective, max_shots)
}

async fn try_llm_refine(
    services: &FlowyMediaServices,
    input: &RefineInput<'_>,
) -> Option<RefineResult> {
    report_media_progress("正在连接 AI 服务优化提示词…");
    let user = format!(
        "Medium: {}\nAspect ratio: {}\nHas reference image: {}\nUser objective:\n{}",
        input.medium,
        input.aspect_ratio.unwrap_or("16:9"),
        input.has_reference_image,
        input.prompt
    );
    let text = llm_json_call(services, REFINE_SYSTEM, &user).await.ok()?;
    let parsed: LlmRefineJson = serde_json::from_str(&text).ok()?;
    Some(llm_json_to_refine(parsed, input.medium))
}

async fn try_llm_storyboard(
    services: &FlowyMediaServices,
    objective: &str,
    max_shots: u32,
) -> Option<StoryboardPlan> {
    report_media_progress(storyboard_planning());
    let user = format!(
        "Plan up to {max_shots} shots for:\n{objective}\nEach shot needs rich visual scene_prompt and motion_prompt."
    );
    let text = llm_json_call(services, STORYBOARD_SYSTEM, &user)
        .await
        .ok()?;
    let mut plan: StoryboardPlan = serde_json::from_str(&text).ok()?;
    plan.shots.truncate(max_shots as usize);
    if plan.shots.is_empty() {
        return None;
    }
    Some(plan)
}

async fn llm_json_call(
    services: &FlowyMediaServices,
    system: &str,
    user: &str,
) -> Result<String, ToolError> {
    let content = services
        .api
        .chat_completions_text(&services.session, system, user, 2048, 0.4, None)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    Ok(extract_json_object(&content))
}

fn extract_json_object(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find('{')
        && let Some(end) = trimmed.rfind('}')
    {
        return trimmed[start..=end].to_string();
    }
    trimmed.to_string()
}

fn llm_json_to_refine(parsed: LlmRefineJson, medium: &str) -> RefineResult {
    let fallback = refine_prompt(&RefineInput {
        prompt: &parsed.output,
        medium,
        aspect_ratio: None,
        has_reference_image: false,
    });
    let output = if parsed.output.trim().is_empty() {
        match medium {
            "motion" | "video_motion" => parsed.motion_prompt.clone(),
            "video" => parsed.video_prompt.clone(),
            _ => parsed.image_prompt.clone(),
        }
    } else {
        parsed.output.clone()
    };
    RefineResult {
        output: if output.trim().is_empty() {
            fallback.output
        } else {
            output
        },
        image_prompt: if parsed.image_prompt.trim().is_empty() {
            fallback.image_prompt
        } else {
            parsed.image_prompt
        },
        video_prompt: if parsed.video_prompt.trim().is_empty() {
            fallback.video_prompt
        } else {
            parsed.video_prompt
        },
        motion_prompt: if parsed.motion_prompt.trim().is_empty() {
            fallback.motion_prompt
        } else {
            parsed.motion_prompt
        },
        negative_prompt: if parsed.negative_prompt.trim().is_empty() {
            fallback.negative_prompt
        } else {
            parsed.negative_prompt
        },
    }
}

fn heuristic_storyboard(objective: &str, max_shots: u32) -> StoryboardPlan {
    let shots = (1..=max_shots.max(1))
        .map(|i| StoryboardShot {
            scene_prompt: format!("Shot {i}: {objective}"),
            motion_prompt: "Slow cinematic push-in".into(),
            duration_secs: 3,
        })
        .collect();
    StoryboardPlan {
        negative_prompt: String::new(),
        shots,
    }
}

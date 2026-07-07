//! Built-in workflow templates.

use nomi_config::MediaWorkflowTemplateMap;
use serde_json::{Value, json};

use super::definition::WorkflowDefinition;

const SIMPLE_TXT2IMG: &str = r#"
id: simple_txt2img
version: 1
description: Single-step text-to-image (legacy, no refinement)
inputs:
  prompt: { type: string, required: true }
  model: { type: string, required: false }
steps:
  - id: generate
    kind: image_generate
    input:
      prompt: "$inputs.prompt"
      model: "$inputs.model"
"#;

const TXT2IMG: &str = r#"
id: txt2img
version: 1
description: Refine prompt then text-to-image with rich visual detail
inputs:
  prompt: { type: string, required: true }
  model: { type: string, required: false }
  aspect_ratio: { type: string, default: "16:9" }
steps:
  - id: refine_prompt
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: image
      aspect_ratio: "$inputs.aspect_ratio"
  - id: generate
    kind: image_generate
    depends_on: [refine_prompt]
    input:
      prompt: "$steps.refine_prompt.output"
      model: "$inputs.model"
  - id: qa
    kind: qa_check
    depends_on: [generate]
    on_fail:
      retry_from: refine_prompt
    input:
      kind: image
      target_step: generate
      step_output: "$steps.generate"
"#;

const IMG2IMG: &str = r#"
id: img2img
version: 1
description: Refine edit prompt then image-to-image from user reference
inputs:
  prompt: { type: string, required: true }
  image_url: { type: string, required: true }
  model: { type: string, required: false }
  aspect_ratio: { type: string, default: "16:9" }
steps:
  - id: refine_edit
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: edit
      aspect_ratio: "$inputs.aspect_ratio"
      has_reference_image: true
  - id: generate
    kind: image_generate
    depends_on: [refine_edit]
    input:
      prompt: "$steps.refine_edit.output"
      image_url: "$inputs.image_url"
      model: "$inputs.model"
  - id: qa
    kind: qa_check
    depends_on: [generate]
    on_fail:
      retry_from: refine_edit
    input:
      kind: image
      target_step: generate
      step_output: "$steps.generate"
"#;

const IMAGE_VARIATION: &str = r#"
id: image_variation
version: 1
description: Generate alternate image variations from a prompt (new seed)
inputs:
  prompt: { type: string, required: true }
  model: { type: string, required: false }
  aspect_ratio: { type: string, default: "16:9" }
steps:
  - id: refine_prompt
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: image
      aspect_ratio: "$inputs.aspect_ratio"
  - id: generate
    kind: image_generate
    depends_on: [refine_prompt]
    input:
      prompt: "$steps.refine_prompt.output"
      model: "$inputs.model"
      extra: { variation: true }
"#;

const IMAGE_UPSCALE: &str = r#"
id: image_upscale
version: 1
description: Enhance/upscale an image via img2img with detail-focused prompt
inputs:
  prompt: { type: string, required: true }
  image_url: { type: string, required: true }
  model: { type: string, required: false }
steps:
  - id: refine_edit
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: edit
      has_reference_image: true
  - id: generate
    kind: image_generate
    depends_on: [refine_edit]
    input:
      prompt: "$steps.refine_edit.output"
      image_url: "$inputs.image_url"
      model: "$inputs.model"
      extra: { upscale: true }
"#;

const VIDEO_EXTEND: &str = r#"
id: video_extend
version: 1
description: Extend a clip from its last frame with new motion
inputs:
  prompt: { type: string, required: true }
  last_frame_url: { type: string, required: true }
  duration: { type: integer, default: 5 }
  aspect_ratio: { type: string, default: "16:9" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_motion
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: motion
      aspect_ratio: "$inputs.aspect_ratio"
      has_reference_image: true
  - id: video
    kind: video_generate
    depends_on: [refine_motion]
    input:
      prompt: "$steps.refine_motion.motion_prompt"
      negative_prompt: "$steps.refine_motion.negative_prompt"
      image_url: "$inputs.last_frame_url"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const PROMPT_REFINE_TXT2VIDEO: &str = r#"
id: prompt_refine_txt2video
version: 1
description: Refine scene+motion prompt then text-to-video
inputs:
  prompt: { type: string, required: true }
  duration: { type: integer, default: 5 }
  aspect_ratio: { type: string, default: "16:9" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_prompt
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: video
      aspect_ratio: "$inputs.aspect_ratio"
  - id: video
    kind: video_generate
    depends_on: [refine_prompt]
    input:
      prompt: "$steps.refine_prompt.video_prompt"
      negative_prompt: "$steps.refine_prompt.negative_prompt"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const IMG2VIDEO_DIRECT: &str = r#"
id: img2video_direct
version: 1
description: Motion prompt refinement then image-to-video from user image
inputs:
  prompt: { type: string, required: true }
  image_url: { type: string, required: true }
  duration: { type: integer, default: 5 }
  aspect_ratio: { type: string, default: "9:16" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_motion
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: motion
      aspect_ratio: "$inputs.aspect_ratio"
      has_reference_image: true
  - id: video
    kind: video_generate
    depends_on: [refine_motion]
    input:
      prompt: "$steps.refine_motion.motion_prompt"
      negative_prompt: "$steps.refine_motion.negative_prompt"
      image_url: "$inputs.image_url"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const LONG_TXT2VIDEO: &str = r#"
id: long_txt2video
version: 1
description: Long text-to-video — split into Seedance clips, chain last-frame, concat
inputs:
  prompt: { type: string, required: true }
  duration: { type: integer, default: 20 }
  aspect_ratio: { type: string, default: "16:9" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_prompt
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: video
      aspect_ratio: "$inputs.aspect_ratio"
  - id: generate
    kind: video_long_generate
    depends_on: [refine_prompt]
    input:
      prompt: "$steps.refine_prompt.video_prompt"
      negative_prompt: "$steps.refine_prompt.negative_prompt"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const LONG_IMG2VIDEO_DIRECT: &str = r#"
id: long_img2video_direct
version: 1
description: Long image-to-video — chained clips from user reference image
inputs:
  prompt: { type: string, required: true }
  image_url: { type: string, required: true }
  duration: { type: integer, default: 20 }
  aspect_ratio: { type: string, default: "9:16" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_motion
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: motion
      aspect_ratio: "$inputs.aspect_ratio"
      has_reference_image: true
  - id: generate
    kind: video_long_generate
    depends_on: [refine_motion]
    input:
      prompt: "$steps.refine_motion.motion_prompt"
      negative_prompt: "$steps.refine_motion.negative_prompt"
      image_url: "$inputs.image_url"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const LONG_IMG2VIDEO: &str = r#"
id: long_img2video
version: 1
description: Long video with keyframe - chained Seedance clips from generated first frame
inputs:
  prompt: { type: string, required: true }
  duration: { type: integer, default: 20 }
  aspect_ratio: { type: string, default: "16:9" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_scene
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: image
      aspect_ratio: "$inputs.aspect_ratio"
  - id: keyframe
    kind: image_generate
    depends_on: [refine_scene]
    input:
      prompt: "$steps.refine_scene.image_prompt"
  - id: refine_motion
    kind: prompt_refine
    depends_on: [keyframe]
    input:
      prompt: "$inputs.prompt"
      medium: motion
      aspect_ratio: "$inputs.aspect_ratio"
      has_reference_image: true
  - id: generate
    kind: video_long_generate
    depends_on: [refine_motion]
    input:
      prompt: "$steps.refine_motion.motion_prompt"
      negative_prompt: "$steps.refine_motion.negative_prompt"
      image_url: "$steps.keyframe.best_url"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const IMG2VIDEO: &str = r#"
id: img2video
version: 1
description: Generate detailed keyframe image then image-to-video
inputs:
  prompt: { type: string, required: true }
  duration: { type: integer, default: 5 }
  aspect_ratio: { type: string, default: "16:9" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_scene
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: image
      aspect_ratio: "$inputs.aspect_ratio"
  - id: keyframe
    kind: image_generate
    depends_on: [refine_scene]
    input:
      prompt: "$steps.refine_scene.image_prompt"
  - id: refine_motion
    kind: prompt_refine
    depends_on: [keyframe]
    input:
      prompt: "$inputs.prompt"
      medium: motion
      aspect_ratio: "$inputs.aspect_ratio"
      has_reference_image: true
  - id: video
    kind: video_generate
    depends_on: [refine_motion]
    input:
      prompt: "$steps.refine_motion.motion_prompt"
      negative_prompt: "$steps.refine_motion.negative_prompt"
      image_url: "$steps.keyframe.best_url"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const STORYBOARD_VIDEO: &str = r#"
id: storyboard_to_video
version: 1
description: Refine narrative prompt, keyframe, then cinematic image-to-video
inputs:
  prompt: { type: string, required: true }
  duration: { type: integer, default: 8 }
  aspect_ratio: { type: string, default: "16:9" }
  resolution: { type: string, default: "720p" }
steps:
  - id: refine_prompt
    kind: prompt_refine
    input:
      prompt: "$inputs.prompt"
      medium: video
      aspect_ratio: "$inputs.aspect_ratio"
  - id: keyframe
    kind: image_generate
    depends_on: [refine_prompt]
    input:
      prompt: "$steps.refine_prompt.image_prompt"
  - id: refine_motion
    kind: prompt_refine
    depends_on: [keyframe]
    input:
      prompt: "$inputs.prompt"
      medium: motion
      aspect_ratio: "$inputs.aspect_ratio"
      has_reference_image: true
  - id: video
    kind: video_generate
    depends_on: [refine_motion]
    input:
      prompt: "$steps.refine_motion.motion_prompt"
      negative_prompt: "$steps.refine_prompt.negative_prompt"
      image_url: "$steps.keyframe.best_url"
      duration: "$inputs.duration"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
"#;

const STORYBOARD_MULTI: &str = r#"
id: storyboard_multi
version: 1
description: Multi-shot storyboard — LLM-planned scenes with per-shot image and video
inputs:
  prompt: { type: string, required: true }
  aspect_ratio: { type: string, default: "16:9" }
  resolution: { type: string, default: "720p" }
  max_shots: { type: integer, default: 3 }
steps:
  - id: storyboard
    kind: storyboard_multi
    input:
      prompt: "$inputs.prompt"
      aspect_ratio: "$inputs.aspect_ratio"
      resolution: "$inputs.resolution"
      max_shots: "$inputs.max_shots"
"#;

pub fn list_builtin_templates() -> Vec<&'static str> {
    vec![
        "simple_txt2img",
        "txt2img",
        "img2img",
        "prompt_refine_txt2video",
        "long_txt2video",
        "img2video_direct",
        "long_img2video_direct",
        "img2video",
        "long_img2video",
        "storyboard_to_video",
        "storyboard_multi",
        "image_variation",
        "image_upscale",
        "video_extend",
    ]
}

pub fn builtin_template(id: &str) -> Option<WorkflowDefinition> {
    let yaml = match id {
        "simple_txt2img" => SIMPLE_TXT2IMG,
        "txt2img" => TXT2IMG,
        "img2img" => IMG2IMG,
        "prompt_refine_txt2video" => PROMPT_REFINE_TXT2VIDEO,
        "long_txt2video" => LONG_TXT2VIDEO,
        "img2video_direct" => IMG2VIDEO_DIRECT,
        "long_img2video_direct" => LONG_IMG2VIDEO_DIRECT,
        "img2video" => IMG2VIDEO,
        "long_img2video" => LONG_IMG2VIDEO,
        "storyboard_to_video" => STORYBOARD_VIDEO,
        "storyboard_multi" => STORYBOARD_MULTI,
        "image_variation" => IMAGE_VARIATION,
        "image_upscale" => IMAGE_UPSCALE,
        "video_extend" => VIDEO_EXTEND,
        _ => return None,
    };
    serde_yaml::from_str(yaml).ok()
}

fn is_img2img_intent(objective: &str) -> bool {
    let lower = objective.to_ascii_lowercase();
    [
        "改",
        "修",
        "风格",
        "背景",
        "替换",
        "编辑",
        "img2img",
        "image to image",
        "edit",
        "modify",
        "style transfer",
        "inpaint",
        "换背景",
        "风格化",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
}

fn is_video_motion_intent(objective: &str) -> bool {
    let lower = objective.to_ascii_lowercase();
    lower.contains("视频")
        || lower.contains("video")
        || lower.contains("动")
        || lower.contains("animate")
        || lower.contains("motion")
        || lower.contains("图生视频")
        || lower.contains("image to video")
}

/// Pick a builtin template from user intent, honoring configured defaults.
pub fn suggest_template_id(
    objective: &str,
    has_image_input: bool,
    defaults: &MediaWorkflowTemplateMap,
) -> String {
    let lower = objective.to_ascii_lowercase();
    if has_image_input {
        if is_img2img_intent(objective) && !is_video_motion_intent(objective) {
            return resolve_template_default("img2img", defaults, "img2img");
        }
        if is_video_motion_intent(objective) || lower.contains("图生视频") {
            let base = resolve_template_default("img2video", defaults, "img2video_direct");
            if let Some(dur) = crate::video_segment::parse_duration_secs_from_text(objective) {
                return crate::video_segment::route_long_video_template(&base, dur, "seedance");
            }
            return base;
        }
        return resolve_template_default("img2img", defaults, "img2img");
    }
    if lower.contains("分镜")
        || lower.contains("storyboard")
        || lower.contains("叙事")
        || lower.contains("多个场景")
        || lower.contains("很多场景")
        || lower.contains("多场景")
        || lower.contains("多镜头")
        || (lower.contains("场景") && lower.contains("丰富"))
    {
        return resolve_template_default("storyboard", defaults, "storyboard_multi");
    }
    if lower.contains("视频") || lower.contains("video") {
        let base = resolve_template_default("txt2video", defaults, "prompt_refine_txt2video");
        if let Some(dur) = crate::video_segment::parse_duration_secs_from_text(objective) {
            return crate::video_segment::route_long_video_template(&base, dur, "seedance");
        }
        return base;
    }
    resolve_template_default("txt2img", defaults, "txt2img")
}

fn resolve_template_default(
    kind: &str,
    defaults: &MediaWorkflowTemplateMap,
    fallback: &str,
) -> String {
    let configured = match kind {
        "txt2img" => defaults.txt2img.trim(),
        "txt2video" => defaults.txt2video.trim(),
        "img2img" => defaults.img2img.trim(),
        "img2video" => defaults.img2video.trim(),
        "storyboard" => defaults.storyboard.trim(),
        _ => "",
    };
    if configured.is_empty() {
        fallback.to_string()
    } else {
        configured.to_string()
    }
}

pub fn default_template_inputs(template_id: &str, prompt: &str, platform: Option<&str>) -> Value {
    let aspect = crate::platform::default_aspect_for_platform(platform);
    match template_id {
        "simple_txt2img" | "txt2img" | "img2img" | "image_variation" => json!({
            "prompt": prompt,
            "aspect_ratio": aspect
        }),
        "img2video_direct" => json!({
            "prompt": prompt,
            "duration": 5,
            "aspect_ratio": aspect,
            "resolution": "720p"
        }),
        "image_upscale" => json!({
            "prompt": prompt,
            "aspect_ratio": aspect
        }),
        "video_extend" => json!({
            "prompt": prompt,
            "duration": 5,
            "aspect_ratio": aspect,
            "resolution": "720p"
        }),
        "long_txt2video" | "long_img2video" | "long_img2video_direct" => json!({
            "prompt": prompt,
            "duration": 20,
            "aspect_ratio": aspect,
            "resolution": "720p"
        }),
        _ => json!({
            "prompt": prompt,
            "duration": 5,
            "aspect_ratio": aspect,
            "resolution": "720p"
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtin_templates_parse() {
        for id in list_builtin_templates() {
            let def = builtin_template(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(def.id, id);
            assert!(!def.steps.is_empty());
        }
    }

    #[test]
    fn suggest_template_video() {
        let defaults = MediaWorkflowTemplateMap::default();
        assert_eq!(
            suggest_template_id("generate a short product video", false, &defaults),
            "prompt_refine_txt2video"
        );
    }

    #[test]
    fn suggest_template_uses_config_default() {
        let mut defaults = MediaWorkflowTemplateMap::default();
        defaults.txt2img = "simple_txt2img".into();
        assert_eq!(
            suggest_template_id("draw a cat", false, &defaults),
            "simple_txt2img"
        );
    }

    #[test]
    fn suggest_storyboard_picks_multi() {
        let defaults = MediaWorkflowTemplateMap::default();
        assert_eq!(
            suggest_template_id("做一个分镜叙事短片", false, &defaults),
            "storyboard_multi"
        );
        assert_eq!(
            suggest_template_id("老北京橘猫日常，很多场景和分镜", false, &defaults),
            "storyboard_multi"
        );
    }

    #[test]
    fn suggest_img2img_for_edit_intent() {
        let defaults = MediaWorkflowTemplateMap::default();
        assert_eq!(
            suggest_template_id("把背景换成海边", true, &defaults),
            "img2img"
        );
    }

    #[test]
    fn suggest_long_video_from_duration_in_objective() {
        let defaults = MediaWorkflowTemplateMap::default();
        let id = suggest_template_id("生成一段约20秒的产品宣传视频", false, &defaults);
        assert_eq!(id, "long_txt2video");
    }

    #[test]
    fn route_long_img2video_for_keyframe_workflow() {
        let routed = crate::video_segment::route_long_video_template("img2video", 25, "seedance");
        assert_eq!(routed, "long_img2video");
    }
}

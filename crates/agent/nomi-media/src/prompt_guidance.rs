//! Agent-facing guidance for Flowy media tools.
//!
//! Native Session does **not** inject [`gateway_media_system_hint`] into the
//! system prefix. `image_generate` (and workflow tools, when registered) carry
//! WHEN/HOW in their schemas. This helper remains for hosts that still need a
//! short routing sentence, and it must only name tools that are advertised.

/// Optional system hint. `has_workflow_tools` must match tools actually
/// advertised this session — never pass `true` when only `image_generate` is
/// registered.
pub fn gateway_media_system_hint(has_workflow_tools: bool) -> String {
    let mut lines = vec![
        "[SYSTEM] You can generate images with `image_generate`.".to_string(),
        "Write a rich scene prompt: subject, materials/textures, lighting, composition, mood — not a one-liner.".to_string(),
        "Deliver files via MEDIA:/local_path from tool results. Include `user_prompt_block` in your reply.".to_string(),
    ];
    if has_workflow_tools {
        lines.extend([
            "Videos: separate SCENE (visuals) from MOTION (camera + subject movement).".to_string(),
            "Video or multi-step pipelines: `media_workflow_plan` then `media_workflow_run` (do not poll `media_workflow_status` in a loop).".to_string(),
            "Image-to-video: describe motion and changes only. Long video (>10s) uses `video_generate` duration or a workflow plan.".to_string(),
        ]);
    }
    lines.join("\n")
}

/// Short prompt field description for tool JSON schemas.
pub const IMAGE_PROMPT_SCHEMA_DESC: &str = "Detailed image description: subject, style/medium, composition, lighting, textures/materials, mood. Include concrete visual specifics — not a one-line summary. Front-load the subject; specify lighting (golden hour, softbox, overcast). Quote any on-image text exactly.";

/// Short prompt field description for video tool schema.
pub const VIDEO_PROMPT_SCHEMA_DESC: &str = "Video prompt: (1) rich scene/visual detail (2) camera motion e.g. dolly in, pan, orbit (3) subject movement. For image-to-video, focus on motion/changes only.";

pub const VIDEO_NEGATIVE_SCHEMA_DESC: &str =
    "Optional negative prompt (e.g. blurry, watermark, jitter, distorted, subtitles).";

pub const VIDEO_SEED_SCHEMA_DESC: &str =
    "Optional seed for reproducibility when the model supports it.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_only_hint_does_not_name_unregistered_workflow_tools() {
        let hint = gateway_media_system_hint(false);
        assert!(hint.contains("image_generate"));
        assert!(!hint.contains("media_workflow_plan"));
        assert!(!hint.contains("media_workflow_run"));
        assert!(!hint.contains("video_generate"));
        assert!(!hint.contains("media_workflow_status"));
    }

    #[test]
    fn workflow_hint_names_plan_and_run() {
        let hint = gateway_media_system_hint(true);
        assert!(hint.contains("media_workflow_plan"));
        assert!(hint.contains("media_workflow_run"));
        assert!(hint.contains("video_generate"));
    }
}

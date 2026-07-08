//! Platform-aware defaults for media workflows.

/// Default aspect ratio for a chat/gateway platform.
pub fn default_aspect_for_platform(platform: Option<&str>) -> &'static str {
    match platform.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) if is_mobile_vertical_platform(p) => "9:16",
        _ => "16:9",
    }
}

fn is_mobile_vertical_platform(platform: &str) -> bool {
    let lower = platform.to_ascii_lowercase();
    lower.contains("wecom")
        || lower.contains("weixin")
        || lower.contains("wechat")
        || lower.contains("whatsapp")
        || lower == "telegram"
}

/// Explain why a workflow template was auto-selected.
pub fn routing_rationale(template_id: &str, objective: &str, has_image: bool) -> String {
    let lower = objective.to_ascii_lowercase();
    match template_id {
        "img2img" => {
            "User supplied a reference image with edit/style intent — img2img refines an edit prompt then applies image-to-image.".into()
        }
        "img2video_direct" if has_image => {
            "User supplied a reference image for motion — img2video_direct refines motion only then animates the provided frame.".into()
        }
        "img2video" => {
            "Text-only brief for video — img2video generates a keyframe first, then image-to-video for richer scene control.".into()
        }
        "storyboard_multi" => {
            "Multi-scene / storyboard intent detected — storyboard_multi plans shots then generates each keyframe + clip.".into()
        }
        "prompt_refine_txt2video" => {
            "Video intent without reference image — prompt_refine_txt2video separates scene detail from motion.".into()
        }
        "long_txt2video" => {
            "Target duration exceeds Seedance single-clip limit (~10s) — long_txt2video splits into chained segments and concat with ffmpeg.".into()
        }
        "long_img2video_direct" => {
            "Long image-to-video — multiple Seedance clips chained via last-frame → first-frame from the user's reference.".into()
        }
        "long_img2video" => {
            "Long video with keyframe — generates a keyframe then chained img2video segments for scene continuity.".into()
        }
        "txt2img" => "Image intent — txt2img refines visual detail then generates with QA.".into(),
        "image_variation" => "Variation request — reuses prompt with a new seed for alternate takes.".into(),
        "image_upscale" => "Upscale/enhance request — img2img-style pass targeting higher detail.".into(),
        "video_extend" => "Extend request — continues from last frame of prior clip.".into(),
        _ if lower.contains("video") => format!(
            "Selected '{template_id}' for video-related objective."
        ),
        _ => format!("Selected '{template_id}' as the default match for this objective."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wecom_prefers_vertical() {
        assert_eq!(default_aspect_for_platform(Some("wecom")), "9:16");
        assert_eq!(default_aspect_for_platform(Some("discord")), "16:9");
    }
}

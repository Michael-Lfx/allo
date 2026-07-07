//! Structured prompt refinement for image/video workflows (no extra LLM round-trip).

/// Inputs for local prompt refinement.
#[derive(Debug, Clone)]
pub struct RefineInput<'a> {
    pub prompt: &'a str,
    /// `image`, `video`, `motion` (image-to-video motion-only), or `edit` (img2img).
    pub medium: &'a str,
    pub aspect_ratio: Option<&'a str>,
    pub has_reference_image: bool,
}

/// Refined prompts for downstream generation steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefineResult {
    /// Primary string consumed by workflow `$steps.*.output` references.
    pub output: String,
    pub image_prompt: String,
    pub video_prompt: String,
    pub motion_prompt: String,
    pub negative_prompt: String,
}

/// Refine a user objective into model-ready prompts with rich visual detail.
pub fn refine_prompt(input: &RefineInput<'_>) -> RefineResult {
    let base = input.prompt.trim();
    let chinese = contains_cjk(base);

    match input.medium {
        "motion" | "video_motion" => refine_motion_only(base, chinese),
        "video" => refine_video(base, chinese, input.has_reference_image, input.aspect_ratio),
        "edit" => refine_edit(base, chinese, input.aspect_ratio),
        _ => refine_image(base, chinese, input.aspect_ratio),
    }
}

fn refine_edit(prompt: &str, chinese: bool, aspect_ratio: Option<&str>) -> RefineResult {
    let composition = composition_hint(aspect_ratio, chinese);
    let enriched = enrich_scene_detail(prompt, chinese);
    let edit_prompt = if chinese {
        format!(
            "{enriched}，{composition}，保留参考图主体结构与构图，仅按描述修改指定区域或整体风格，细节清晰，无变形"
        )
    } else {
        format!(
            "{enriched}, {composition}, preserve subject structure from reference while applying the described edit only, sharp detail, no distortion"
        )
    };
    let negative = if chinese {
        "模糊，低清晰度，变形，水印，文字，噪点，主体漂移，构图大幅改变".to_string()
    } else {
        "blurry, low resolution, distorted, watermark, text overlay, subject drift, major composition change".to_string()
    };
    RefineResult {
        output: edit_prompt.clone(),
        image_prompt: edit_prompt,
        video_prompt: String::new(),
        motion_prompt: String::new(),
        negative_prompt: negative,
    }
}

fn refine_image(prompt: &str, chinese: bool, aspect_ratio: Option<&str>) -> RefineResult {
    let composition = composition_hint(aspect_ratio, chinese);
    let enriched = enrich_scene_detail(prompt, chinese);
    let image_prompt = if chinese {
        format!(
            "{enriched}，{composition}，精细材质与表面纹理，层次分明的光影，锐利对焦，高细节，专业摄影质感"
        )
    } else {
        format!(
            "{enriched}, {composition}, rich material textures and surface detail, layered lighting with soft shadows, sharp focus, ultra-detailed, professional photograph"
        )
    };
    let negative = if chinese {
        "模糊，低清晰度，变形，水印，文字，噪点，过曝，肢体错误".to_string()
    } else {
        "blurry, low resolution, distorted, watermark, text overlay, noise, overexposed, bad anatomy"
            .to_string()
    };
    RefineResult {
        output: image_prompt.clone(),
        image_prompt,
        video_prompt: String::new(),
        motion_prompt: String::new(),
        negative_prompt: negative,
    }
}

fn refine_video(
    prompt: &str,
    chinese: bool,
    has_reference_image: bool,
    aspect_ratio: Option<&str>,
) -> RefineResult {
    let motion = refine_motion_only(prompt, chinese);
    if has_reference_image {
        return motion;
    }
    let composition = composition_hint(aspect_ratio, chinese);
    let enriched = enrich_scene_detail(prompt, chinese);
    let image_prompt = if chinese {
        format!("{enriched}，{composition}，色彩饱满，光影自然，细节丰富，质感真实")
    } else {
        format!(
            "{enriched}, {composition}, vivid color, natural lighting, rich fine detail, realistic textures"
        )
    };
    let video_prompt = if chinese {
        format!("画面：{image_prompt}。运动：{}", motion.motion_prompt)
    } else {
        format!("Scene: {image_prompt}. Motion: {}", motion.motion_prompt)
    };
    let negative = default_video_negative(chinese);
    RefineResult {
        output: video_prompt.clone(),
        image_prompt,
        video_prompt,
        motion_prompt: motion.motion_prompt,
        negative_prompt: negative,
    }
}

fn refine_motion_only(prompt: &str, chinese: bool) -> RefineResult {
    let enriched = enrich_scene_detail(prompt, chinese);
    let motion_prompt = if chinese {
        format!(
            "{enriched}。镜头运动：缓慢推近并轻微环绕主体，主体有自然细微动作，画面稳定流畅，景深过渡柔和"
        )
    } else {
        format!(
            "{enriched}. Camera: slow dolly-in with a gentle orbit around the subject; subtle natural subject motion; stable smooth footage; soft depth-of-field transition"
        )
    };
    let negative = default_video_negative(chinese);
    RefineResult {
        output: motion_prompt.clone(),
        image_prompt: String::new(),
        video_prompt: motion_prompt.clone(),
        motion_prompt,
        negative_prompt: negative,
    }
}

/// Enrich short or sparse prompts with concrete visual detail anchors.
fn enrich_scene_detail(prompt: &str, chinese: bool) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    if trimmed.len() >= 80 || detail_rich_enough(trimmed) {
        return trimmed.to_string();
    }
    if chinese {
        format!("{trimmed}，环境氛围具体可见，前景与背景层次分明，主体特征清晰可辨")
    } else {
        format!(
            "{trimmed}, with concrete environmental atmosphere, clear foreground-background separation, and distinct subject features"
        )
    }
}

fn detail_rich_enough(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    let markers = [
        "lighting", "texture", "detail", "camera", "镜头", "光影", "质感", "细节", "氛围", "构图",
    ];
    markers.iter().filter(|m| lower.contains(**m)).count() >= 2
}

fn composition_hint(aspect_ratio: Option<&str>, chinese: bool) -> String {
    let ratio = aspect_ratio.unwrap_or("16:9");
    let portrait = ratio.contains("9:16") || ratio.contains("3:4") || ratio.contains("4:5");
    if chinese {
        if portrait {
            "竖幅构图，主体居中偏上，留出呼吸感留白".to_string()
        } else {
            "宽幅电影构图，主体与场景关系明确，视觉引导线清晰".to_string()
        }
    } else if portrait {
        "vertical portrait framing, subject upper-center with balanced negative space".to_string()
    } else {
        "cinematic wide composition with clear visual leading lines".to_string()
    }
}

fn default_video_negative(chinese: bool) -> String {
    if chinese {
        "模糊，变形，水印，字幕，低清晰度，画面抖动，闪烁，多余肢体，主体融化".to_string()
    } else {
        "blurry, distorted, watermark, subtitles, low quality, jitter, flicker, extra limbs, subject morphing"
            .to_string()
    }
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_refine_adds_detail_for_short_prompt() {
        let result = refine_prompt(&RefineInput {
            prompt: "a red sports car",
            medium: "image",
            aspect_ratio: Some("16:9"),
            has_reference_image: false,
        });
        assert!(result.image_prompt.contains("red sports car"));
        assert!(result.image_prompt.to_ascii_lowercase().contains("texture"));
    }

    #[test]
    fn motion_refine_for_img2video() {
        let result = refine_prompt(&RefineInput {
            prompt: "海浪拍打礁石",
            medium: "motion",
            aspect_ratio: Some("9:16"),
            has_reference_image: true,
        });
        assert!(result.motion_prompt.contains("镜头"));
    }

    #[test]
    fn video_refine_includes_scene_and_motion() {
        let result = refine_prompt(&RefineInput {
            prompt: "sunset over mountains",
            medium: "video",
            aspect_ratio: Some("16:9"),
            has_reference_image: false,
        });
        assert!(result.video_prompt.contains("Scene:"));
        assert!(result.video_prompt.contains("Motion:"));
        assert!(!result.negative_prompt.is_empty());
    }
}

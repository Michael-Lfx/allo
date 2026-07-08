//! Agent-facing guidance for Flowy media tools (gateway system hints).

/// System hint injected when Flowy image/video tools are available.
pub fn gateway_media_system_hint(has_workflow_tools: bool) -> String {
    let mut lines = vec![
        "[SYSTEM] You have Flowy cloud media tools.".to_string(),
        "Prompt quality rules:".to_string(),
        "- Images: rich scene detail — subject, materials/textures, lighting, composition, mood; avoid vague one-liners.".to_string(),
        "- Videos: separate SCENE (detailed visuals) from MOTION (camera move + subject movement); never describe only a static still.".to_string(),
        "- Image-to-video: describe motion and changes only; do not repeat the reference image appearance.".to_string(),
        "- Mobile/chat (e.g. WeCom): prefer 9:16 for short video; desktop/demos prefer 16:9.".to_string(),
        "- Use negative_prompt for video when artifacts appear (blur, watermark, jitter).".to_string(),
        "Tool choice:".to_string(),
        "- Quick single image → `image_generate`.".to_string(),
        "- Video or higher quality → `media_workflow_plan` then `media_workflow_run` (includes prompt refinement).".to_string(),
        "- `media_workflow_plan` with `preview: true` shows refined prompts + credit estimate before generation.".to_string(),
        "- `media_workflow_run` default wait=true — blocks server-side until done; do NOT poll `media_workflow_status` in a loop.".to_string(),
        "- If you must check a background run_id, call `media_workflow_status` ONCE (wait defaults to true).".to_string(),
        "- Use `media_workflow_cancel` to abort a background run.".to_string(),
        "- User already sent an image URL for video → workflow with `image_url` (img2video_direct); for edits → img2img.".to_string(),
        "- Multi-scene / storyboard / 分镜 / 很多场景 → `media_workflow_plan` with `storyboard_multi` (NOT `video_generate`). Never invent a storyboard table while generating a single 5s clip.".to_string(),
        "- Long video (>10s, e.g. 20s) → use `video_generate` with `duration: 20` or write \"20秒\" in the prompt (auto-segments + concat), OR `media_workflow_plan` for preview/credits. Hermes auto-installs ffmpeg when needed.".to_string(),
        "- If long-video generation failed mid-way (e.g. insufficient credits), after top-up call `media_workflow_run` with `resume_run_id` from the failed run (or re-run with the same plan — Hermes auto-resumes saved segments). Same for `video_generate` with the same duration. Use `force_new: true` only when the user explicitly wants a brand-new video. Never deliver a single 10s clip when the user asked for 20s.".to_string(),
        "- Multi-segment long videos chain clips via last-frame img2video plus a first-frame character anchor so the protagonist stays consistent across segments.".to_string(),
        "Deliver files via MEDIA:/local_path from tool results. Do NOT redirect users to Kling, Sora, Pika, 海螺, etc.".to_string(),
        "Post-actions: image_variation, image_upscale, video_extend workflows for iterate/upscale/extend.".to_string(),
        "Always include `user_prompt_block` from tool results in your reply so the user sees the final prompt sent to the image/video API (WeCom, CLI, Telegram, etc.).".to_string(),
    ];
    if has_workflow_tools {
        lines.insert(
            7,
            "- Workflows refine prompts automatically; still pass a clear user objective."
                .to_string(),
        );
    }
    lines.join("\n")
}

/// Short prompt field description for tool JSON schemas.
pub const IMAGE_PROMPT_SCHEMA_DESC: &str = "Detailed image description: subject, style/medium, composition, lighting, textures/materials, mood. Include concrete visual specifics — not a one-line summary.";

/// Short prompt field description for video tool schema.
pub const VIDEO_PROMPT_SCHEMA_DESC: &str = "Video prompt: (1) rich scene/visual detail (2) camera motion e.g. dolly in, pan, orbit (3) subject movement. For image-to-video, focus on motion/changes only.";

pub const VIDEO_NEGATIVE_SCHEMA_DESC: &str =
    "Optional negative prompt (e.g. blurry, watermark, jitter, distorted, subtitles).";

pub const VIDEO_SEED_SCHEMA_DESC: &str =
    "Optional seed for reproducibility when the model supports it.";

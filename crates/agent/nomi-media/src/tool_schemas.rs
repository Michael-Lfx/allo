//! Tool schemas for Flowy-backed media tools (no FAL/xAI model enums).

use indexmap::IndexMap;
use serde_json::json;

use nomi_types::{JsonSchema, ToolSchema, tool_schema};

use crate::prompt_guidance::{
    IMAGE_PROMPT_SCHEMA_DESC, VIDEO_NEGATIVE_SCHEMA_DESC, VIDEO_PROMPT_SCHEMA_DESC,
    VIDEO_SEED_SCHEMA_DESC,
};
use crate::workflows::templates::list_builtin_templates;

/// Schema for Flowy `video_generate` — model is optional Flowy id, not FAL families.
pub fn flowy_video_generate_schema() -> ToolSchema {
    let mut props = IndexMap::new();
    props.insert(
        "prompt".into(),
        json!({
            "type": "string",
            "description": VIDEO_PROMPT_SCHEMA_DESC
        }),
    );
    props.insert(
        "model".into(),
        json!({
            "type": "string",
            "description": "Optional Flowy video model list id (AIPC-... from `hermes media models`). Omit to use config default."
        }),
    );
    props.insert(
        "image_url".into(),
        json!({
            "type": "string",
            "description": "Optional starting image URL for image-to-video (first frame)."
        }),
    );
    props.insert(
        "reference_image_urls".into(),
        json!({
            "type": "array",
            "items": {"type": "string"},
            "description": "Optional reference image URLs for reference-guided video."
        }),
    );
    props.insert(
        "duration".into(),
        json!({
            "type": "integer",
            "minimum": 1,
            "maximum": 120,
            "description": "Video length in seconds. Seedance caps ~10s per clip; values above 10 auto-run long-video segmentation + concat (no extra tool needed)."
        }),
    );
    props.insert(
        "aspect_ratio".into(),
        json!({
            "type": "string",
            "description": "Aspect ratio: 16:9 (landscape) or 9:16 (mobile/WeCom).",
            "default": "16:9"
        }),
    );
    props.insert(
        "resolution".into(),
        json!({
            "type": "string",
            "description": "Output resolution when supported. Seedance fast often requires 720p.",
            "enum": ["360p", "480p", "540p", "720p", "1080p"]
        }),
    );
    props.insert(
        "negative_prompt".into(),
        json!({
            "type": "string",
            "description": VIDEO_NEGATIVE_SCHEMA_DESC
        }),
    );
    props.insert(
        "seed".into(),
        json!({
            "type": "integer",
            "description": VIDEO_SEED_SCHEMA_DESC
        }),
    );
    props.insert(
        "last_frame_url".into(),
        json!({
            "type": "string",
            "description": "Optional last-frame image URL for Seedance (role=last_frame)."
        }),
    );
    props.insert(
        "reference_video_url".into(),
        json!({
            "type": "string",
            "description": "Optional reference video URL (role=reference_video)."
        }),
    );
    props.insert(
        "reference_audio_url".into(),
        json!({
            "type": "string",
            "description": "Optional reference audio URL (role=reference_audio)."
        }),
    );
    props.insert(
        "generate_audio".into(),
        json!({
            "type": "boolean",
            "description": "Request generated audio track when supported by the model."
        }),
    );

    tool_schema(
        "video_generate",
        "Generate a video via Flowy Seedance. Duration >10s automatically splits into chained clips and concat (long video). Returns standardized assets with MEDIA: local path when save_locally is enabled. Response includes user_prompt_block with the exact API prompt — quote it in your reply.",
        JsonSchema::object(props, vec!["prompt".into()]),
    )
}

/// Schema for Flowy `image_generate`.
pub fn flowy_image_generate_schema() -> ToolSchema {
    let mut props = IndexMap::new();
    props.insert(
        "prompt".into(),
        json!({
            "type": "string",
            "description": IMAGE_PROMPT_SCHEMA_DESC
        }),
    );
    props.insert(
        "model".into(),
        json!({
            "type": "string",
            "description": "Optional Flowy image model list id (AIPC-... from `hermes media models`). Omit to use config default."
        }),
    );
    props.insert(
        "image_url".into(),
        json!({
            "type": "string",
            "description": "Optional reference image URL for image-to-image / edit."
        }),
    );

    tool_schema(
        "image_generate",
        "Generate an image via Flowy cloud API. Returns standardized assets with MEDIA: local path when save_locally is enabled. Response includes user_prompt_block with the exact API prompt — quote it in your reply.",
        JsonSchema::object(props, vec!["prompt".into()]),
    )
}

/// Schema for `media_workflow_plan`.
pub fn media_workflow_plan_schema() -> ToolSchema {
    let mut props = IndexMap::new();
    props.insert(
        "objective".into(),
        json!({
            "type": "string",
            "description": "User goal in natural language. Include visual specifics (subject, style, mood, setting); workflows will refine into model-ready prompts."
        }),
    );
    props.insert(
        "workflow_id".into(),
        json!({
            "type": "string",
            "description": "Optional builtin template id",
            "enum": list_builtin_templates()
        }),
    );
    props.insert(
        "image_url".into(),
        json!({
            "type": "string",
            "description": "Reference image URL for image-to-video workflows (selects img2video_direct when auto)."
        }),
    );
    props.insert(
        "duration".into(),
        json!({"type": "integer", "description": "Video duration in seconds"}),
    );
    props.insert(
        "aspect_ratio".into(),
        json!({"type": "string", "description": "16:9 or 9:16 (defaults from platform when set)", "default": "16:9"}),
    );
    props.insert(
        "platform".into(),
        json!({
            "type": "string",
            "description": "Chat platform hint (wecom, telegram, discord) for default aspect ratio."
        }),
    );
    props.insert(
        "preview".into(),
        json!({
            "type": "boolean",
            "description": "When true, return refined prompt preview without running generation. Defaults to media.workflows.confirm_before_run."
        }),
    );
    props.insert(
        "preview_only".into(),
        json!({
            "type": "boolean",
            "description": "For storyboard_multi — plan shots only, no render."
        }),
    );
    props.insert(
        "shots_to_render".into(),
        json!({
            "type": "array",
            "items": {"type": "integer"},
            "description": "For storyboard_multi — render only these shot numbers (1-based)."
        }),
    );
    props.insert(
        "last_frame_url".into(),
        json!({
            "type": "string",
            "description": "Last frame URL for video_extend post-action workflow."
        }),
    );
    tool_schema(
        "media_workflow_plan",
        "Plan a multi-step image/video workflow with automatic prompt refinement, credit estimate, and optional prompt preview.",
        JsonSchema::object(props, vec!["objective".into()]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowy_video_schema_has_no_fal_model_enum() {
        let schema = flowy_video_generate_schema();
        let props = schema.parameters.properties.as_ref().expect("properties");
        let model = props.get("model").expect("model property");
        assert!(
            model.get("enum").is_none(),
            "Flowy video schema must not list FAL model families"
        );
    }
}

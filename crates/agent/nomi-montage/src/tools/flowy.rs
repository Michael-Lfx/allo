//! `flowy_image` / `flowy_video` — thin tool wrappers over
//! `nomi_media_backends::{MediaImage, MediaVideo}`.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use nomi_media_backends::{MediaImage, MediaVideo};

use crate::artifacts::{ArtifactRef, ArtifactRefKind};
use crate::error::{MontageError, MontageResult};
use crate::events::EventKind;

use super::contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};

pub struct FlowyImageTool;

#[async_trait]
impl MontageTool for FlowyImageTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "flowy_image".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Creative,
            capability: "image_generation".into(),
            provider: "flowy".into(),
            runtime: ToolRuntime::Api,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["prompt", "out_name"],
                "properties": {
                    "prompt": {"type": "string"},
                    "out_name": {"type": "string", "description": "filename under assets/images/, e.g. 'scene_1_wide.png'"},
                    "reference_image_names": {"type": "array", "items": {"type": "string"}}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "api_call".into(),
            estimated_cost_credits: Some(500),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        if ctx.is_cancelled() {
            return Err(MontageError::Cancelled);
        }
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("flowy_image requires 'prompt'".into()))?;
        let out_name = args
            .get("out_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("flowy_image requires 'out_name'".into()))?;

        let refs_dir = ctx.paths.assets_images_dir();
        let ref_names: Vec<String> = args
            .get("reference_image_names")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let ref_paths: Vec<PathBuf> = ref_names.iter().map(|n| refs_dir.join(n)).collect();
        let ref_path_refs: Vec<&std::path::Path> = ref_paths.iter().map(PathBuf::as_path).collect();

        let out_path = refs_dir.join(out_name);
        tokio::fs::create_dir_all(&refs_dir).await?;

        let image = ctx.image_backend()?;
        ctx.emit(
            EventKind::ToolCalled,
            format!("flowy_image → {out_name}"),
            Some(serde_json::json!({
                "prompt_chars": prompt.chars().count(),
                "model": ctx.models.image,
                "aspect": ctx.output.aspect,
            })),
        );
        image.generate(prompt, &ref_path_refs, &out_path).await?;

        ctx.emit(EventKind::ToolResult, format!("flowy_image wrote {out_name}"), None);
        Ok(ToolResult::ok(format!("generated image '{out_name}'"))
            .with_artifact(ArtifactRef::media(out_name, out_path.display().to_string(), ArtifactRefKind::Image)))
    }
}

pub struct FlowyVideoTool;

#[async_trait]
impl MontageTool for FlowyVideoTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "flowy_video".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Creative,
            capability: "video_generation".into(),
            provider: "flowy".into(),
            runtime: ToolRuntime::Api,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["prompt", "out_name", "duration_secs"],
                "properties": {
                    "prompt": {"type": "string"},
                    "out_name": {"type": "string", "description": "filename under assets/video/, e.g. 'shot_003.mp4'"},
                    "duration_secs": {"type": "integer", "minimum": 1, "maximum": 15},
                    "first_frame_name": {"type": "string"},
                    "last_frame_name": {"type": "string"},
                    "reference_image_names": {"type": "array", "items": {"type": "string"}}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "api_call_slow".into(),
            estimated_cost_credits: Some(4000),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        if ctx.is_cancelled() {
            return Err(MontageError::Cancelled);
        }
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("flowy_video requires 'prompt'".into()))?;
        let out_name = args
            .get("out_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("flowy_video requires 'out_name'".into()))?;
        let duration = ctx.clamp_clip_duration(
            args
                .get("duration_secs")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| MontageError::InvalidParams("flowy_video requires 'duration_secs'".into()))?
                as u32,
        );

        let images_dir = ctx.paths.assets_images_dir();
        let video_dir = ctx.paths.assets_video_dir();
        tokio::fs::create_dir_all(&video_dir).await?;

        let first_frame = args
            .get("first_frame_name")
            .and_then(|v| v.as_str())
            .map(|n| images_dir.join(n));
        let last_frame = args
            .get("last_frame_name")
            .and_then(|v| v.as_str())
            .map(|n| images_dir.join(n));
        let ref_names: Vec<String> = args
            .get("reference_image_names")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let ref_paths: Vec<PathBuf> = ref_names.iter().map(|n| images_dir.join(n)).collect();
        let ref_path_refs: Vec<&std::path::Path> = ref_paths.iter().map(PathBuf::as_path).collect();

        let out_path = video_dir.join(out_name);
        let video = ctx.video_backend()?;

        ctx.emit(
            EventKind::ToolCalled,
            format!("flowy_video → {out_name} ({duration}s)"),
            Some(serde_json::json!({
                "model": ctx.models.video,
                "aspect": ctx.output.aspect,
                "resolution": ctx.output.resolution,
                "duration_secs": duration,
            })),
        );
        video
            .generate(
                prompt,
                first_frame.as_deref(),
                last_frame.as_deref(),
                &ref_path_refs,
                duration,
                &out_path,
                None,
            )
            .await?;

        ctx.emit(EventKind::ToolResult, format!("flowy_video wrote {out_name}"), None);
        Ok(ToolResult::ok(format!("generated video '{out_name}'"))
            .with_artifact(ArtifactRef::media(out_name, out_path.display().to_string(), ArtifactRefKind::Video)))
    }
}

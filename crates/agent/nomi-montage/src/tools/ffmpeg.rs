//! Local ffmpeg-backed tools: `video_stitch` and `extract_last_frame`.
//!
//! Thin wrappers over `nomi_media_backends::media_local` — no ffmpeg
//! invocation logic lives in this crate.

use async_trait::async_trait;
use serde_json::Value;

use nomi_media_backends::media_local;

use crate::artifacts::{ArtifactRef, ArtifactRefKind};
use crate::error::{MontageError, MontageResult};
use crate::events::EventKind;

use super::contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};

pub struct VideoStitchTool;

#[async_trait]
impl MontageTool for VideoStitchTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "video_stitch".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Core,
            capability: "video_stitch".into(),
            provider: "ffmpeg".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["clip_names", "out_name"],
                "properties": {
                    "clip_names": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "ordered filenames under assets/video/"},
                    "out_name": {"type": "string", "description": "filename under renders/, e.g. 'final.mp4'"}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "local_cpu".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        if ctx.is_cancelled() {
            return Err(MontageError::Cancelled);
        }
        let clip_names: Vec<String> = args
            .get("clip_names")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .filter(|v: &Vec<String>| !v.is_empty())
            .ok_or_else(|| MontageError::InvalidParams("video_stitch requires non-empty 'clip_names'".into()))?;
        let out_name = args
            .get("out_name")
            .and_then(|v| v.as_str())
            .unwrap_or("final.mp4");

        let video_dir = ctx.paths.assets_video_dir();
        let clip_paths: Vec<std::path::PathBuf> = clip_names.iter().map(|n| video_dir.join(n)).collect();
        for (name, path) in clip_names.iter().zip(clip_paths.iter()) {
            if !path.is_file() {
                return Ok(ToolResult::failed(format!(
                    "clip '{name}' not found at {} — generate it before stitching",
                    path.display()
                )));
            }
        }
        let clip_refs: Vec<&std::path::Path> = clip_paths.iter().map(std::path::PathBuf::as_path).collect();
        let out_path = ctx.paths.renders_dir().join(out_name);

        ctx.emit(
            EventKind::ToolCalled,
            format!("video_stitch → {out_name} ({} clips)", clip_refs.len()),
            None,
        );
        media_local::concat_videos(&clip_refs, &out_path).await?;
        ctx.emit(EventKind::ToolResult, format!("video_stitch wrote {out_name}"), None);

        Ok(ToolResult::ok(format!("stitched {} clips into '{out_name}'", clip_refs.len()))
            .with_artifact(ArtifactRef::media(out_name, out_path.display().to_string(), ArtifactRefKind::Video)))
    }
}

pub struct ExtractLastFrameTool;

#[async_trait]
impl MontageTool for ExtractLastFrameTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "extract_last_frame".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Core,
            capability: "frame_extract".into(),
            provider: "ffmpeg".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["video_name", "out_name"],
                "properties": {
                    "video_name": {"type": "string", "description": "filename under assets/video/"},
                    "out_name": {"type": "string", "description": "filename under assets/images/"}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "local_cpu".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        let video_name = args
            .get("video_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("extract_last_frame requires 'video_name'".into()))?;
        let out_name = args
            .get("out_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("extract_last_frame requires 'out_name'".into()))?;
        let video_path = ctx.paths.assets_video_dir().join(video_name);
        let out_path = ctx.paths.assets_images_dir().join(out_name);
        media_local::extract_last_frame(&video_path, &out_path).await?;
        Ok(ToolResult::ok(format!("extracted last frame of '{video_name}' to '{out_name}'"))
            .with_artifact(ArtifactRef::media(out_name, out_path.display().to_string(), ArtifactRefKind::Image)))
    }
}

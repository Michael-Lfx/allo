//! `image_selector` / `video_selector` — generate N candidates via the
//! underlying Flowy backend and keep the best one.
//!
//! The scoring here is intentionally simple and original (not a port of any
//! third-party selector): a candidate must first pass a hard usability check
//! (decodable image / probeable video container); among usable candidates we
//! prefer the first successful generation, since Flowy models do not expose a
//! deterministic quality signal we could rank on. This keeps the selector
//! mechanism (generate → validate → choose → discard rejects) faithful even
//! though the ranking heuristic itself is minimal in Phase 1.

use async_trait::async_trait;
use serde_json::Value;

use nomi_media_backends::media_local::{is_usable_image_file, is_usable_video_file};
use nomi_media_backends::{MediaImage, MediaVideo};

use crate::artifacts::{ArtifactRef, ArtifactRefKind};
use crate::error::{MontageError, MontageResult};
use crate::events::EventKind;

use super::contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};

const MAX_CANDIDATES: u32 = 3;

pub struct ImageSelectorTool;

#[async_trait]
impl MontageTool for ImageSelectorTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "image_selector".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Creative,
            capability: "image_generation".into(),
            provider: "flowy".into(),
            runtime: ToolRuntime::Hybrid,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["prompt", "out_name"],
                "properties": {
                    "prompt": {"type": "string"},
                    "out_name": {"type": "string"},
                    "candidates": {"type": "integer", "minimum": 1, "maximum": MAX_CANDIDATES},
                    "reference_image_names": {"type": "array", "items": {"type": "string"}}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: vec!["flowy_image".into()],
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
            .ok_or_else(|| MontageError::InvalidParams("image_selector requires 'prompt'".into()))?;
        let out_name = args
            .get("out_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("image_selector requires 'out_name'".into()))?;
        let candidates = args
            .get("candidates")
            .and_then(|v| v.as_u64())
            .map(|n| (n as u32).clamp(1, MAX_CANDIDATES))
            .unwrap_or(1);
        let images_dir = ctx.paths.assets_images_dir();
        tokio::fs::create_dir_all(&images_dir).await?;

        let ref_names: Vec<String> = args
            .get("reference_image_names")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let ref_paths: Vec<std::path::PathBuf> = ref_names.iter().map(|n| images_dir.join(n)).collect();
        let ref_refs: Vec<&std::path::Path> = ref_paths.iter().map(std::path::PathBuf::as_path).collect();

        let image = ctx.image_backend()?;
        let final_path = images_dir.join(out_name);
        let mut last_err = None;
        for attempt in 0..candidates {
            if ctx.is_cancelled() {
                return Err(MontageError::Cancelled);
            }
            let candidate_path = if attempt == 0 {
                final_path.clone()
            } else {
                images_dir.join(format!("{out_name}.candidate-{attempt}"))
            };
            ctx.emit(
                EventKind::ToolCalled,
                format!("image_selector attempt {}/{candidates} → {out_name}", attempt + 1),
                None,
            );
            match image.generate(prompt, &ref_refs, &candidate_path).await {
                Ok(()) if is_usable_image_file(&candidate_path) => {
                    if candidate_path != final_path {
                        let _ = tokio::fs::rename(&candidate_path, &final_path).await;
                    }
                    ctx.emit(EventKind::ToolResult, format!("image_selector chose attempt {}", attempt + 1), None);
                    return Ok(ToolResult::ok(format!(
                        "selected image '{out_name}' after {} attempt(s)",
                        attempt + 1
                    ))
                    .with_artifact(ArtifactRef::media(out_name, final_path.display().to_string(), ArtifactRefKind::Image)));
                }
                Ok(()) => {
                    let _ = tokio::fs::remove_file(&candidate_path).await;
                    last_err = Some("generated file failed usability check".to_string());
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        Ok(ToolResult::failed(format!(
            "image_selector exhausted {candidates} candidate(s) without a usable image: {}",
            last_err.unwrap_or_default()
        )))
    }
}

pub struct VideoSelectorTool;

#[async_trait]
impl MontageTool for VideoSelectorTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "video_selector".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Creative,
            capability: "video_generation".into(),
            provider: "flowy".into(),
            runtime: ToolRuntime::Hybrid,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["prompt", "out_name", "duration_secs"],
                "properties": {
                    "prompt": {"type": "string"},
                    "out_name": {"type": "string"},
                    "duration_secs": {"type": "integer", "minimum": 1, "maximum": 15},
                    "candidates": {"type": "integer", "minimum": 1, "maximum": MAX_CANDIDATES},
                    "first_frame_name": {"type": "string"},
                    "last_frame_name": {"type": "string"}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: vec!["flowy_video".into()],
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
            .ok_or_else(|| MontageError::InvalidParams("video_selector requires 'prompt'".into()))?;
        let out_name = args
            .get("out_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("video_selector requires 'out_name'".into()))?;
        let duration = ctx.clamp_clip_duration(
            args
                .get("duration_secs")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| MontageError::InvalidParams("video_selector requires 'duration_secs'".into()))?
                as u32,
        );
        let candidates = args
            .get("candidates")
            .and_then(|v| v.as_u64())
            .map(|n| (n as u32).clamp(1, MAX_CANDIDATES))
            .unwrap_or(1);

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

        let final_path = video_dir.join(out_name);
        let mut last_err = None;
        for attempt in 0..candidates {
            if ctx.is_cancelled() {
                return Err(MontageError::Cancelled);
            }
            let candidate_path = if attempt == 0 {
                final_path.clone()
            } else {
                video_dir.join(format!("{out_name}.candidate-{attempt}.mp4"))
            };
            ctx.emit(
                EventKind::ToolCalled,
                format!("video_selector attempt {}/{candidates} → {out_name}", attempt + 1),
                Some(serde_json::json!({
                    "model": ctx.models.video,
                    "aspect": ctx.output.aspect,
                    "resolution": ctx.output.resolution,
                    "duration_secs": duration,
                })),
            );
            let video = ctx.video_backend()?;
            match video
                .generate(prompt, first_frame.as_deref(), last_frame.as_deref(), &[], duration, &candidate_path, None)
                .await
            {
                Ok(()) if is_usable_video_file(&candidate_path) => {
                    if candidate_path != final_path {
                        let _ = tokio::fs::rename(&candidate_path, &final_path).await;
                    }
                    ctx.emit(EventKind::ToolResult, format!("video_selector chose attempt {}", attempt + 1), None);
                    return Ok(ToolResult::ok(format!(
                        "selected video '{out_name}' after {} attempt(s)",
                        attempt + 1
                    ))
                    .with_artifact(ArtifactRef::media(out_name, final_path.display().to_string(), ArtifactRefKind::Video)));
                }
                Ok(()) => {
                    let _ = tokio::fs::remove_file(&candidate_path).await;
                    last_err = Some("generated file failed usability check".to_string());
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        Ok(ToolResult::failed(format!(
            "video_selector exhausted {candidates} candidate(s) without a usable video: {}",
            last_err.unwrap_or_default()
        )))
    }
}

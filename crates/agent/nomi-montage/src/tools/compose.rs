//! `video_compose` — the governed entry point to turning shots into a final cut.
//!
//! Unlike `video_stitch` (a dumb concatenate), `video_compose` is where
//! `delivery_promise` and `slideshow_risk` are enforced *before* any encode
//! happens (see `assets/CONTRACT.md` Rule Zero). Phase 1 only implements the
//! `ffmpeg` render runtime; `remotion` / `hyperframes` are declared in some
//! pipeline YAMLs for Phase 2 and must fail explicitly here rather than
//! silently falling back to ffmpeg (that would itself be a silent downgrade
//! of the promised look, since Remotion/HyperFrames compositions imply
//! declarative overlays/captions ffmpeg alone cannot produce).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use nomi_media_backends::media_local;

use crate::artifacts::{ArtifactRef, ArtifactRefKind};
use crate::error::{MontageError, MontageResult};
use crate::events::EventKind;
use crate::governance::{
    DeliveryPromise, SlideshowRiskInputs, check_no_silent_downgrade, compute_slideshow_risk, is_blocked,
};

use super::contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};

#[derive(Debug, Deserialize)]
struct ComposeShot {
    name: String,
    #[serde(default)]
    is_motion: bool,
    #[serde(default)]
    hold_secs: f32,
}

#[derive(Debug, Deserialize)]
struct ComposeArgs {
    shots: Vec<ComposeShot>,
    #[serde(default = "default_runtime")]
    render_runtime: String,
    #[serde(default = "default_out_name")]
    out_name: String,
    #[serde(default = "default_promise")]
    delivery_promise: DeliveryPromise,
}

fn default_runtime() -> String {
    "ffmpeg".to_string()
}
fn default_out_name() -> String {
    "final.mp4".to_string()
}
fn default_promise() -> DeliveryPromise {
    DeliveryPromise::Motion
}

pub struct VideoComposeTool;

#[async_trait]
impl MontageTool for VideoComposeTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "video_compose".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Core,
            capability: "video_compose".into(),
            provider: "ffmpeg".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["shots"],
                "properties": {
                    "shots": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["name"],
                            "properties": {
                                "name": {"type": "string"},
                                "is_motion": {"type": "boolean"},
                                "hold_secs": {"type": "number"}
                            }
                        }
                    },
                    "render_runtime": {"type": "string", "enum": ["ffmpeg", "remotion", "hyperframes"]},
                    "out_name": {"type": "string"},
                    "delivery_promise": {"type": "string", "enum": ["motion", "hybrid_motion_still", "slideshow"]}
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
        let parsed: ComposeArgs = serde_json::from_value(args)
            .map_err(|e| MontageError::InvalidParams(format!("video_compose args: {e}")))?;
        if parsed.shots.is_empty() {
            return Err(MontageError::InvalidParams("video_compose requires at least one shot".into()));
        }

        if parsed.render_runtime != "ffmpeg" {
            let msg = format!(
                "render_runtime '{}' is not available in this build (Phase 1 only ships ffmpeg). \
                 Do not fall back to ffmpeg silently — either ask the human to accept an ffmpeg cut \
                 instead of the promised {} look, or hold this stage as awaiting_human.",
                parsed.render_runtime, parsed.render_runtime
            );
            ctx.emit(EventKind::Error, msg.clone(), None);
            return Err(MontageError::ToolUnavailable(
                format!("video_compose:{}", parsed.render_runtime),
                msg,
            ));
        }

        let total = parsed.shots.len();
        let motion = parsed.shots.iter().filter(|s| s.is_motion).count();
        let stills = total - motion;
        check_no_silent_downgrade(parsed.delivery_promise, total, motion)?;

        let long_runs = count_long_still_runs(&parsed.shots);
        let avg_hold = if stills == 0 {
            0.0
        } else {
            parsed
                .shots
                .iter()
                .filter(|s| !s.is_motion)
                .map(|s| s.hold_secs)
                .sum::<f32>()
                / stills as f32
        };
        let risk = compute_slideshow_risk(&SlideshowRiskInputs {
            total_shots: total,
            still_shots: stills,
            avg_still_hold_secs: avg_hold,
            long_still_runs: long_runs,
        });
        if is_blocked(risk) {
            return Err(MontageError::GovernanceBlocked(format!(
                "slideshow_risk={risk:.2} exceeds the compose threshold — send back to `assets`/`edit` \
                 for more motion coverage or shorter still holds before composing."
            )));
        }

        let video_dir = ctx.paths.assets_video_dir();
        let clip_paths: Vec<std::path::PathBuf> = parsed.shots.iter().map(|s| video_dir.join(&s.name)).collect();
        for (shot, path) in parsed.shots.iter().zip(clip_paths.iter()) {
            if !path.is_file() {
                return Ok(ToolResult::failed(format!(
                    "shot '{}' not found at {} — generate it before composing",
                    shot.name,
                    path.display()
                )));
            }
        }
        let clip_refs: Vec<&std::path::Path> = clip_paths.iter().map(std::path::PathBuf::as_path).collect();
        let out_path = ctx.paths.renders_dir().join(&parsed.out_name);

        ctx.emit(
            EventKind::ToolCalled,
            format!("video_compose(ffmpeg) → {} ({total} shots, risk={risk:.2})", parsed.out_name),
            None,
        );
        media_local::concat_videos(&clip_refs, &out_path).await?;
        ctx.emit(EventKind::ToolResult, format!("video_compose wrote {}", parsed.out_name), None);

        Ok(ToolResult::ok(format!(
            "composed {total} shots ({motion} motion / {stills} still) into '{}' via ffmpeg",
            parsed.out_name
        ))
        .with_meta(serde_json::json!({"slideshow_risk": risk, "motion_shots": motion, "total_shots": total}))
        .with_artifact(ArtifactRef::media(
            &parsed.out_name,
            out_path.display().to_string(),
            ArtifactRefKind::Video,
        )))
    }
}

fn count_long_still_runs(shots: &[ComposeShot]) -> usize {
    let mut runs = 0usize;
    let mut current = 0usize;
    for shot in shots {
        if shot.is_motion {
            current = 0;
        } else {
            current += 1;
            if current == 3 {
                runs += 1;
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_still_run_detection() {
        let shots = |pattern: &[bool]| {
            pattern
                .iter()
                .map(|m| ComposeShot {
                    name: "x".into(),
                    is_motion: *m,
                    hold_secs: 3.0,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(count_long_still_runs(&shots(&[true, true, true])), 0);
        assert_eq!(count_long_still_runs(&shots(&[false, false, false, true])), 1);
        // Six consecutive stills is still a single run (length >= 3), not two.
        assert_eq!(
            count_long_still_runs(&shots(&[false, false, false, false, false, false])),
            1
        );
        // Two separate runs of length >= 3, interrupted by motion.
        assert_eq!(
            count_long_still_runs(&shots(&[
                false, false, false, true, false, false, false
            ])),
            2
        );
    }
}

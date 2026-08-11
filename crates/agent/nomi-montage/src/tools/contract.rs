//! Tool contract — the Rust-side equivalent of OpenMontage's `BaseTool`.
//!
//! Every capability the Executive Producer can invoke (write an artifact,
//! generate an image, stitch clips…) implements [`MontageTool`] and declares a
//! [`ToolSpec`] up front, so the registry/preflight/UI can reason about
//! availability, cost, and fallbacks without executing anything.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::artifacts::{ArtifactRef, ArtifactRegistry};
use crate::error::MontageResult;
use crate::events::EventKind;
use crate::governance::CostDelta;
use crate::paths::ProjectPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTier {
    /// Always available, no external cost (artifact/checkpoint IO, decision log).
    Core,
    /// Generates or transforms creative media (image/video/compose/selectors).
    Creative,
    /// Governance / accounting helpers (cost, preflight).
    Governance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRuntime {
    /// Runs on-box (ffmpeg subprocess, filesystem IO).
    Local,
    /// Calls the Flowy server (chat/image/video).
    Api,
    /// Local orchestration around an API call (e.g. selector that scores then
    /// calls `flowy_image`).
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStability {
    Stable,
    Beta,
    /// Declared by a pipeline's `tools_available` but not registered in this
    /// build. Executing it always fails loudly (see [`crate::tools::registry`]).
    Unavailable,
}

/// Static identity + governance metadata for a tool, independent of any call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub version: String,
    pub tier: ToolTier,
    pub capability: String,
    pub provider: String,
    pub runtime: ToolRuntime,
    pub stability: ToolStability,
    pub input_schema: Value,
    pub output_schema: Value,
    #[serde(default)]
    pub fallback_tools: Vec<String>,
    #[serde(default)]
    pub resource_profile: String,
    #[serde(default)]
    pub estimated_cost_credits: Option<u64>,
}

/// Uniform result shape every tool returns, regardless of what it did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    #[serde(default)]
    pub artifacts: Vec<ArtifactRef>,
    pub message: String,
    #[serde(default)]
    pub cost: CostDelta,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub meta: Value,
}

impl ToolResult {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            artifacts: Vec::new(),
            message: message.into(),
            cost: CostDelta::zero(),
            model: None,
            meta: Value::Null,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            artifacts: Vec::new(),
            message: message.into(),
            cost: CostDelta::zero(),
            model: None,
            meta: Value::Null,
        }
    }

    pub fn with_artifact(mut self, artifact: ArtifactRef) -> Self {
        self.artifacts.push(artifact);
        self
    }

    pub fn with_cost(mut self, cost: CostDelta) -> Self {
        self.cost = cost;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = meta;
        self
    }
}

/// Everything a tool needs to act on one project, without reaching back into
/// the orchestrator.
#[derive(Clone)]
pub struct ToolContext {
    pub project_id: String,
    pub stage: String,
    pub paths: Arc<ProjectPaths>,
    pub artifact_registry: Arc<ArtifactRegistry>,
    pub media: Option<nomi_media_backends::FlowyMediaServices>,
    pub cancel: CancellationToken,
    pub events_path: PathBuf,
    /// User topic / brief captured at project creation.
    pub user_prompt: String,
    pub style_playbook: Option<String>,
    pub models: crate::project::ModelSelection,
    pub output: crate::project::OutputSettings,
    pub budget_credits: u64,
}

impl ToolContext {
    pub fn emit(&self, kind: EventKind, message: impl Into<String>, data: Option<Value>) {
        let mut record = crate::events::EventRecord::new(self.project_id.clone(), kind, message)
            .with_stage(self.stage.clone());
        if let Some(d) = data {
            record = record.with_data(d);
        }
        let _ = crate::events::append_event(&self.events_path, &record);
    }

    pub fn require_media(&self) -> MontageResult<&nomi_media_backends::FlowyMediaServices> {
        self.media
            .as_ref()
            .ok_or(crate::error::MontageError::NotAuthenticated)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Flowy image backend bound to this project's model + aspect preferences.
    pub fn image_backend(&self) -> MontageResult<nomi_media_backends::FlowyImage> {
        let media = self.require_media()?;
        let model = nonempty_opt(self.models.image.as_deref());
        let aspect = nonempty_opt(Some(self.output.aspect.as_str()));
        Ok(media.image_with_model_and_aspect(model, aspect))
    }

    /// Flowy video backend bound to this project's model + aspect + resolution.
    pub fn video_backend(&self) -> MontageResult<nomi_media_backends::FlowyVideo> {
        let media = self.require_media()?;
        let model = nonempty_opt(self.models.video.as_deref());
        let aspect = nonempty_opt(Some(self.output.aspect.as_str()));
        let resolution = nonempty_opt(Some(self.output.resolution.as_str()));
        Ok(media.video_with_session_quality(
            model,
            Some(self.cancel.clone()),
            aspect,
            resolution,
            None,
        ))
    }

    /// Clamp a requested clip duration into the Flowy / Seedance accepted range.
    pub fn clamp_clip_duration(&self, requested: u32) -> u32 {
        use nomi_media_backends::video_quality::{MAX_CLIP_DURATION_SECS, MIN_CLIP_DURATION_SECS};
        requested.clamp(MIN_CLIP_DURATION_SECS, MAX_CLIP_DURATION_SECS)
    }

    /// Human-readable production lock for EP / stage briefs.
    pub fn production_brief_block(&self) -> String {
        let mut lines = vec![
            format!("User brief:\n{}", self.user_prompt.trim()),
            format!(
                "Output lock: aspect={}, resolution={}, fps={}",
                self.output.aspect, self.output.resolution, self.output.fps
            ),
        ];
        if let Some(secs) = self.output.target_duration_secs {
            lines.push(format!(
                "Target finished duration: ~{secs}s (plan shot count / clip lengths to land near this; each Flowy clip must be {}–{}s).",
                nomi_media_backends::video_quality::MIN_CLIP_DURATION_SECS,
                nomi_media_backends::video_quality::MAX_CLIP_DURATION_SECS,
            ));
        }
        if let Some(pb) = self.style_playbook.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            lines.push(format!("Style / visual direction: {pb}"));
        }
        let mut model_bits = Vec::new();
        if let Some(m) = nonempty_opt(self.models.chat.as_deref()) {
            model_bits.push(format!("chat={m}"));
        }
        if let Some(m) = nonempty_opt(self.models.image.as_deref()) {
            model_bits.push(format!("image={m}"));
        }
        if let Some(m) = nonempty_opt(self.models.video.as_deref()) {
            model_bits.push(format!("video={m}"));
        }
        if !model_bits.is_empty() {
            lines.push(format!("Models: {}", model_bits.join(", ")));
        }
        lines.push(format!("Budget ceiling: {} credits", self.budget_credits));
        lines.join("\n")
    }
}

fn nonempty_opt(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[async_trait]
pub trait MontageTool: Send + Sync {
    fn spec(&self) -> &ToolSpec;

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult>;
}

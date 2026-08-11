//! `write_artifact` / `read_artifact` — schema-validated JSON artifact IO.

use async_trait::async_trait;
use serde_json::Value;

use crate::artifacts::{ArtifactRef, ArtifactRefKind};
use crate::error::{MontageError, MontageResult};
use crate::events::EventKind;

use super::contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};

pub struct WriteArtifactTool;

#[async_trait]
impl MontageTool for WriteArtifactTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "write_artifact".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Core,
            capability: "artifact_io".into(),
            provider: "local".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["name", "content"],
                "properties": {
                    "name": {"type": "string", "description": "canonical artifact name, e.g. 'script'"},
                    "content": {"type": "object", "description": "artifact JSON body, validated against schemas/artifacts/<name>.json"}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "cheap".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("write_artifact requires 'name'".into()))?;
        let content = args
            .get("content")
            .cloned()
            .ok_or_else(|| MontageError::InvalidParams("write_artifact requires 'content'".into()))?;

        if ctx.artifact_registry.has(name) {
            ctx.artifact_registry.validate(name, &content)?;
        }

        let path = ctx.paths.artifact_path(name);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let pretty = serde_json::to_string_pretty(&content)?;
        tokio::fs::write(&path, &pretty).await?;

        ctx.emit(
            EventKind::ArtifactWritten,
            format!("wrote artifact '{name}'"),
            Some(serde_json::json!({"artifact": name})),
        );

        Ok(ToolResult::ok(format!("artifact '{name}' written"))
            .with_artifact(ArtifactRef::json(name, path.display().to_string())))
    }
}

pub struct ReadArtifactTool;

#[async_trait]
impl MontageTool for ReadArtifactTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "read_artifact".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Core,
            capability: "artifact_io".into(),
            provider: "local".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["name"],
                "properties": {"name": {"type": "string"}}
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "cheap".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("read_artifact requires 'name'".into()))?;
        let path = ctx.paths.artifact_path(name);
        if !path.exists() {
            return Ok(ToolResult::failed(format!("artifact '{name}' does not exist yet")));
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let content: Value = serde_json::from_str(&raw)?;
        Ok(ToolResult::ok(format!("read artifact '{name}'"))
            .with_meta(serde_json::json!({"name": name, "content": content}))
            .with_artifact(ArtifactRef::media(name, path.display().to_string(), ArtifactRefKind::Json)))
    }
}

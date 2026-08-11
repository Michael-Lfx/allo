//! Bookkeeping tools: checkpoint notes, decision log, and cost estimate/reconcile.
//!
//! Checkpoint *stage advancement* is orchestrator-controlled (see
//! `orchestrator::stage_runner`), not exposed to the LLM — but the LLM can
//! attach a short note to the checkpoint (e.g. "used stock B-roll for shot 4
//! because Flowy video was rate-limited") so a human reviewing later sees why,
//! and it can append structured entries to `decision_log`.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;

use crate::checkpoint::CheckpointStore;
use crate::error::{MontageError, MontageResult};
use crate::events::EventKind;
use crate::governance::CostDelta;

use super::contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};

pub struct CheckpointNoteTool;

#[async_trait]
impl MontageTool for CheckpointNoteTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "checkpoint_note".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Core,
            capability: "checkpoint".into(),
            provider: "local".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["note"],
                "properties": {"note": {"type": "string"}}
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "cheap".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        let note = args
            .get("note")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("checkpoint_note requires 'note'".into()))?;
        let store = CheckpointStore::new(&ctx.paths);
        let Some(mut cp) = store.read()? else {
            return Ok(ToolResult::failed("no checkpoint exists yet for this project"));
        };
        cp.notes.push(format!("[{}] {note}", Utc::now().to_rfc3339()));
        if cp.notes.len() > 200 {
            let drain = cp.notes.len() - 200;
            cp.notes.drain(0..drain);
        }
        store.write(&cp)?;
        ctx.emit(EventKind::CheckpointWritten, "checkpoint note added", None);
        Ok(ToolResult::ok("note recorded"))
    }
}

pub struct DecisionLogAppendTool;

#[async_trait]
impl MontageTool for DecisionLogAppendTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "decision_log_append".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Core,
            capability: "decision_log".into(),
            provider: "local".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["decision"],
                "properties": {
                    "decision": {"type": "string"},
                    "rationale": {"type": "string"},
                    "alternatives_considered": {"type": "array", "items": {"type": "string"}}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "cheap".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        let decision = args
            .get("decision")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MontageError::InvalidParams("decision_log_append requires 'decision'".into()))?;
        let path = ctx.paths.decision_log_path();
        let mut log: Value = if path.exists() {
            let raw = tokio::fs::read_to_string(&path).await?;
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({"entries": []}))
        } else {
            serde_json::json!({"entries": []})
        };
        let entry = serde_json::json!({
            "at": Utc::now().to_rfc3339(),
            "stage": ctx.stage,
            "decision": decision,
            "rationale": args.get("rationale").and_then(|v| v.as_str()).unwrap_or(""),
            "alternatives_considered": args.get("alternatives_considered").cloned().unwrap_or_else(|| serde_json::json!([])),
        });
        log["entries"]
            .as_array_mut()
            .ok_or_else(|| MontageError::msg("decision_log.json corrupt: 'entries' is not an array"))?
            .push(entry);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, serde_json::to_string_pretty(&log)?).await?;
        ctx.emit(EventKind::ArtifactWritten, "decision logged", None);
        Ok(ToolResult::ok("decision recorded"))
    }
}

pub struct CostEstimateTool;

#[async_trait]
impl MontageTool for CostEstimateTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "cost_estimate".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Governance,
            capability: "cost".into(),
            provider: "local".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["tool", "estimated_credits"],
                "properties": {
                    "tool": {"type": "string"},
                    "estimated_credits": {"type": "integer", "minimum": 0},
                    "basis": {"type": "string"}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "cheap".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        let tool = args.get("tool").and_then(|v| v.as_str()).unwrap_or("unknown");
        let estimated = args
            .get("estimated_credits")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MontageError::InvalidParams("cost_estimate requires 'estimated_credits'".into()))?;
        ctx.emit(
            EventKind::ToolResult,
            format!("cost estimate for {tool}: {estimated} credits"),
            Some(serde_json::json!({"tool": tool, "estimated_credits": estimated})),
        );
        Ok(ToolResult::ok(format!("estimated {estimated} credits for {tool}"))
            .with_cost(CostDelta::of(0)))
    }
}

pub struct CostReconcileTool;

#[async_trait]
impl MontageTool for CostReconcileTool {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: "cost_reconcile".into(),
            version: "1.0.0".into(),
            tier: ToolTier::Governance,
            capability: "cost".into(),
            provider: "local".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Stable,
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["actual_credits"],
                "properties": {
                    "reserved_credits": {"type": "integer", "minimum": 0},
                    "actual_credits": {"type": "integer", "minimum": 0}
                }
            }),
            output_schema: serde_json::json!({"type": "object"}),
            fallback_tools: Vec::new(),
            resource_profile: "cheap".into(),
            estimated_cost_credits: Some(0),
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> MontageResult<ToolResult> {
        let actual = args
            .get("actual_credits")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| MontageError::InvalidParams("cost_reconcile requires 'actual_credits'".into()))?;
        ctx.emit(
            EventKind::ToolResult,
            format!("cost reconciled: {actual} credits"),
            None,
        );
        Ok(ToolResult::ok("cost reconciled").with_cost(CostDelta::of(actual)))
    }
}

//! Tool registry — resolves stage `tools_available` names to executable
//! [`MontageTool`]s, or an explicit [`UnavailableTool`] that always fails
//! loudly (Rule Zero: never silently no-op an unimplemented capability).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::MontageResult;

use super::contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};

pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn MontageTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn MontageTool>) {
        self.tools.insert(tool.spec().name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn MontageTool>> {
        self.tools.get(name).cloned()
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn spec(&self, name: &str) -> Option<ToolSpec> {
        self.tools.get(name).map(|t| t.spec().clone())
    }

    pub fn all_specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec().clone()).collect()
    }

    /// Resolve every `name` to a tool. Names with no registered implementation
    /// resolve to an [`UnavailableTool`] rather than being dropped, so the
    /// caller can still surface them (e.g. in a system prompt's tool list) and
    /// so a mistaken call fails with a clear message instead of "tool not found".
    pub fn allowlist(&self, names: &[String]) -> Vec<Arc<dyn MontageTool>> {
        names
            .iter()
            .map(|name| {
                self.get(name)
                    .unwrap_or_else(|| Arc::new(UnavailableTool::new(name.clone())))
            })
            .collect()
    }

    /// Names in `names` that have no registered implementation.
    pub fn unavailable_of(&self, names: &[String]) -> Vec<String> {
        names
            .iter()
            .filter(|n| !self.is_registered(n))
            .cloned()
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Stand-in for a tool a pipeline YAML lists in `tools_available` that has no
/// implementation in this build (e.g. `remotion_render` in Phase 1). Calling
/// it always fails with an explicit, actionable message — this is the
/// mechanical half of "never fake success" from `assets/CONTRACT.md`.
pub struct UnavailableTool {
    spec: ToolSpec,
}

impl UnavailableTool {
    pub fn new(name: String) -> Self {
        let spec = ToolSpec {
            name: name.clone(),
            version: "0.0.0".into(),
            tier: ToolTier::Core,
            capability: "unavailable".into(),
            provider: "none".into(),
            runtime: ToolRuntime::Local,
            stability: ToolStability::Unavailable,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            fallback_tools: Vec::new(),
            resource_profile: "none".into(),
            estimated_cost_credits: None,
        };
        Self { spec }
    }
}

#[async_trait]
impl MontageTool for UnavailableTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn execute(&self, ctx: &ToolContext, _args: Value) -> MontageResult<ToolResult> {
        let msg = format!(
            "tool '{}' is not available in this build. Do not claim this step succeeded — \
             tell the human it is missing (preflight should already have flagged it) or pick a \
             pipeline/runtime that only uses registered tools.",
            self.spec.name
        );
        ctx.emit(
            crate::events::EventKind::Error,
            msg.clone(),
            Some(serde_json::json!({ "tool": self.spec.name })),
        );
        Err(crate::error::MontageError::ToolUnavailable(
            self.spec.name.clone(),
            msg,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_tools_surface_by_name() {
        let registry = ToolRegistry::new();
        let names = vec!["remotion_render".to_string(), "flowy_image".to_string()];
        let unavailable = registry.unavailable_of(&names);
        assert_eq!(unavailable, names);
    }
}

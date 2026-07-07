use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::json;

use nomi_types::{JsonSchema, ToolError, ToolHandler, ToolSchema, tool_schema};

use crate::workflows::runner::WorkflowRunner;

pub struct MediaWorkflowCancelHandler {
    runner: Arc<WorkflowRunner>,
}

impl MediaWorkflowCancelHandler {
    pub fn new(runner: Arc<WorkflowRunner>) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl ToolHandler for MediaWorkflowCancelHandler {
    async fn execute(&self, params: serde_json::Value) -> Result<String, ToolError> {
        let run_id = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidParams("missing 'run_id'".into()))?;

        self.runner.cancel_run(run_id).await?;
        nomi_types::report_tool_progress(format!("媒体工作流已取消（run_id={run_id}）"));
        Ok(json!({
            "success": true,
            "run_id": run_id,
            "status": "cancelled",
            "hint": "Poll media_workflow_status to confirm terminal state."
        })
        .to_string())
    }

    fn schema(&self) -> ToolSchema {
        let mut props = IndexMap::new();
        props.insert(
            "run_id".into(),
            json!({"type":"string","description":"Workflow run id to cancel"}),
        );
        tool_schema(
            "media_workflow_cancel",
            "Cancel a running async media workflow and any in-flight video task.",
            JsonSchema::object(props, vec!["run_id".into()]),
        )
    }
}

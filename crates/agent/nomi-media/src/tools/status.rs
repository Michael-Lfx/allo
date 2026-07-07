use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::json;

use nomi_types::{JsonSchema, ToolError, ToolHandler, ToolSchema, tool_schema};

use crate::delivery::workflow_prompt_json;
use crate::workflows::store::WorkflowRunStore;

pub struct MediaWorkflowStatusHandler {
    store: Arc<WorkflowRunStore>,
    default_timeout: Duration,
}

impl MediaWorkflowStatusHandler {
    pub fn new(store: Arc<WorkflowRunStore>) -> Self {
        Self {
            store,
            default_timeout: Duration::from_secs(3600),
        }
    }

    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }
}

#[async_trait]
impl ToolHandler for MediaWorkflowStatusHandler {
    async fn execute(&self, params: serde_json::Value) -> Result<String, ToolError> {
        let run_id = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidParams("missing 'run_id'".into()))?;

        let wait = params.get("wait").and_then(|v| v.as_bool()).unwrap_or(true);
        let timeout_secs = params
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.default_timeout.as_secs());

        let record = if wait {
            self.store
                .wait_until_terminal(run_id, Duration::from_secs(timeout_secs))
                .await?
        } else {
            self.store.get(run_id).ok_or_else(|| {
                ToolError::ExecutionFailed(format!("workflow run not found: {run_id}"))
            })?
        };

        let mut out = json!({
            "run": record,
            "manifest_hint": format!("~/.hermes/media/workflows/{}/manifest.json", record.run_id),
            "waited": wait,
        });
        let prompt_payload = workflow_prompt_json(&record);
        if let (Some(obj), Some(prompts)) = (out.as_object_mut(), prompt_payload.as_object()) {
            for (key, value) in prompts {
                obj.insert(key.clone(), value.clone());
            }
        }
        Ok(out.to_string())
    }

    fn schema(&self) -> ToolSchema {
        let mut props = IndexMap::new();
        props.insert(
            "run_id".into(),
            json!({"type":"string","description":"Workflow run id from media_workflow_run"}),
        );
        props.insert(
            "wait".into(),
            json!({
                "type": "boolean",
                "description": "When true (default), block server-side until succeeded/failed/cancelled. Call ONCE — do NOT poll in an LLM loop."
            }),
        );
        props.insert(
            "timeout_secs".into(),
            json!({
                "type": "integer",
                "description": "Max seconds to wait when wait=true (default 3600)."
            }),
        );
        tool_schema(
            "media_workflow_status",
            "Query or wait for a media workflow run. Default wait=true blocks until complete without repeated LLM turns.",
            JsonSchema::object(props, vec!["run_id".into()]),
        )
    }
}

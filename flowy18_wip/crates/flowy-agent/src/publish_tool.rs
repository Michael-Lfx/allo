//! Explicit, host-owned artifact publishing tool.

use std::sync::Arc;

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{
    SessionOrigin, ToolCallArgs, ToolExecutionResult, ToolRunContext,
};
use serde::Deserialize;
use serde_json::json;

use crate::artifact_store::{ArtifactStore, PrepareArtifact};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishArgs {
    path: String,
    delivery_key: String,
    #[serde(default)]
    display_name: Option<String>,
}

pub fn definition(store: ArtifactStore) -> Arc<ToolDefinition> {
    Arc::new(
        ToolDefinition::new(
            "publish_file",
            "Publish one file created in the current turn's output directory. This snapshots the file for the conversation and library; it does not generate files. Only call after the requested file is complete. A successful call stages the file; final delivery is confirmed after the whole turn is saved.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "minLength": 1, "description": "Path relative to this turn's output directory."},
                    "delivery_key": {"type": "string", "minLength": 1, "maxLength": 128, "description": "Stable slot name for retries in this turn."},
                    "display_name": {"type": "string", "maxLength": 255, "description": "Optional safe display name; the source extension is preserved."}
                },
                "required": ["path", "delivery_key"],
                "additionalProperties": false
            }),
            move |args: ToolCallArgs, context: ToolRunContext| {
                let store = store.clone();
                Box::pin(async move {
                    if context.signal.is_cancelled() {
                        return ToolExecutionResult::error("CANCELLED", "publication cancelled");
                    }
                    if context.session_origin != SessionOrigin::Home {
                        return ToolExecutionResult::error("PUBLISH_SCOPE", "only Home conversations can publish files");
                    }
                    if !context.artifact_delivery_available {
                        return ToolExecutionResult::error("PUBLISH_CONTEXT", "durable delivery store is unavailable");
                    }
                    let Some(session_id) = context.agent_id.clone() else {
                        return ToolExecutionResult::error("PUBLISH_CONTEXT", "session identity is unavailable");
                    };
                    let Some(submission_id) = context.submission_id.clone() else {
                        return ToolExecutionResult::error("PUBLISH_CONTEXT", "turn identity is unavailable");
                    };
                    let Some(output_root) = context.output_root.clone() else {
                        return ToolExecutionResult::error("PUBLISH_CONTEXT", "turn output directory is unavailable");
                    };
                    let parsed: PublishArgs = match serde_json::from_value(args.arguments.clone()) {
                        Ok(parsed) => parsed,
                        Err(error) => return ToolExecutionResult::error("INVALID_ARGS", error.to_string()),
                    };
                    if context.signal.is_cancelled() {
                        return ToolExecutionResult::error("CANCELLED", "publication cancelled");
                    }
                    let receipt = match store.prepare(PrepareArtifact {
                        source_root: output_root,
                        relative_path: parsed.path,
                        session_id,
                        submission_id,
                        tool_call_id: args.call_id,
                        delivery_key: parsed.delivery_key,
                        display_name: parsed.display_name,
                        origin: context.session_origin,
                    }) {
                        Ok(receipt) => receipt,
                        Err(error) => return ToolExecutionResult::error("PUBLISH_FAILED", error.to_string()),
                    };
                    if context.signal.is_cancelled() {
                        // The prepared file remains non-visible and will never
                        // receive a committed marker without a successful turn.
                        return ToolExecutionResult::error("CANCELLED", "publication cancelled");
                    }
                    let summary = json!({
                        "status": "staged",
                        "artifact_id": receipt.artifact_id,
                        "file_name": receipt.display_name,
                        "bytes": receipt.size_bytes,
                        "sha256": receipt.sha256,
                        "message": "File staged. It becomes a delivered artifact only when this turn completes successfully."
                    });
                    let text = serde_json::to_string(&summary).unwrap_or_else(|_| "File staged for turn completion.".into());
                    let mut result = ToolExecutionResult::success_text(text, summary);
                    if let ToolExecutionResult::Success { delivery, .. } = &mut result {
                        *delivery = Some(Box::new(receipt));
                    }
                    result
                })
            },
        )
        .with_artifact_delivery(),
    )
}

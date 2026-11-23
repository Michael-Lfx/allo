//! Explicit, host-owned artifact publishing tool.

use std::path::Path;
use std::sync::Arc;

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{
    ContentBlock, SessionOrigin, ToolCallArgs, ToolExecutionResult,
    ToolRunContext,
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

fn publish_error(
    code: &'static str,
    ui_key: &'static str,
    message: &'static str,
) -> ToolExecutionResult {
    let content = json!({
        "status": "error",
        "code": code,
        "message": message,
        "ui_key": ui_key,
    })
    .to_string();
    ToolExecutionResult::Error {
        message: message.into(),
        code: code.into(),
        content: vec![ContentBlock::text(content)],
    }
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
                        return publish_error("CANCELLED", "chat.publish.cancelled", "File publication was cancelled.");
                    }
                    if context.session_origin != SessionOrigin::Home {
                        return publish_error("PUBLISH_SCOPE", "chat.publish.home_only", "Only Home conversations can publish files.");
                    }
                    if !context.artifact_delivery_available {
                        return publish_error("PUBLISH_CONTEXT", "chat.publish.unavailable", "File delivery is unavailable.");
                    }
                    let Some(session_id) = context.agent_id.clone() else {
                        return publish_error("PUBLISH_CONTEXT", "chat.publish.context", "This turn cannot publish files.");
                    };
                    let Some(submission_id) = context.submission_id.clone() else {
                        return publish_error("PUBLISH_CONTEXT", "chat.publish.context", "This turn cannot publish files.");
                    };
                    let Some(output_root) = context.output_root.clone() else {
                        return publish_error("PUBLISH_CONTEXT", "chat.publish.context", "This turn cannot publish files.");
                    };
                    let parsed: PublishArgs = match serde_json::from_value(args.arguments.clone()) {
                        Ok(parsed) => parsed,
                        Err(error) => {
                            tracing::warn!(%error, "publish_file received invalid arguments");
                            return publish_error("INVALID_ARGS", "chat.publish.invalid_args", "The file publication request was invalid.");
                        }
                    };
                    if context.signal.is_cancelled() {
                        return publish_error("CANCELLED", "chat.publish.cancelled", "File publication was cancelled.");
                    }
                    // The parsed-write importer identifies sources relative to
                    // the session workspace, while publish_file resolves them
                    // inside this turn's output directory.
                    let source_relative_path = output_root
                        .parent()
                        .and_then(Path::parent)
                        .and_then(|workspace_root| {
                            output_root.strip_prefix(workspace_root).ok()
                        })
                        .map(|relative| {
                            relative
                                .join(Path::new(&parsed.path))
                                .to_string_lossy()
                                .replace('\\', "/")
                        })
                        .unwrap_or_else(|| parsed.path.replace('\\', "/"));
                    let receipt = match store.prepare(PrepareArtifact {
                        source_root: output_root,
                        relative_path: parsed.path,
                        source_relative_path,
                        conversation_import: false,
                        session_id,
                        submission_id,
                        tool_call_id: args.call_id,
                        delivery_key: parsed.delivery_key,
                        display_name: parsed.display_name,
                        origin: context.session_origin,
                    }) {
                        Ok(receipt) => receipt,
                        Err(error) => {
                            tracing::warn!(%error, "publish_file could not stage the file");
                            return publish_error("PUBLISH_FAILED", "chat.publish.failed", "Couldn't publish the file.");
                        }
                    };
                    if context.signal.is_cancelled() {
                        // The prepared file remains non-visible and will never
                        // receive a committed marker without a successful turn.
                        return publish_error("CANCELLED", "chat.publish.cancelled", "File publication was cancelled.");
                    }
                    let summary = json!({
                        "status": "staged",
                        "message": "File staged. It will be delivered when the turn completes.",
                        "ui_key": "chat.publish.staged",
                        "artifact_id": receipt.artifact_id,
                        "file_name": receipt.display_name,
                        "bytes": receipt.size_bytes,
                        "sha256": receipt.sha256,
                    });
                    let content = serde_json::to_string_pretty(&summary)
                        .unwrap_or_else(|_| summary.to_string());
                    let mut result =
                        ToolExecutionResult::success_text(content, summary);
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

#[cfg(test)]
mod tests {
    use super::publish_error;
    use flowy_agent_core::{ContentBlock, ToolExecutionResult};

    #[test]
    fn publish_failures_keep_model_copy_and_a_ui_localization_key() {
        let ToolExecutionResult::Error {
            message, content, ..
        } = publish_error(
            "CANCELLED",
            "chat.publish.cancelled",
            "File publication was cancelled.",
        )
        else {
            panic!("expected an error result");
        };
        assert_eq!(message, "File publication was cancelled.");
        let [ContentBlock::Text { text }] = content.as_slice() else {
            panic!("expected one text block");
        };
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(value["ui_key"], "chat.publish.cancelled");
        assert_eq!(value["message"], "File publication was cancelled.");
    }
}

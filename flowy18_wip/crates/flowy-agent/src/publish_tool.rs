//! Explicit, host-owned artifact publishing tool.

use std::path::Path;
use std::sync::{Arc, Mutex};

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{
    ContentBlock, SessionOrigin, ToolCallArgs, ToolExecutionResult,
    ToolRunContext,
};
use serde::Deserialize;
use serde_json::json;

use crate::artifact_store::{ArtifactStore, PrepareArtifact};
use crate::delivery::DeliveryStore;
use crate::handle::now_ms;

/// `(delivery_id, artifact_path, sha256)` entries staged during one turn so
/// the host can abort unresolved `deliver_intent` records if the turn ends
/// without producing a durable `DeliveryCommit`.
pub type StagedIntentLog = Arc<Mutex<Vec<(String, String, String)>>>;

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

#[cfg(test)]
pub fn definition(
    store: ArtifactStore,
    gate: Option<DeliveryStore>,
) -> Arc<ToolDefinition> {
    definition_with_revision(store, gate, None, None)
}

pub fn definition_with_revision(
    store: ArtifactStore,
    gate: Option<DeliveryStore>,
    expected_contract_revision: Option<u32>,
    staged_intents: Option<StagedIntentLog>,
) -> Arc<ToolDefinition> {
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
                let gate = gate.clone();
                let staged_intents = staged_intents.clone();
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
                    // Normalize redundant workspace-relative `outputs/<submission_id>/`
                    // prefixes so publish_file matches create_deck / verify_deck /
                    // verify_artifact when models pass the same relative path across tools.
                    let normalized_path = {
                        let slashed = parsed.path.replace('\\', "/");
                        let trimmed =
                            slashed.strip_prefix("./").unwrap_or(&slashed);
                        let expected_prefix =
                            format!("outputs/{submission_id}/");
                        trimmed
                            .strip_prefix(&expected_prefix)
                            .unwrap_or(trimmed)
                            .to_owned()
                    };
                    // Strictly accept only normal relative components inside
                    // `output_root`: reject Windows drive prefixes (`D:foo`),
                    // rooted-without-drive (`\Users\...`), parent traversals
                    // (`..`), empty segments, and reserved device names.
                    let normalized = Path::new(&normalized_path);
                    let has_normal_component = normalized
                        .components()
                        .any(|c| matches!(c, std::path::Component::Normal(_)));
                    let only_safe_components = normalized.components().all(|c| {
                        matches!(
                            c,
                            std::path::Component::Normal(_)
                                | std::path::Component::CurDir
                        )
                    });
                    if normalized_path.trim().is_empty()
                        || normalized_path.contains('\0')
                        || !has_normal_component
                        || !only_safe_components
                        || crate::handle::has_reserved_device_component(
                            normalized,
                        )
                        || !crate::handle::confined_to(
                            &output_root,
                            &normalized_path,
                        )
                    {
                        return publish_error(
                            "PUBLISH_FAILED",
                            "chat.publish.failed",
                            "Couldn't publish the file.",
                        );
                    }
                    let artifact_candidate = output_root.join(&normalized_path);
                    if std::fs::symlink_metadata(&artifact_candidate)
                        .is_ok_and(|meta| meta.file_type().is_symlink())
                    {
                        return publish_error(
                            "PUBLISH_FAILED",
                            "chat.publish.failed",
                            "Couldn't publish the file.",
                        );
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
                        .map_or_else(
                            || normalized_path.clone(),
                            |relative| {
                                relative
                                    .join(Path::new(&normalized_path))
                                    .to_string_lossy()
                                    .replace('\\', "/")
                            },
                        );
                    let prepare_args = PrepareArtifact {
                        source_root: output_root.clone(),
                        relative_path: normalized_path.clone(),
                        source_relative_path,
                        conversation_import: false,
                        session_id: session_id.clone(),
                        submission_id: submission_id.clone(),
                        tool_call_id: args.call_id,
                        delivery_key: parsed.delivery_key,
                        display_name: parsed.display_name,
                        origin: context.session_origin,
                    };
                    let gate_clone = gate.clone();
                    let store_clone = store.clone();
                    let output_root_clone = output_root.clone();
                    let normalized_clone = normalized_path.clone();
                    let session_id_clone = session_id.clone();
                    let submission_id_clone = submission_id.clone();
                    let signal_clone = context.signal.clone();
                    let staged_log = staged_intents.clone();
                    tokio::task::spawn_blocking(move || {
                        let (receipt, staged_intent) =
                            if let Some(gate) = &gate_clone {
                                let artifact =
                                    output_root_clone.join(&normalized_clone);
                                match gate.stage_with_revision(
                                    &artifact,
                                    false,
                                    expected_contract_revision,
                                    now_ms(),
                                    |checked_bytes| {
                                        let expected_hash =
                                            crate::delivery::hash_bytes(
                                                checked_bytes,
                                            );
                                        let receipt =
                                            store_clone.prepare(prepare_args)?;
                                        if receipt.sha256 != expected_hash {
                                            return Err(std::io::Error::new(
                                                std::io::ErrorKind::InvalidData,
                                                "DELIVERY_ARTIFACT_CHANGED",
                                            ));
                                        }
                                        Ok(receipt)
                                    },
                                ) {
                                    Ok(result) => {
                                        if let Some(reason) =
                                            result.outcome.blocked
                                        {
                                            tracing::info!(
                                                target: "flowy.delivery",
                                                session_id = %session_id_clone,
                                                root_session_id = %session_id_clone,
                                                submission_id = %submission_id_clone,
                                                delivery_id = %result.outcome.delivery_id,
                                                code = reason.code(),
                                                path = %artifact.display(),
                                                "publish_file blocked by the delivery gate"
                                            );
                                            return publish_error(
                                                reason.code(),
                                                "chat.publish.not_verified",
                                                "This file has not passed verification.",
                                            );
                                        }
                                        let Some(receipt) = result.value else {
                                            return publish_error(
                                                "PUBLISH_FAILED",
                                                "chat.publish.failed",
                                                "Couldn't publish the file.",
                                            );
                                        };
                                        tracing::info!(
                                            target: "flowy.delivery",
                                            session_id = %session_id_clone,
                                            root_session_id = %session_id_clone,
                                            submission_id = %submission_id_clone,
                                            delivery_id = %result.outcome.delivery_id,
                                            artifact_id = %receipt.artifact_id,
                                            sha256 = %receipt.sha256,
                                            exemption_code = result.outcome.exemption_code.unwrap_or(""),
                                            "publish_file staged through delivery gate"
                                        );
                                        let intent = Some((
                                            gate.clone(),
                                            result.outcome.delivery_id,
                                            artifact.display().to_string(),
                                        ));
                                        (receipt, intent)
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, "publish_file failed in delivery gate or staging");
                                        if error.kind()
                                            == std::io::ErrorKind::InvalidData
                                            && error.to_string()
                                                == "DELIVERY_ARTIFACT_CHANGED"
                                        {
                                            return publish_error(
                                                "DELIVERY_ARTIFACT_CHANGED",
                                                "chat.publish.not_verified",
                                                "This file changed after verification.",
                                            );
                                        }
                                        return publish_error(
                                            "PUBLISH_FAILED",
                                            "chat.publish.failed",
                                            "Couldn't publish the file.",
                                        );
                                    }
                                }
                            } else {
                                match store_clone.prepare(prepare_args) {
                                    Ok(receipt) => (receipt, None),
                                    Err(error) => {
                                        tracing::warn!(%error, "publish_file could not stage the file");
                                        return publish_error(
                                            "PUBLISH_FAILED",
                                            "chat.publish.failed",
                                            "Couldn't publish the file.",
                                        );
                                    }
                                }
                            };
                        if signal_clone.is_cancelled() {
                            // Close the staged intent before returning CANCELLED
                            // so a cancelled tool call never leaves a ghost
                            // in_doubt entry in the audit log.
                            if let Some((gate, delivery_id, artifact_path)) =
                                staged_intent
                            {
                                let _ = gate.abort_intent(
                                    &delivery_id,
                                    &artifact_path,
                                    &receipt.sha256,
                                    "publish_cancelled",
                                    now_ms(),
                                );
                            }
                            return publish_error(
                                "CANCELLED",
                                "chat.publish.cancelled",
                                "File publication was cancelled.",
                            );
                        }
                        if let (
                            Some(log),
                            Some((_, delivery_id, artifact_path)),
                        ) = (staged_log, staged_intent)
                        {
                            log.lock()
                                .unwrap_or_else(
                                    std::sync::PoisonError::into_inner,
                                )
                                .push((
                                    delivery_id,
                                    artifact_path,
                                    receipt.sha256.clone(),
                                ));
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
                        let mut result = ToolExecutionResult::success_text(
                            content, summary,
                        );
                        if let ToolExecutionResult::Success {
                            delivery, ..
                        } = &mut result
                        {
                            *delivery = Some(Box::new(receipt));
                        }
                        result
                    })
                    .await
                    .unwrap_or_else(|error| {
                        tracing::warn!(%error, "publish_file blocking task failed");
                        publish_error(
                            "PUBLISH_FAILED",
                            "chat.publish.failed",
                            "Couldn't publish the file.",
                        )
                    })
                })
            },
        )
        .with_artifact_delivery(),
    )
}

#[cfg(test)]
mod tests {
    use super::publish_error;
    use super::{DeliveryStore, definition};
    use flowy_agent_core::{
        CancelToken, ContentBlock, SessionOrigin, ToolCallArgs,
        ToolExecutionResult, ToolRunContext,
    };
    use serde_json::json;

    #[tokio::test]
    async fn a_pptx_without_a_report_is_refused_at_the_publish_entry() {
        let root = std::env::temp_dir()
            .join(format!("flowy-publish-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store =
            crate::artifact_store::ArtifactStore::open(root.join("store"))
                .unwrap();
        let gate = DeliveryStore::new(root.join("delivery"));
        let tool = definition(store.clone(), Some(gate));
        let submission_id = format!("submission-{:032x}", 1);
        let output_root = root.join("workspace/outputs").join(&submission_id);
        std::fs::create_dir_all(&output_root).unwrap();
        std::fs::write(output_root.join("deck.pptx"), b"PK deck").unwrap();
        let context = ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("session-1".to_owned()),
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: Some(submission_id),
            session_origin: SessionOrigin::Home,
            output_root: Some(output_root),
            artifact_delivery_available: true,
        };
        let args = ToolCallArgs {
            call_id: "call-1".to_owned(),
            name: "publish_file".to_owned(),
            arguments: json!({
                "path": "deck.pptx",
                "delivery_key": "deck",
            }),
        };
        let result = (tool.execute)(args, context).await;
        let ToolExecutionResult::Error { code, .. } = result else {
            panic!("an unverified deck must be refused, got {result:?}");
        };
        assert_eq!(code, "DELIVERY_NOT_VERIFIED");
        let events = std::fs::read_to_string(
            root.join("delivery/events.jsonl"),
        )
        .expect("ordinary publish_file refusal must write events.jsonl");
        assert!(events.contains("deliver_blocked"));
        assert!(events.contains("DELIVERY_NOT_VERIFIED"));
        assert_eq!(store.usage().unwrap().staging_bytes, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn non_exempt_unknown_extension_is_refused_and_audited_while_exempt_csv_passes()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-publish-alltypes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store =
            crate::artifact_store::ArtifactStore::open(root.join("store"))
                .unwrap();
        let gate = DeliveryStore::new(root.join("delivery"));
        let tool = definition(store.clone(), Some(gate));
        let submission_id = format!("submission-{:032x}", 2);
        let output_root = root.join("workspace/outputs").join(&submission_id);
        std::fs::create_dir_all(&output_root).unwrap();
        std::fs::write(output_root.join("bundle.zip"), b"PK\x03\x04archive")
            .unwrap();
        std::fs::write(output_root.join("sales.csv"), b"col\n1\n").unwrap();
        let make_ctx = || ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("session-2".to_owned()),
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: Some(submission_id.clone()),
            session_origin: SessionOrigin::Home,
            output_root: Some(output_root.clone()),
            artifact_delivery_available: true,
        };

        let refused = (tool.execute)(
            ToolCallArgs {
                call_id: "call-zip".to_owned(),
                name: "publish_file".to_owned(),
                arguments: json!({
                    "path": "bundle.zip",
                    "delivery_key": "zip",
                }),
            },
            make_ctx(),
        )
        .await;
        let ToolExecutionResult::Error { code, .. } = refused else {
            panic!("unknown unverified extension must be refused: {refused:?}");
        };
        assert_eq!(code, "DELIVERY_NOT_VERIFIED");
        assert_eq!(store.usage().unwrap().staging_bytes, 0);

        let allowed = (tool.execute)(
            ToolCallArgs {
                call_id: "call-csv".to_owned(),
                name: "publish_file".to_owned(),
                arguments: json!({
                    "path": "sales.csv",
                    "delivery_key": "csv",
                }),
            },
            make_ctx(),
        )
        .await;
        assert!(matches!(
            allowed,
            ToolExecutionResult::Success {
                delivery: Some(_),
                ..
            }
        ));
        let events =
            std::fs::read_to_string(root.join("delivery/events.jsonl"))
                .unwrap();
        assert!(events.contains("deliver_blocked"));
        assert!(events.contains("deliver_exempt"));
        assert!(events.contains(crate::delivery::EXEMPT_NO_VERIFIER_V0));
        assert!(events.contains("deliver_intent"));
        assert!(
            !events.contains("\"deliver\""),
            "staging alone must not record final deliver before Library commit"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn tampered_pptx_after_verification_is_refused_with_artifact_changed()
    {
        let root = std::env::temp_dir()
            .join(format!("flowy-publish-tampered-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store =
            crate::artifact_store::ArtifactStore::open(root.join("store"))
                .unwrap();
        let gate = DeliveryStore::new(root.join("delivery"));
        let submission_id = format!("submission-{:032x}", 3);
        let output_root = root.join("workspace/outputs").join(&submission_id);
        std::fs::create_dir_all(&output_root).unwrap();
        let deck = output_root.join("deck.pptx");
        std::fs::write(&deck, b"PK original").unwrap();
        gate.record_report(&deck, Vec::new(), 10).unwrap();
        std::fs::write(&deck, b"PK modified-after-report").unwrap();

        let tool = definition(store.clone(), Some(gate));
        let result = (tool.execute)(
            ToolCallArgs {
                call_id: "call-tampered".to_owned(),
                name: "publish_file".to_owned(),
                arguments: json!({
                    "path": "deck.pptx",
                    "delivery_key": "deck",
                }),
            },
            ToolRunContext {
                signal: CancelToken::new(),
                agent_id: Some("session-3".to_owned()),
                cwd: Some(root.display().to_string()),
                todos: None,
                submission_id: Some(submission_id),
                session_origin: SessionOrigin::Home,
                output_root: Some(output_root),
                artifact_delivery_available: true,
            },
        )
        .await;
        let ToolExecutionResult::Error { code, .. } = result else {
            panic!("tampered deck must be refused: {result:?}");
        };
        assert_eq!(code, "DELIVERY_ARTIFACT_CHANGED");
        assert_eq!(store.usage().unwrap().staging_bytes, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

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

    #[tokio::test]
    async fn publish_file_accepts_submission_prefixed_relative_paths() {
        let root = std::env::temp_dir()
            .join(format!("flowy-publish-prefix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store =
            crate::artifact_store::ArtifactStore::open(root.join("store"))
                .unwrap();
        let gate = DeliveryStore::new(root.join("delivery"));
        let submission_id = format!("submission-{:032x}", 9);
        let output_root = root.join("workspace/outputs").join(&submission_id);
        std::fs::create_dir_all(&output_root).unwrap();
        std::fs::write(output_root.join("sales.csv"), b"col\n1\n").unwrap();

        let tool = definition(store, Some(gate));
        let result = (tool.execute)(
            ToolCallArgs {
                call_id: "call-prefixed".to_owned(),
                name: "publish_file".to_owned(),
                arguments: json!({
                    "path": format!("outputs\\{submission_id}\\sales.csv"),
                    "delivery_key": "sales",
                }),
            },
            ToolRunContext {
                signal: CancelToken::new(),
                agent_id: Some("session-prefix".to_owned()),
                cwd: Some(root.display().to_string()),
                todos: None,
                submission_id: Some(submission_id),
                session_origin: SessionOrigin::Home,
                output_root: Some(output_root),
                artifact_delivery_available: true,
            },
        )
        .await;
        assert!(
            matches!(
                result,
                ToolExecutionResult::Success {
                    delivery: Some(_),
                    ..
                }
            ),
            "submission-prefixed relative path must publish cleanly: {result:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn stale_contract_revision_report_is_refused_at_publish_file() {
        let root = std::env::temp_dir()
            .join(format!("flowy-publish-stale-rev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store =
            crate::artifact_store::ArtifactStore::open(root.join("store"))
                .unwrap();
        let gate = DeliveryStore::new(root.join("delivery"));
        let submission_id = format!("submission-{:032x}", 10);
        let output_root = root.join("workspace/outputs").join(&submission_id);
        std::fs::create_dir_all(&output_root).unwrap();
        let deck = output_root.join("deck.pptx");
        std::fs::write(&deck, b"PK rev1-deck").unwrap();
        gate.record_report_bytes_with_revision(
            &deck,
            b"PK rev1-deck",
            Vec::new(),
            Some(1),
            10,
        )
        .unwrap();

        let tool = super::definition_with_revision(
            store.clone(),
            Some(gate),
            Some(2),
            None,
        );
        let result = (tool.execute)(
            ToolCallArgs {
                call_id: "call-stale-rev".to_owned(),
                name: "publish_file".to_owned(),
                arguments: json!({
                    "path": "deck.pptx",
                    "delivery_key": "deck",
                }),
            },
            ToolRunContext {
                signal: CancelToken::new(),
                agent_id: Some("session-rev".to_owned()),
                cwd: Some(root.display().to_string()),
                todos: None,
                submission_id: Some(submission_id),
                session_origin: SessionOrigin::Home,
                output_root: Some(output_root),
                artifact_delivery_available: true,
            },
        )
        .await;
        let ToolExecutionResult::Error { code, .. } = result else {
            panic!(
                "stale rev=1 report must be refused under rev=2: {result:?}"
            );
        };
        assert_eq!(code, "DELIVERY_CONTRACT_REVISION_MISMATCH");
        assert_eq!(store.usage().unwrap().staging_bytes, 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}

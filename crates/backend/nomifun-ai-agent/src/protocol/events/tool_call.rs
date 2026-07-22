use std::collections::HashSet;

use agent_client_protocol::schema::Meta as SdkMeta;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::artifact_store::PersistedArtifact;

/// Enforce the shared tool-name/arguments artifact contract at the normalized
/// runtime boundary. This is intentionally backend-agnostic: external runtimes
/// (OpenClaw, Remote, Nanobot) must not bypass the same minimum-count and MIME
/// rules merely because they did not run through `BackendOutputSink`.
pub fn validate_completed_artifact_contract(data: &ToolCallEventData) -> Result<(), String> {
    if data.status != ToolCallStatus::Completed {
        return Ok(());
    }
    validate_artifact_receipt_integrity(&data.name, &data.artifacts)?;
    let contract = nomi_agent::output::artifact_contract_with_input(&data.name, &data.args)
        .map_err(|error| format!("invalid artifact contract for tool '{}': {error}", data.name))?;
    let Some(contract) = contract else {
        return Ok(());
    };
    let mime_types = data
        .artifacts
        .iter()
        .map(|artifact| artifact.mime_type.as_str())
        .collect::<Vec<_>>();
    contract.validate_mimes(&mime_types).map_err(|error| {
        format!(
            "tool '{}' did not deliver its required verified artifacts: {error}",
            data.name
        )
    })
}

/// Validate identity and locator uniqueness independently of tool identity.
/// ACP updates may omit a title/raw tool name, but their untrusted receipt
/// batches must still satisfy the same UI-key and file-locator invariants.
pub fn validate_artifact_receipt_integrity(
    tool_name: &str,
    artifacts: &[PersistedArtifact],
) -> Result<(), String> {
    let mut artifact_ids = HashSet::with_capacity(artifacts.len());
    let mut canonical_paths = HashSet::with_capacity(artifacts.len());
    let mut relative_paths = HashSet::with_capacity(artifacts.len());
    for artifact in artifacts {
        if artifact.id.trim().is_empty() {
            return Err(format!(
                "tool '{}' reported an artifact with an empty id",
                tool_name
            ));
        }
        if !artifact_ids.insert(artifact.id.as_str()) {
            return Err(format!(
                "tool '{}' reported the same artifact id more than once: {}",
                tool_name, artifact.id
            ));
        }
        if !canonical_paths.insert(artifact.path.as_str()) {
            return Err(format!(
                "tool '{}' reported the same canonical artifact path more than once: {}",
                tool_name, artifact.path
            ));
        }
        if !relative_paths.insert(artifact.relative_path.as_str()) {
            return Err(format!(
                "tool '{}' reported the same workspace-relative artifact path more than once: {}",
                tool_name, artifact.relative_path
            ));
        }
    }
    Ok(())
}

/// Data for the `ToolCall` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEventData {
    pub call_id: String,
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
    pub status: ToolCallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Verified user-visible outputs. Inline base64 is never placed on the
    /// event bus or in conversation history; only durable metadata is stored.
    // Keep an explicit empty array on Running/Error correction frames. Live
    // consumers merge lifecycle updates by call_id; omitting this field could
    // otherwise leave an earlier completed receipt visible after failure.
    #[serde(default)]
    pub artifacts: Vec<PersistedArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpToolCallEventData {
    pub session_id: String,
    pub update: AcpToolCallUpdateData,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<SdkMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpToolCallUpdateData {
    #[serde(rename = "sessionUpdate")]
    pub session_update: AcpToolCallSessionUpdateKind,
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AcpToolCallStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<AcpToolCallKind>,
    #[serde(rename = "rawInput", skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<Value>,
    #[serde(rename = "rawOutput", skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<AcpToolCallContentItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<AcpToolCallLocationItem>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpToolCallSessionUpdateKind {
    ToolCall,
    ToolCallUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpToolCallKind {
    Read,
    Edit,
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpToolCallContentItem {
    Content {
        content: AcpToolCallTextBlock,
    },
    Diff {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        old_text: Option<String>,
        new_text: String,
    },
    /// Inline ACP media/resource bytes after verified workspace persistence.
    Artifact {
        artifact: PersistedArtifact,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_uri: Option<String>,
    },
    /// A provider-owned resource that is already addressable by URI. The URI
    /// is preserved instead of being silently discarded.
    ResourceLink {
        name: String,
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        size_bytes: Option<i64>,
    },
    Terminal {
        terminal_id: String,
    },
    /// Explicit delivery failure retained in the receipt. When this variant is
    /// emitted, the enclosing ACP tool status is forced to `failed`.
    ArtifactError {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpToolCallTextBlock {
    #[serde(rename = "type")]
    pub block_type: AcpToolCallTextBlockType,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpToolCallTextBlockType {
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpToolCallLocationItem {
    pub path: String,
}

/// Status of a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Running,
    Completed,
    Error,
    /// Turn closed (end_turn / cancel / truncate) before the tool finished.
    /// Not a tool execution failure — UI should show canceled, not failed.
    Canceled,
}

/// Project an ACP tool-call frame onto the canonical [`ToolCallEventData`] shape.
///
/// ACP-only fields (`kind`, `locations`, rich `content` blocks beyond artifacts,
/// `_meta`, `sessionUpdate`) are folded into `output`/`description` where useful
/// and otherwise dropped. Callers that need lossless ACP UI should keep the
/// original frame for wire/DB; use this projection for shared terminal gates,
/// artifact contracts, and channel outbox decisions.
pub fn project_acp_tool_call_to_tool_call(data: &AcpToolCallEventData) -> ToolCallEventData {
    let status = match data.update.status {
        Some(AcpToolCallStatus::Completed) => ToolCallStatus::Completed,
        Some(AcpToolCallStatus::Failed) => ToolCallStatus::Error,
        Some(AcpToolCallStatus::Pending | AcpToolCallStatus::InProgress) | None => {
            ToolCallStatus::Running
        }
    };
    let name = data
        .update
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "tool".to_owned());
    let args = data.update.raw_input.clone().unwrap_or(Value::Null);
    let artifacts = data
        .update
        .content
        .iter()
        .flatten()
        .filter_map(|item| match item {
            AcpToolCallContentItem::Artifact { artifact, .. } => Some(artifact.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let output = acp_tool_call_output_summary(data);
    ToolCallEventData {
        call_id: data.update.tool_call_id.clone(),
        name,
        args,
        status,
        input: data.update.raw_input.clone(),
        output,
        description: data.update.title.clone(),
        artifacts,
    }
}

fn acp_tool_call_output_summary(data: &AcpToolCallEventData) -> Option<String> {
    if let Some(raw_output) = data.update.raw_output.as_ref() {
        if let Some(text) = raw_output.as_str() {
            if !text.is_empty() {
                return Some(text.to_owned());
            }
        }
        if !raw_output.is_null() {
            return Some(raw_output.to_string());
        }
    }
    let texts = data
        .update
        .content
        .iter()
        .flatten()
        .filter_map(|item| match item {
            AcpToolCallContentItem::Content { content } => Some(content.text.as_str()),
            AcpToolCallContentItem::ArtifactError { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

/// Stable dedupe key for ACP content items — avoids `serde_json::to_string` on
/// the hot merge path.
pub fn acp_content_item_dedupe_key(item: &AcpToolCallContentItem) -> String {
    match item {
        AcpToolCallContentItem::Content { content } => {
            format!("content:{}:{}", content.text.len(), content.text)
        }
        AcpToolCallContentItem::Diff {
            path,
            old_text,
            new_text,
        } => format!(
            "diff:{path}:{}:{}:{}",
            old_text.as_ref().map(|s| s.len()).unwrap_or(0),
            new_text.len(),
            new_text
        ),
        AcpToolCallContentItem::Artifact { artifact, .. } => {
            format!("artifact:{}", artifact.id)
        }
        AcpToolCallContentItem::ResourceLink { uri, name, .. } => {
            format!("resource:{name}:{uri}")
        }
        AcpToolCallContentItem::Terminal { terminal_id } => {
            format!("terminal:{terminal_id}")
        }
        AcpToolCallContentItem::ArtifactError { message } => {
            format!("artifact_error:{message}")
        }
    }
}

/// Apply the normalized ToolCall artifact contract to an ACP update via projection.
pub fn validate_completed_acp_artifact_contract(
    data: &AcpToolCallEventData,
) -> Result<(), String> {
    if data.update.status != Some(AcpToolCallStatus::Completed) {
        return Ok(());
    }
    let projected = project_acp_tool_call_to_tool_call(data);
    validate_artifact_receipt_integrity("ACP artifact delivery", &projected.artifacts)
        .map_err(|error| format!("ACP {error}"))?;
    const IDENTITY_KEYS: &[&str] = &[
        "tool",
        "tool_name",
        "toolName",
        "name",
        "operation",
        "operation_name",
        "operationName",
    ];
    let mut identities = data
        .update
        .title
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    for value in [&data.update.raw_input, &data.update.raw_output]
        .into_iter()
        .filter_map(Option::as_ref)
    {
        let Some(object) = value.as_object() else {
            continue;
        };
        identities.extend(
            IDENTITY_KEYS
                .iter()
                .filter_map(|key| object.get(*key).and_then(Value::as_str)),
        );
    }
    identities.sort_unstable();
    identities.dedup();
    for name in identities {
        let mut candidate = projected.clone();
        candidate.name = name.to_owned();
        validate_completed_artifact_contract(&candidate).map_err(|error| format!("ACP {error}"))?;
    }
    Ok(())
}

/// A single entry in a `ToolGroup` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGroupEntry {
    pub call_id: String,
    pub name: String,
    pub status: ToolCallStatus,
    #[serde(default)]
    pub description: Option<String>,
}

/// Whether `preview` is a superseded streaming artifact for the same logical call as
/// `canonical` (e.g. text-channel `<tool_call>` progress or partial Browser args).
pub fn should_supersede_preview(preview: &ToolCallEventData, canonical: &ToolCallEventData) -> bool {
    if preview.call_id == canonical.call_id || preview.name != canonical.name {
        return false;
    }
    if !is_canonical_executable_tool_call(canonical) {
        return false;
    }
    is_superseding_preview_args(&preview.args, &preview.name, &canonical.args)
}

fn is_superseding_preview_args(preview_args: &Value, tool_name: &str, canonical_args: &Value) -> bool {
    if preview_args.is_null() {
        return true;
    }
    let Some(preview_obj) = preview_args.as_object() else {
        return false;
    };
    if preview_obj.is_empty() {
        return true;
    }
    let Some(canonical_obj) = canonical_args.as_object() else {
        return false;
    };
    if tool_name == "Browser" {
        let canonical_action = canonical_obj
            .get("action")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        let preview_action = preview_obj
            .get("action")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        if canonical_action && !preview_action {
            return true;
        }
    }
    preview_obj
        .iter()
        .all(|(key, preview_val)| canonical_obj.get(key).is_none_or(|canon_val| preview_val == canon_val))
}

/// A running tool call that carries enough args to execute (not a stream preview).
pub fn is_canonical_executable_tool_call(data: &ToolCallEventData) -> bool {
    if data.status != ToolCallStatus::Running {
        return false;
    }
    has_executable_tool_args(&data.name, &data.args)
}

fn has_executable_tool_args(tool_name: &str, args: &Value) -> bool {
    if args.is_null() {
        return false;
    }
    let Some(obj) = args.as_object() else {
        return !args.as_str().is_some_and(str::is_empty);
    };
    if obj.is_empty() {
        return false;
    }
    if tool_name == "Browser" {
        return obj
            .get("action")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
    }
    true
}

#[cfg(test)]
mod supersede_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn supersedes_text_channel_client_generated_preview() {
        let preview = ToolCallEventData {
            call_id: "nomi-call_fbb31e380c974b268f4561c1".into(),
            name: "Browser".into(),
            args: Value::Null,
            status: ToolCallStatus::Running,
            input: None,
            output: None,
            description: None,
            artifacts: vec![],
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019f4065a9857932ac6fa5c9c44e1c77".into(),
            name: "Browser".into(),
            args: json!({"action": "navigate", "url": "https://example.com"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"action": "navigate", "url": "https://example.com"})),
            output: None,
            description: None,
            artifacts: vec![],
        };
        assert!(should_supersede_preview(&preview, &canonical));
    }

    #[test]
    fn supersedes_partial_browser_url_only_preview() {
        let preview = ToolCallEventData {
            call_id: "nomi-call_019f4066ba327fe288252356d8081a64".into(),
            name: "Browser".into(),
            args: json!({"url": "https://www.bing.com/search?q=test"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"url": "https://www.bing.com/search?q=test"})),
            output: None,
            description: None,
            artifacts: vec![],
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019f4066ba967ff3b94a4e14d21dc970".into(),
            name: "Browser".into(),
            args: json!({"action": "navigate", "url": "https://www.bing.com/search?q=test"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"action": "navigate", "url": "https://www.bing.com/search?q=test"})),
            output: None,
            description: None,
            artifacts: vec![],
        };
        assert!(should_supersede_preview(&preview, &canonical));
    }

    #[test]
    fn does_not_supersede_completed_canonical() {
        let preview = ToolCallEventData {
            call_id: "nomi-call_call_preview".into(),
            name: "Browser".into(),
            args: Value::Null,
            status: ToolCallStatus::Running,
            input: None,
            output: None,
            description: None,
            artifacts: vec![],
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019real".into(),
            name: "Read".into(),
            args: json!({"path": "/tmp/a.txt"}),
            status: ToolCallStatus::Completed,
            input: Some(json!({"path": "/tmp/a.txt"})),
            output: Some("ok".into()),
            description: None,
            artifacts: vec![],
        };
        assert!(!should_supersede_preview(&preview, &canonical));
    }

    #[test]
    fn does_not_supersede_same_name_different_invocation() {
        let preview = ToolCallEventData {
            call_id: "nomi-call_fbb31e380c974b268f4561c1".into(),
            name: "Read".into(),
            args: json!({"path": "/tmp/a.txt"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"path": "/tmp/a.txt"})),
            output: None,
            description: None,
            artifacts: vec![],
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019f4065a9857932ac6fa5c9c44e1c77".into(),
            name: "Read".into(),
            args: json!({"path": "/tmp/b.txt"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"path": "/tmp/b.txt"})),
            output: None,
            description: None,
            artifacts: vec![],
        };
        assert!(!should_supersede_preview(&preview, &canonical));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_store::ArtifactKind;
    use serde_json::json;

    fn image(path: &str, relative_path: &str) -> PersistedArtifact {
        PersistedArtifact {
            id: format!("artifact-{relative_path}"),
            kind: ArtifactKind::Image,
            mime_type: "image/png".to_owned(),
            path: path.to_owned(),
            relative_path: relative_path.to_owned(),
            size_bytes: 1,
            sha256: "00".repeat(32),
        }
    }

    fn completed_images(artifacts: Vec<PersistedArtifact>) -> ToolCallEventData {
        ToolCallEventData {
            call_id: "call-images".to_owned(),
            name: "image_gen".to_owned(),
            args: json!({"count": 2}),
            status: ToolCallStatus::Completed,
            input: None,
            output: None,
            description: None,
            artifacts,
        }
    }

    #[test]
    fn duplicate_canonical_path_cannot_satisfy_requested_count() {
        let result = validate_completed_artifact_contract(&completed_images(vec![
            image("/workspace/a.png", "nomifun-artifacts/a.png"),
            image("/workspace/a.png", "nomifun-artifacts/alias.png"),
        ]));

        assert!(
            result
                .unwrap_err()
                .contains("same canonical artifact path more than once")
        );
    }

    #[test]
    fn empty_artifact_id_cannot_satisfy_requested_count() {
        let mut first = image("/workspace/a.png", "nomifun-artifacts/a.png");
        first.id = "   ".to_owned();
        let result = validate_completed_artifact_contract(&completed_images(vec![
            first,
            image("/workspace/b.png", "nomifun-artifacts/b.png"),
        ]));

        assert!(result.unwrap_err().contains("artifact with an empty id"));
    }

    #[test]
    fn duplicate_artifact_id_cannot_satisfy_requested_count() {
        let first = image("/workspace/a.png", "nomifun-artifacts/a.png");
        let mut second = image("/workspace/b.png", "nomifun-artifacts/b.png");
        second.id = first.id.clone();
        let result = validate_completed_artifact_contract(&completed_images(vec![first, second]));

        assert!(
            result
                .unwrap_err()
                .contains("same artifact id more than once")
        );
    }

    #[test]
    fn duplicate_relative_path_cannot_satisfy_requested_count() {
        let mut second = image("/workspace/b.png", "nomifun-artifacts/a.png");
        second.id = "artifact-relative-alias".to_owned();
        let result = validate_completed_artifact_contract(&completed_images(vec![
            image("/workspace/a.png", "nomifun-artifacts/a.png"),
            second,
        ]));

        assert!(
            result
                .unwrap_err()
                .contains("same workspace-relative artifact path more than once")
        );
    }

    #[test]
    fn distinct_receipts_satisfy_requested_count() {
        validate_completed_artifact_contract(&completed_images(vec![
            image("/workspace/a.png", "nomifun-artifacts/a.png"),
            image("/workspace/b.png", "nomifun-artifacts/b.png"),
        ]))
        .unwrap();
    }

    #[test]
    fn projects_acp_completed_with_artifacts() {
        use crate::artifact_store::{ArtifactKind, PersistedArtifact};

        let data = AcpToolCallEventData {
            session_id: "sess".into(),
            update: AcpToolCallUpdateData {
                session_update: AcpToolCallSessionUpdateKind::ToolCallUpdate,
                tool_call_id: "tool-1".into(),
                status: Some(AcpToolCallStatus::Completed),
                title: Some("image_gen".into()),
                kind: None,
                raw_input: Some(json!({"count": 1})),
                raw_output: None,
                content: Some(vec![AcpToolCallContentItem::Artifact {
                    artifact: PersistedArtifact {
                        id: "art-1".into(),
                        kind: ArtifactKind::Image,
                        mime_type: "image/png".into(),
                        path: "/tmp/a.png".into(),
                        relative_path: "a.png".into(),
                        size_bytes: 1,
                        sha256: "00".repeat(32),
                    },
                    source_uri: None,
                }]),
                locations: None,
            },
            meta: None,
        };
        let projected = project_acp_tool_call_to_tool_call(&data);
        assert_eq!(projected.call_id, "tool-1");
        assert_eq!(projected.name, "image_gen");
        assert_eq!(projected.status, ToolCallStatus::Completed);
        assert_eq!(projected.artifacts.len(), 1);
    }
}

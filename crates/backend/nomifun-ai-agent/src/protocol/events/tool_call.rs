use agent_client_protocol::schema::Meta as SdkMeta;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019f4065a9857932ac6fa5c9c44e1c77".into(),
            name: "Browser".into(),
            args: json!({"action": "navigate", "url": "https://example.com"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"action": "navigate", "url": "https://example.com"})),
            output: None,
            description: None,
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
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019f4066ba967ff3b94a4e14d21dc970".into(),
            name: "Browser".into(),
            args: json!({"action": "navigate", "url": "https://www.bing.com/search?q=test"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"action": "navigate", "url": "https://www.bing.com/search?q=test"})),
            output: None,
            description: None,
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
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019real".into(),
            name: "Read".into(),
            args: json!({"path": "/tmp/a.txt"}),
            status: ToolCallStatus::Completed,
            input: Some(json!({"path": "/tmp/a.txt"})),
            output: Some("ok".into()),
            description: None,
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
        };
        let canonical = ToolCallEventData {
            call_id: "nomi-call_019f4065a9857932ac6fa5c9c44e1c77".into(),
            name: "Read".into(),
            args: json!({"path": "/tmp/b.txt"}),
            status: ToolCallStatus::Running,
            input: Some(json!({"path": "/tmp/b.txt"})),
            output: None,
            description: None,
        };
        assert!(!should_supersede_preview(&preview, &canonical));
    }
}

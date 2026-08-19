//! Session observation envelope and vocabulary (schema_version = 1).
//!
//! The on-disk / API envelope is unlabeled: `event_type` is a string so a
//! reader can keep unknown events and their raw `payload`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Shared major with [`crate::SCHEMA_VERSION`].
pub const OBSERVATION_SCHEMA_VERSION: u32 = crate::SCHEMA_VERSION;

pub const EVENT_LLM_REQUEST: &str = "llm/request";
pub const EVENT_LLM_RESPONSE: &str = "llm/response";
pub const EVENT_TOOL_EXECUTION_STARTED: &str = "tool/execution_started";
pub const EVENT_TOOL_EXECUTION_COMPLETED: &str = "tool/execution_completed";
pub const EVENT_TOOL_EXECUTION_FAILED: &str = "tool/execution_failed";
pub const EVENT_TOOL_EXECUTION_CANCELLED: &str = "tool/execution_cancelled";
pub const EVENT_OBSERVATION_GAP: &str = "observation/gap";
pub const EVENT_TURN_START: &str = "turn/start";
pub const EVENT_TURN_END: &str = "turn/end";

/// Fallback `event_seq` boundary when no conversation / execution / turn is bound.
pub const PROCESS_BOUNDARY_ID: &str = "process";

/// Whether this call belongs on the session workflow UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationScope {
    SessionWorkflow,
    SessionAuxiliary,
    ProcessDiagnostic,
}

/// Completeness of the recorded request relative to what the model saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    Canonical,
    Wire,
    ProtocolPartial,
    Unknown,
}

/// Capture policy applied before persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capture {
    Full,
    Redacted,
    Truncated,
    MetadataOnly,
}

/// Integrity of one observation execution boundary (not the whole conversation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Integrity {
    Complete,
    Degraded,
}

/// What the agent did. Distinct from [`Integrity`] (whether the log is missing pieces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Truncated,
    Unknown,
}

/// One omitted-field note. UI must not invent values for these fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Omitted {
    pub field: String,
    pub reason: String,
}

/// Mapped existing IDs. Never introduce an unqualified `Run` type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationIds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    /// AgentExecution attempt. Must not be named `attempt_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_model_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_call_id: Option<String>,
}

impl ObservationIds {
    /// `event_seq` ownership: `root_turn_id` > `execution_id` > `conversation_id` > `process`.
    pub fn boundary_id(&self) -> String {
        if let Some(id) = nonempty(self.root_turn_id.as_deref()) {
            return id.to_owned();
        }
        if let Some(id) = nonempty(self.execution_id.as_deref()) {
            return id.to_owned();
        }
        if let Some(id) = nonempty(self.conversation_id.as_deref()) {
            return id.to_owned();
        }
        PROCESS_BOUNDARY_ID.to_owned()
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

/// Caller-supplied context for one model invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCallContext {
    pub call_kind: String,
    pub observation_scope: ObservationScope,
    pub ids: ObservationIds,
}

impl ModelCallContext {
    pub fn new(
        call_kind: impl Into<String>,
        observation_scope: ObservationScope,
        ids: ObservationIds,
    ) -> Self {
        Self {
            call_kind: call_kind.into(),
            observation_scope,
            ids,
        }
    }
}

/// Unlabeled observation envelope. Unknown `event_type` values remain readable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservationEvent {
    pub schema_version: u32,
    pub event_type: String,
    pub event_seq: u64,
    /// RFC3339 timestamp for display / duration only. Do not sort by this.
    pub timestamp: String,
    pub timestamp_ms: u64,
    pub payload: Value,
}

impl ObservationEvent {
    pub fn new(
        event_type: impl Into<String>,
        event_seq: u64,
        timestamp: impl Into<String>,
        timestamp_ms: u64,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            event_type: event_type.into(),
            event_seq,
            timestamp: timestamp.into(),
            timestamp_ms,
            payload,
        }
    }
}

/// Deserialize one JSONL line. Unknown `event_type` is kept with raw payload.
pub fn read_event(json: &str) -> Result<ObservationEvent, serde_json::Error> {
    serde_json::from_str(json)
}

/// Read mapped IDs from an event payload (`payload.ids`).
pub fn ids_from_payload(payload: &Value) -> ObservationIds {
    payload
        .get("ids")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_event() -> ObservationEvent {
        ObservationEvent::new(
            EVENT_LLM_REQUEST,
            1,
            "2026-08-18T10:00:00Z",
            1_755_508_800_000,
            json!({
                "call_kind": "agent_turn",
                "observation_scope": "session_workflow",
                "ids": {
                    "conversation_id": "c1",
                    "root_turn_id": "t1",
                    "model_call_id": "mc1"
                },
                "fidelity": "canonical",
                "capture": ["truncated", "redacted"],
                "request": { "model": "test", "system": "hi" }
            }),
        )
    }

    #[test]
    fn observation_event_roundtrips_json() {
        let event = sample_event();
        let json = serde_json::to_string(&event).unwrap();
        let back: ObservationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
        assert_eq!(back.schema_version, OBSERVATION_SCHEMA_VERSION);
        assert_eq!(back.event_type, EVENT_LLM_REQUEST);
        assert_eq!(back.payload["request"]["model"], "test");
    }

    #[test]
    fn unknown_event_type_keeps_raw_payload() {
        let raw = r#"{
            "schema_version": 1,
            "event_type": "future/mystery",
            "event_seq": 9,
            "timestamp": "2026-08-18T10:00:01Z",
            "timestamp_ms": 1755508801000,
            "payload": { "keep": true, "nested": { "x": 1 } }
        }"#;
        let event = read_event(raw).unwrap();
        assert_eq!(event.event_type, "future/mystery");
        assert_eq!(event.event_seq, 9);
        assert_eq!(event.payload["keep"], true);
        assert_eq!(event.payload["nested"]["x"], 1);
    }

    #[test]
    fn boundary_id_prefers_root_turn_over_execution_and_conversation() {
        let ids = ObservationIds {
            conversation_id: Some("conv".into()),
            execution_id: Some("exec".into()),
            root_turn_id: Some("turn".into()),
            ..ObservationIds::default()
        };
        assert_eq!(ids.boundary_id(), "turn");
    }

    #[test]
    fn boundary_id_prefers_execution_over_conversation() {
        let ids = ObservationIds {
            conversation_id: Some("conv".into()),
            execution_id: Some("exec".into()),
            root_turn_id: Some("  ".into()),
            ..ObservationIds::default()
        };
        assert_eq!(ids.boundary_id(), "exec");
    }

    #[test]
    fn boundary_id_falls_back_to_conversation_then_process() {
        let conv = ObservationIds {
            conversation_id: Some("conv".into()),
            ..ObservationIds::default()
        };
        assert_eq!(conv.boundary_id(), "conv");

        let empty = ObservationIds::default();
        assert_eq!(empty.boundary_id(), PROCESS_BOUNDARY_ID);

        let blank = ObservationIds {
            conversation_id: Some("".into()),
            execution_id: Some("   ".into()),
            ..ObservationIds::default()
        };
        assert_eq!(blank.boundary_id(), PROCESS_BOUNDARY_ID);
    }

    #[test]
    fn model_call_context_roundtrips() {
        let ctx = ModelCallContext::new(
            "compaction",
            ObservationScope::SessionWorkflow,
            ObservationIds {
                conversation_id: Some("c".into()),
                root_turn_id: Some("t".into()),
                ..ObservationIds::default()
            },
        );
        let json = serde_json::to_string(&ctx).unwrap();
        let back: ModelCallContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ctx);
        assert_eq!(back.ids.boundary_id(), "t");
    }

    #[test]
    fn vocabulary_enums_use_snake_case() {
        assert_eq!(
            serde_json::to_value(ObservationScope::SessionWorkflow).unwrap(),
            json!("session_workflow")
        );
        assert_eq!(
            serde_json::to_value(Fidelity::ProtocolPartial).unwrap(),
            json!("protocol_partial")
        );
        assert_eq!(
            serde_json::to_value(Capture::MetadataOnly).unwrap(),
            json!("metadata_only")
        );
        assert_eq!(
            serde_json::to_value(Integrity::Degraded).unwrap(),
            json!("degraded")
        );
        assert_eq!(
            serde_json::to_value(ExecutionStatus::Failed).unwrap(),
            json!("failed")
        );
    }
}

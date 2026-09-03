//! Wire DTOs for developer-mode session observation APIs.
//!
//! These types intentionally stay independent of the agent trace projector.
//! The backend adapter converts the internal projection into this stable HTTP
//! shape before a route serializes it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationListDto {
    pub recorder_health: RecorderHealthDto,
    pub summary: ObservationSummaryDto,
    #[serde(default)]
    pub turns: Vec<SessionObservationTurnDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecorderHealthDto {
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservationSummaryDto {
    pub turn_count: u64,
    pub model_call_count: u64,
    pub tool_count: u64,
    pub active_duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_span_ms: Option<u64>,
    pub integrity: String,
    pub coverage: String,
    pub max_event_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationTurnDto {
    pub root_turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_attempt_id: Option<String>,
    pub status: String,
    pub integrity: String,
    pub interrupted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_preview: Option<String>,
    #[serde(default)]
    pub prompt_preview_context_only: bool,
    #[serde(default)]
    pub max_event_seq: u64,
    #[serde(default)]
    pub has_turn_start: bool,
    #[serde(default)]
    pub has_turn_end: bool,
    pub gap_count: u32,
    #[serde(default)]
    pub timeline: Vec<SessionObservationTimelineEventDto>,
    #[serde(default)]
    pub model_calls: Vec<SessionObservationCallDto>,
    #[serde(default)]
    pub gaps: Vec<SessionObservationGapDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationCallDto {
    pub model_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_scope: Option<String>,
    pub status: String,
    pub integrity: String,
    pub interrupted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<SessionObservationTokenUsageDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_summary: Option<SessionObservationRequestSummaryDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_message_view: Option<SessionObservationRequestMessageViewDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_summary: Option<SessionObservationResponseSummaryDto>,
    #[serde(default)]
    pub tools: Vec<SessionObservationToolDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationRequestMessageViewDto {
    pub mode: String,
    pub hidden_message_count: u32,
    pub visible_message_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationTimelineEventDto {
    pub event_seq: u64,
    pub event_type: String,
    pub timestamp_ms: u64,
    pub relative_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationTokenUsageDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationRequestSummaryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub has_system: bool,
    #[serde(default)]
    pub message_count: u32,
    #[serde(default)]
    pub tool_definition_count: u32,
    #[serde(default)]
    pub system_omitted: bool,
    #[serde(default)]
    pub messages_omitted: bool,
    #[serde(default)]
    pub tools_omitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationResponseSummaryDto {
    #[serde(default)]
    pub has_text: bool,
    #[serde(default)]
    pub has_thinking: bool,
    #[serde(default)]
    pub text_omitted: bool,
    #[serde(default)]
    pub thinking_omitted: bool,
    #[serde(default)]
    pub tool_use_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationToolDto {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationGapDto {
    pub event_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationEventDto {
    pub schema_version: u32,
    pub event_type: String,
    pub event_seq: u64,
    pub timestamp: String,
    pub timestamp_ms: u64,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationExportTurnDto {
    pub root_turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_attempt_id: Option<String>,
    pub status: String,
    pub integrity: String,
    pub interrupted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_preview: Option<String>,
    #[serde(default)]
    pub prompt_preview_context_only: bool,
    pub max_event_seq: u64,
    pub has_turn_start: bool,
    pub has_turn_end: bool,
    pub gap_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationExportDto {
    pub export_version: u32,
    pub schema_version: u32,
    pub exported_at_ms: u64,
    pub conversation_id: String,
    pub root_turn_id: String,
    pub status: String,
    pub integrity: String,
    pub coverage: String,
    pub has_turn_end: bool,
    pub turn: SessionObservationExportTurnDto,
    pub events: Vec<SessionObservationEventDto>,
}

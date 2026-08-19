//! Session observation: canonical LLM/tool event log under
//! `{data_dir}/diagnostics/observation/`.
//!
//! This crate is intentionally agent-layer only — it does **not** depend on
//! `nomi-agent` or any `nomifun-*` backend crate.

mod capture;
mod event;
mod project;
mod recorder;
mod redact;
mod session;

/// Shared major schema version for observation events and eval alignment.
pub const SCHEMA_VERSION: u32 = 1;

pub use capture::{
    capture_and_size_cap, capture_borrowed, capture_canonical_request, omitted_binary_payload,
    MAX_EVENT_BYTES, OMITTED_REASON_BINARY_PAYLOAD,
};
pub use event::{
    ids_from_payload, read_event, Capture, ExecutionStatus, Fidelity, Integrity, ModelCallContext,
    ObservationEvent, ObservationIds, ObservationScope, Omitted, EVENT_LLM_REQUEST,
    EVENT_LLM_RESPONSE, EVENT_OBSERVATION_GAP, EVENT_TOOL_EXECUTION_CANCELLED,
    EVENT_TOOL_EXECUTION_COMPLETED, EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED,
    EVENT_TURN_END, EVENT_TURN_START, OBSERVATION_SCHEMA_VERSION, PROCESS_BOUNDARY_ID,
};
pub use project::{
    event_belongs_to_turn, fold_observation_summary, last_user_text_preview, project_call_detail,
    project_turn_by_id, project_turns, strip_projected_turn_payloads,
    NormalizedObservationUsage, ObservationSummary, ObservationSummaryFold, ProjectedGap,
    ProjectedModelCall, ProjectedRequestSummary, ProjectedResponseSummary, ProjectedTokenUsage,
    ProjectedToolExecution, ProjectedTurn, ToolExecutionStatus,
    COVERAGE_RETAINED_OBSERVATION_HISTORY,
};
pub use recorder::{
    ObservationRecorder, RecorderError, RecorderHealth, RecorderHealthStatus, GC_MAX_AGE_DAYS,
    MAX_CONTROL_EVENTS, MAX_QUEUE_EVENTS, MAX_TOTAL_OBSERVATION_BYTES, OBSERVATION_DIR,
    ROTATE_BYTES,
};
pub use redact::{
    redact_preview, truncate_chars, MAX_PREVIEW_CHARS, OMITTED_REASON_EVENT_SIZE_LIMIT,
};
pub use session::{classify_session_kind, is_session_dialogue};

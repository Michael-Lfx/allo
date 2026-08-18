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

pub use capture::capture_canonical_request;
pub use event::{
    ids_from_payload, read_event, Capture, Fidelity, Integrity, ModelCallContext, ObservationEvent,
    ObservationIds, ObservationScope, Omitted, EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE,
    EVENT_OBSERVATION_GAP, EVENT_TOOL_EXECUTION_CANCELLED, EVENT_TOOL_EXECUTION_COMPLETED,
    EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED,
    OBSERVATION_SCHEMA_VERSION, PROCESS_BOUNDARY_ID,
};
pub use project::{
    event_belongs_to_turn, project_turn_by_id, project_turns, strip_projected_turn_payloads,
    ProjectedGap, ProjectedModelCall, ProjectedToolExecution, ProjectedTurn,
};
pub use recorder::{
    ObservationRecorder, RecorderError, GC_MAX_AGE_DAYS, OBSERVATION_DIR, ROTATE_BYTES,
};
pub use redact::{redact_preview, truncate_chars, MAX_PREVIEW_CHARS};
pub use session::{classify_session_kind, is_session_dialogue};

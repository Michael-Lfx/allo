//! Developer-mode session observation.
//!
//! Recording is gated by the `system.developerMode` client preference and
//! never alters turn control flow — every failure degrades to a warning.

mod hub;
mod prefs;

pub use hub::{
    AgentTraceHub, SessionObservationList, TraceApiError, DEFAULT_SESSION_OBSERVATION_LIST_LIMIT,
    MAX_SESSION_OBSERVATION_LIST_LIMIT,
};
pub use prefs::{DEVELOPER_MODE_PREF_KEY, developer_mode_enabled};

pub use nomi_agent_trace::{
    ObservationIds, ObservationRecorder, ProjectedTurn, classify_session_kind, is_session_dialogue,
};

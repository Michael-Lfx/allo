//! Developer-mode session observation.
//!
//! Recording stays enabled for captured agent observations. The
//! `system.developerMode` client preference gates HTTP reads and the UI, never
//! turn control flow — every recorder failure degrades to a warning.

mod hub;
mod prefs;

pub use hub::{
    AgentTraceHub, SessionObservationList, TraceApiError, session_observation_call_dto,
    session_observation_list_dto, session_observation_turn_dto,
    DEFAULT_SESSION_OBSERVATION_LIST_LIMIT, MAX_SESSION_OBSERVATION_LIST_LIMIT,
};
pub use prefs::{DEVELOPER_MODE_PREF_KEY, developer_mode_enabled};

pub use nomi_agent_trace::{
    ObservationIds, ObservationRecorder, ProjectedTurn, classify_session_kind, is_session_dialogue,
};

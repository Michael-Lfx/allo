//! Developer-mode agent turn tracing.
//!
//! Collects a side-channel mirror of [`AgentStreamEvent`] into
//! [`nomi_agent_trace::FileTraceStore`]. Recording is gated by the
//! `system.developerMode` client preference and never alters turn control
//! flow — every failure degrades to a warning.

mod collector;
mod hub;
mod prefs;

pub use collector::{TurnTraceCollector, TurnTraceContext};
pub use hub::{AgentTraceHub, TraceApiError};
pub use prefs::{DEVELOPER_MODE_PREF_KEY, developer_mode_enabled};

pub use nomi_agent_trace::{
    FileTraceStore, TraceArtifactIndexEntry, TraceArtifactMeta, TraceIndexEntry, TraceStoreError,
    TurnTrace, classify_session_kind, is_session_dialogue,
};

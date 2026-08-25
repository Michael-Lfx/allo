//! Meeting session domain types and E1 event envelopes.

use serde::{Deserialize, Serialize};

use crate::frame::AudioChannel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingSessionStatus {
    Created,
    Recording,
    Paused,
    Stopping,
    Stopped,
    Failed,
}

impl MeetingSessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Recording => "recording",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "created" => Some(Self::Created),
            "recording" => Some(Self::Recording),
            "paused" => Some(Self::Paused),
            "stopping" => Some(Self::Stopping),
            "stopped" => Some(Self::Stopped),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttBackendChoice {
    Auto,
    LocalSherpa,
    CloudModelInvoke,
}

impl SttBackendChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::LocalSherpa => "local_sherpa",
            Self::CloudModelInvoke => "cloud_model_invoke",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "auto" => Some(Self::Auto),
            "local_sherpa" => Some(Self::LocalSherpa),
            "cloud_model_invoke" => Some(Self::CloudModelInvoke),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSessionSnapshot {
    pub session_id: String,
    pub user_id: String,
    pub title: String,
    pub status: MeetingSessionStatus,
    pub bound_conversation_id: Option<String>,
    pub data_dir: String,
    pub mic_available: bool,
    pub loopback_available: bool,
    pub stt_backend: SttBackendChoice,
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSegmentSnapshot {
    pub segment_id: String,
    pub session_id: String,
    pub channel: Option<AudioChannel>,
    pub speaker_id: Option<String>,
    pub speaker_label: String,
    pub text: String,
    pub is_partial: bool,
    pub is_manual_edit: bool,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MeetingEvent {
    SessionUpdated {
        session: MeetingSessionSnapshot,
    },
    SegmentUpserted {
        segment: MeetingSegmentSnapshot,
    },
    CapabilityDegraded {
        session_id: String,
        mic_available: bool,
        loopback_available: bool,
        message: String,
    },
    Error {
        session_id: String,
        message: String,
    },
}

impl MeetingEvent {
    pub const WS_NAME: &'static str = "meeting:event";

    pub fn session_id(&self) -> &str {
        match self {
            Self::SessionUpdated { session } => &session.session_id,
            Self::SegmentUpserted { segment } => &segment.session_id,
            Self::CapabilityDegraded { session_id, .. } => session_id,
            Self::Error { session_id, .. } => session_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateMeetingSessionRequest {
    pub user_id: String,
    pub title: String,
    pub data_dir: String,
    pub bound_conversation_id: Option<String>,
    pub stt_backend: SttBackendChoice,
    pub mic_available: bool,
    pub loopback_available: bool,
}

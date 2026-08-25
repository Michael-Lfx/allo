use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MeetingSessionRow {
    pub id: i64,
    pub session_id: String,
    pub user_id: String,
    pub title: String,
    pub status: String,
    pub bound_conversation_id: Option<String>,
    pub data_dir: String,
    pub mic_available: i64,
    pub loopback_available: i64,
    pub stt_backend: String,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MeetingSegmentRow {
    pub id: i64,
    pub session_id: String,
    pub segment_id: String,
    pub channel: Option<String>,
    pub speaker_id: Option<String>,
    pub speaker_label: String,
    pub text: String,
    pub is_partial: i64,
    pub is_manual_edit: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MeetingSpeakerRow {
    pub id: i64,
    pub session_id: String,
    pub speaker_id: String,
    pub display_name: String,
    pub voiceprint_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MeetingVoiceprintRow {
    pub id: i64,
    pub voiceprint_id: String,
    pub user_id: String,
    pub display_name: String,
    pub embedding_blob: Vec<u8>,
    pub created_at: i64,
    pub updated_at: i64,
}

use crate::error::DbError;
use crate::models::{
    MeetingSegmentRow, MeetingSessionRow, MeetingSpeakerRow, MeetingVoiceprintRow,
};

#[derive(Debug, Clone)]
pub struct InsertMeetingSessionParams {
    pub session_id: String,
    pub user_id: String,
    pub title: String,
    pub status: String,
    pub bound_conversation_id: Option<String>,
    pub data_dir: String,
    pub mic_available: bool,
    pub loopback_available: bool,
    pub stt_backend: String,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct UpdateMeetingSessionParams {
    pub title: Option<String>,
    pub status: Option<String>,
    pub bound_conversation_id: Option<Option<String>>,
    pub mic_available: Option<bool>,
    pub loopback_available: Option<bool>,
    pub stt_backend: Option<String>,
    pub started_at: Option<Option<i64>>,
    pub ended_at: Option<Option<i64>>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct UpsertMeetingSegmentParams {
    pub session_id: String,
    pub segment_id: String,
    pub channel: Option<String>,
    pub speaker_id: Option<String>,
    pub speaker_label: String,
    pub text: String,
    pub is_partial: bool,
    pub is_manual_edit: bool,
    pub start_ms: i64,
    pub end_ms: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct UpsertMeetingSpeakerParams {
    pub session_id: String,
    pub speaker_id: String,
    pub display_name: String,
    pub voiceprint_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct UpsertMeetingVoiceprintParams {
    pub voiceprint_id: String,
    pub user_id: String,
    pub display_name: String,
    pub embedding_blob: Vec<u8>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[async_trait::async_trait]
pub trait IMeetingRepository: Send + Sync {
    async fn insert_session(
        &self,
        params: &InsertMeetingSessionParams,
    ) -> Result<MeetingSessionRow, DbError>;

    async fn update_session(
        &self,
        session_id: &str,
        params: &UpdateMeetingSessionParams,
    ) -> Result<Option<MeetingSessionRow>, DbError>;

    async fn get_session(&self, session_id: &str) -> Result<Option<MeetingSessionRow>, DbError>;

    async fn list_sessions_for_owner(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<MeetingSessionRow>, DbError>;

    async fn upsert_segment(
        &self,
        params: &UpsertMeetingSegmentParams,
    ) -> Result<MeetingSegmentRow, DbError>;

    async fn list_segments(
        &self,
        session_id: &str,
    ) -> Result<Vec<MeetingSegmentRow>, DbError>;

    async fn search_segments(
        &self,
        session_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MeetingSegmentRow>, DbError>;

    async fn upsert_speaker(
        &self,
        params: &UpsertMeetingSpeakerParams,
    ) -> Result<MeetingSpeakerRow, DbError>;

    async fn list_speakers(
        &self,
        session_id: &str,
    ) -> Result<Vec<MeetingSpeakerRow>, DbError>;

    async fn upsert_voiceprint(
        &self,
        params: &UpsertMeetingVoiceprintParams,
    ) -> Result<MeetingVoiceprintRow, DbError>;

    async fn list_voiceprints(
        &self,
        user_id: &str,
    ) -> Result<Vec<MeetingVoiceprintRow>, DbError>;

    async fn delete_voiceprint(
        &self,
        user_id: &str,
        voiceprint_id: &str,
    ) -> Result<bool, DbError>;

    async fn clear_voiceprints(&self, user_id: &str) -> Result<u64, DbError>;
}

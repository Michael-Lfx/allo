use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::{
    MeetingSegmentRow, MeetingSessionRow, MeetingSpeakerRow, MeetingVoiceprintRow,
};
use crate::repository::meeting::{
    IMeetingRepository, InsertMeetingSessionParams, UpdateMeetingSessionParams,
    UpsertMeetingSegmentParams, UpsertMeetingSpeakerParams, UpsertMeetingVoiceprintParams,
};

const SESSION_COLUMNS: &str = "id, session_id, user_id, title, status, \
     bound_conversation_id, data_dir, mic_available, loopback_available, stt_backend, \
     started_at, ended_at, created_at, updated_at";

const SEGMENT_COLUMNS: &str = "id, session_id, segment_id, channel, speaker_id, speaker_label, \
     text, is_partial, is_manual_edit, start_ms, end_ms, created_at, updated_at";

const SPEAKER_COLUMNS: &str =
    "id, session_id, speaker_id, display_name, voiceprint_id, created_at, updated_at";

const VOICEPRINT_COLUMNS: &str =
    "id, voiceprint_id, user_id, display_name, embedding_blob, created_at, updated_at";

#[derive(Clone, Debug)]
pub struct SqliteMeetingRepository {
    pool: SqlitePool,
}

impl SqliteMeetingRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl IMeetingRepository for SqliteMeetingRepository {
    async fn insert_session(
        &self,
        params: &InsertMeetingSessionParams,
    ) -> Result<MeetingSessionRow, DbError> {
        let sql = format!(
            "INSERT INTO meeting_sessions (\
                session_id, user_id, title, status, bound_conversation_id, data_dir, \
                mic_available, loopback_available, stt_backend, started_at, ended_at, \
                created_at, updated_at\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
            RETURNING {SESSION_COLUMNS}"
        );
        let row = sqlx::query_as::<_, MeetingSessionRow>(&sql)
            .bind(&params.session_id)
            .bind(&params.user_id)
            .bind(&params.title)
            .bind(&params.status)
            .bind(&params.bound_conversation_id)
            .bind(&params.data_dir)
            .bind(i64::from(params.mic_available))
            .bind(i64::from(params.loopback_available))
            .bind(&params.stt_backend)
            .bind(params.started_at)
            .bind(params.ended_at)
            .bind(params.created_at)
            .bind(params.updated_at)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn update_session(
        &self,
        session_id: &str,
        params: &UpdateMeetingSessionParams,
    ) -> Result<Option<MeetingSessionRow>, DbError> {
        let existing = self.get_session(session_id).await?;
        let Some(existing) = existing else {
            return Ok(None);
        };

        let title = params.title.clone().unwrap_or(existing.title);
        let status = params.status.clone().unwrap_or(existing.status);
        let bound = match &params.bound_conversation_id {
            Some(v) => v.clone(),
            None => existing.bound_conversation_id,
        };
        let mic = params
            .mic_available
            .map(i64::from)
            .unwrap_or(existing.mic_available);
        let loopback = params
            .loopback_available
            .map(i64::from)
            .unwrap_or(existing.loopback_available);
        let stt = params
            .stt_backend
            .clone()
            .unwrap_or(existing.stt_backend);
        let started = match params.started_at {
            Some(v) => v,
            None => existing.started_at,
        };
        let ended = match params.ended_at {
            Some(v) => v,
            None => existing.ended_at,
        };

        let sql = format!(
            "UPDATE meeting_sessions SET \
                title = ?, status = ?, bound_conversation_id = ?, mic_available = ?, \
                loopback_available = ?, stt_backend = ?, started_at = ?, ended_at = ?, \
                updated_at = ? \
             WHERE session_id = ? \
             RETURNING {SESSION_COLUMNS}"
        );
        let row = sqlx::query_as::<_, MeetingSessionRow>(&sql)
            .bind(title)
            .bind(status)
            .bind(bound)
            .bind(mic)
            .bind(loopback)
            .bind(stt)
            .bind(started)
            .bind(ended)
            .bind(params.updated_at)
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<MeetingSessionRow>, DbError> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM meeting_sessions WHERE session_id = ?");
        let row = sqlx::query_as::<_, MeetingSessionRow>(&sql)
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    async fn list_sessions_for_owner(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<MeetingSessionRow>, DbError> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM meeting_sessions \
             WHERE user_id = ? ORDER BY created_at DESC LIMIT ?"
        );
        let rows = sqlx::query_as::<_, MeetingSessionRow>(&sql)
            .bind(user_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn upsert_segment(
        &self,
        params: &UpsertMeetingSegmentParams,
    ) -> Result<MeetingSegmentRow, DbError> {
        let sql = format!(
            "INSERT INTO meeting_segments (\
                session_id, segment_id, channel, speaker_id, speaker_label, text, \
                is_partial, is_manual_edit, start_ms, end_ms, created_at, updated_at\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
            ON CONFLICT(segment_id) DO UPDATE SET \
                channel = excluded.channel, \
                speaker_id = excluded.speaker_id, \
                speaker_label = excluded.speaker_label, \
                text = excluded.text, \
                is_partial = excluded.is_partial, \
                is_manual_edit = excluded.is_manual_edit, \
                start_ms = excluded.start_ms, \
                end_ms = excluded.end_ms, \
                updated_at = excluded.updated_at \
            RETURNING {SEGMENT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, MeetingSegmentRow>(&sql)
            .bind(&params.session_id)
            .bind(&params.segment_id)
            .bind(&params.channel)
            .bind(&params.speaker_id)
            .bind(&params.speaker_label)
            .bind(&params.text)
            .bind(i64::from(params.is_partial))
            .bind(i64::from(params.is_manual_edit))
            .bind(params.start_ms)
            .bind(params.end_ms)
            .bind(params.created_at)
            .bind(params.updated_at)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn list_segments(
        &self,
        session_id: &str,
    ) -> Result<Vec<MeetingSegmentRow>, DbError> {
        let sql = format!(
            "SELECT {SEGMENT_COLUMNS} FROM meeting_segments \
             WHERE session_id = ? ORDER BY start_ms ASC, id ASC"
        );
        let rows = sqlx::query_as::<_, MeetingSegmentRow>(&sql)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn search_segments(
        &self,
        session_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MeetingSegmentRow>, DbError> {
        let pattern = format!("%{query}%");
        let sql = format!(
            "SELECT {SEGMENT_COLUMNS} FROM meeting_segments \
             WHERE session_id = ? AND text LIKE ? \
             ORDER BY start_ms ASC LIMIT ?"
        );
        let rows = sqlx::query_as::<_, MeetingSegmentRow>(&sql)
            .bind(session_id)
            .bind(pattern)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn upsert_speaker(
        &self,
        params: &UpsertMeetingSpeakerParams,
    ) -> Result<MeetingSpeakerRow, DbError> {
        let sql = format!(
            "INSERT INTO meeting_speakers (\
                session_id, speaker_id, display_name, voiceprint_id, created_at, updated_at\
            ) VALUES (?, ?, ?, ?, ?, ?) \
            ON CONFLICT(session_id, speaker_id) DO UPDATE SET \
                display_name = excluded.display_name, \
                voiceprint_id = excluded.voiceprint_id, \
                updated_at = excluded.updated_at \
            RETURNING {SPEAKER_COLUMNS}"
        );
        let row = sqlx::query_as::<_, MeetingSpeakerRow>(&sql)
            .bind(&params.session_id)
            .bind(&params.speaker_id)
            .bind(&params.display_name)
            .bind(&params.voiceprint_id)
            .bind(params.created_at)
            .bind(params.updated_at)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn list_speakers(
        &self,
        session_id: &str,
    ) -> Result<Vec<MeetingSpeakerRow>, DbError> {
        let sql = format!(
            "SELECT {SPEAKER_COLUMNS} FROM meeting_speakers \
             WHERE session_id = ? ORDER BY id ASC"
        );
        let rows = sqlx::query_as::<_, MeetingSpeakerRow>(&sql)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn upsert_voiceprint(
        &self,
        params: &UpsertMeetingVoiceprintParams,
    ) -> Result<MeetingVoiceprintRow, DbError> {
        let sql = format!(
            "INSERT INTO meeting_voiceprints (\
                voiceprint_id, user_id, display_name, embedding_blob, created_at, updated_at\
            ) VALUES (?, ?, ?, ?, ?, ?) \
            ON CONFLICT(voiceprint_id) DO UPDATE SET \
                display_name = excluded.display_name, \
                embedding_blob = excluded.embedding_blob, \
                updated_at = excluded.updated_at \
            RETURNING {VOICEPRINT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, MeetingVoiceprintRow>(&sql)
            .bind(&params.voiceprint_id)
            .bind(&params.user_id)
            .bind(&params.display_name)
            .bind(&params.embedding_blob)
            .bind(params.created_at)
            .bind(params.updated_at)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn list_voiceprints(
        &self,
        user_id: &str,
    ) -> Result<Vec<MeetingVoiceprintRow>, DbError> {
        let sql = format!(
            "SELECT {VOICEPRINT_COLUMNS} FROM meeting_voiceprints \
             WHERE user_id = ? ORDER BY updated_at DESC"
        );
        let rows = sqlx::query_as::<_, MeetingVoiceprintRow>(&sql)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn delete_voiceprint(
        &self,
        user_id: &str,
        voiceprint_id: &str,
    ) -> Result<bool, DbError> {
        let result = sqlx::query(
            "DELETE FROM meeting_voiceprints WHERE user_id = ? AND voiceprint_id = ?",
        )
        .bind(user_id)
        .bind(voiceprint_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn clear_voiceprints(&self, user_id: &str) -> Result<u64, DbError> {
        let result = sqlx::query("DELETE FROM meeting_voiceprints WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

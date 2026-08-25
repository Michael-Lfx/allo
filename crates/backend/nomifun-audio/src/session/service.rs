//! Meeting session service: persist + in-process E1 event fan-out.

use std::sync::Arc;

use nomifun_db::{
    IMeetingRepository, InsertMeetingSessionParams, MeetingSegmentRow, MeetingSessionRow,
    UpdateMeetingSessionParams, UpsertMeetingSegmentParams,
};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::frame::AudioChannel;
use crate::session::types::{
    CreateMeetingSessionRequest, MeetingEvent, MeetingSegmentSnapshot, MeetingSessionSnapshot,
    MeetingSessionStatus, SttBackendChoice,
};

#[derive(Clone)]
pub struct MeetingSessionService {
    repo: Arc<dyn IMeetingRepository>,
    events: broadcast::Sender<MeetingEvent>,
}

impl MeetingSessionService {
    pub fn new(repo: Arc<dyn IMeetingRepository>) -> Self {
        let (events, _) = broadcast::channel(256);
        Self { repo, events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MeetingEvent> {
        self.events.subscribe()
    }

    pub async fn create_session(
        &self,
        req: CreateMeetingSessionRequest,
    ) -> Result<MeetingSessionSnapshot, String> {
        let now = now_ms();
        let session_id = Uuid::now_v7().to_string();
        let row = self
            .repo
            .insert_session(&InsertMeetingSessionParams {
                session_id,
                user_id: req.user_id,
                title: req.title,
                status: MeetingSessionStatus::Created.as_str().to_string(),
                bound_conversation_id: req.bound_conversation_id,
                data_dir: req.data_dir,
                mic_available: req.mic_available,
                loopback_available: req.loopback_available,
                stt_backend: req.stt_backend.as_str().to_string(),
                started_at: None,
                ended_at: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .map_err(|e| e.to_string())?;
        let snap = snapshot_from_row(row)?;
        self.publish(MeetingEvent::SessionUpdated {
            session: snap.clone(),
        });
        Ok(snap)
    }

    pub async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<Option<MeetingSessionSnapshot>, String> {
        let row = self
            .repo
            .get_session(session_id)
            .await
            .map_err(|e| e.to_string())?;
        row.map(snapshot_from_row).transpose()
    }

    pub async fn list_sessions(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<MeetingSessionSnapshot>, String> {
        let rows = self
            .repo
            .list_sessions_for_owner(user_id, limit)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter().map(snapshot_from_row).collect()
    }

    pub async fn set_status(
        &self,
        session_id: &str,
        status: MeetingSessionStatus,
    ) -> Result<MeetingSessionSnapshot, String> {
        let now = now_ms();
        let mut params = UpdateMeetingSessionParams {
            title: None,
            status: Some(status.as_str().to_string()),
            bound_conversation_id: None,
            mic_available: None,
            loopback_available: None,
            stt_backend: None,
            started_at: None,
            ended_at: None,
            updated_at: now,
        };
        match status {
            MeetingSessionStatus::Recording => {
                params.started_at = Some(Some(now));
            }
            MeetingSessionStatus::Stopped | MeetingSessionStatus::Failed => {
                params.ended_at = Some(Some(now));
            }
            _ => {}
        }
        let row = self
            .repo
            .update_session(session_id, &params)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("meeting session not found: {session_id}"))?;
        let snap = snapshot_from_row(row)?;
        self.publish(MeetingEvent::SessionUpdated {
            session: snap.clone(),
        });
        Ok(snap)
    }

    pub async fn bind_conversation(
        &self,
        session_id: &str,
        conversation_id: Option<String>,
    ) -> Result<MeetingSessionSnapshot, String> {
        let row = self
            .repo
            .update_session(
                session_id,
                &UpdateMeetingSessionParams {
                    title: None,
                    status: None,
                    bound_conversation_id: Some(conversation_id),
                    mic_available: None,
                    loopback_available: None,
                    stt_backend: None,
                    started_at: None,
                    ended_at: None,
                    updated_at: now_ms(),
                },
            )
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("meeting session not found: {session_id}"))?;
        let snap = snapshot_from_row(row)?;
        self.publish(MeetingEvent::SessionUpdated {
            session: snap.clone(),
        });
        Ok(snap)
    }

    pub async fn upsert_segment(
        &self,
        segment: MeetingSegmentSnapshot,
    ) -> Result<MeetingSegmentSnapshot, String> {
        let now = now_ms();
        let row = self
            .repo
            .upsert_segment(&UpsertMeetingSegmentParams {
                session_id: segment.session_id.clone(),
                segment_id: segment.segment_id.clone(),
                channel: segment.channel.map(|c| c.label().to_string()),
                speaker_id: segment.speaker_id.clone(),
                speaker_label: segment.speaker_label.clone(),
                text: segment.text.clone(),
                is_partial: segment.is_partial,
                is_manual_edit: segment.is_manual_edit,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                created_at: now,
                updated_at: now,
            })
            .await
            .map_err(|e| e.to_string())?;
        let snap = segment_from_row(row)?;
        self.publish(MeetingEvent::SegmentUpserted {
            segment: snap.clone(),
        });
        Ok(snap)
    }

    pub async fn list_segments(
        &self,
        session_id: &str,
    ) -> Result<Vec<MeetingSegmentSnapshot>, String> {
        let rows = self
            .repo
            .list_segments(session_id)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter().map(segment_from_row).collect()
    }

    pub async fn search_segments(
        &self,
        session_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MeetingSegmentSnapshot>, String> {
        let rows = self
            .repo
            .search_segments(session_id, query, limit)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter().map(segment_from_row).collect()
    }

    pub fn publish_capability_degraded(
        &self,
        session_id: impl Into<String>,
        mic_available: bool,
        loopback_available: bool,
        message: impl Into<String>,
    ) {
        self.publish(MeetingEvent::CapabilityDegraded {
            session_id: session_id.into(),
            mic_available,
            loopback_available,
            message: message.into(),
        });
    }

    fn publish(&self, event: MeetingEvent) {
        let _ = self.events.send(event);
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn snapshot_from_row(row: MeetingSessionRow) -> Result<MeetingSessionSnapshot, String> {
    Ok(MeetingSessionSnapshot {
        session_id: row.session_id,
        user_id: row.user_id,
        title: row.title,
        status: MeetingSessionStatus::parse(&row.status)
            .ok_or_else(|| format!("unknown meeting status: {}", row.status))?,
        bound_conversation_id: row.bound_conversation_id,
        data_dir: row.data_dir,
        mic_available: row.mic_available != 0,
        loopback_available: row.loopback_available != 0,
        stt_backend: SttBackendChoice::parse(&row.stt_backend)
            .ok_or_else(|| format!("unknown stt backend: {}", row.stt_backend))?,
        started_at_ms: row.started_at,
        ended_at_ms: row.ended_at,
        created_at_ms: row.created_at,
        updated_at_ms: row.updated_at,
    })
}

fn segment_from_row(row: MeetingSegmentRow) -> Result<MeetingSegmentSnapshot, String> {
    let channel = match row.channel.as_deref() {
        None => None,
        Some("mic") => Some(AudioChannel::Mic),
        Some("loopback") => Some(AudioChannel::Loopback),
        Some(other) => return Err(format!("unknown audio channel: {other}")),
    };
    Ok(MeetingSegmentSnapshot {
        segment_id: row.segment_id,
        session_id: row.session_id,
        channel,
        speaker_id: row.speaker_id,
        speaker_label: row.speaker_label,
        text: row.text,
        is_partial: row.is_partial != 0,
        is_manual_edit: row.is_manual_edit != 0,
        start_ms: row.start_ms,
        end_ms: row.end_ms,
    })
}

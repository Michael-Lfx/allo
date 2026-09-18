//! E1 fan-out: `MeetingSessionService::subscribe()` → `UserEventSink` as
//! `WebSocketMessage::new("meeting:event", …)`.

use std::sync::Arc;

use nomifun_api_types::WebSocketMessage;
use nomifun_realtime::UserEventSink;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

use crate::session::{MeetingEvent, MeetingSessionService};

/// Bridge in-process meeting events onto the owner's realtime channel.
pub fn spawn_meeting_event_bridge(
    service: MeetingSessionService,
    user_events: Arc<dyn UserEventSink>,
) -> JoinHandle<()> {
    let mut rx = service.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    deliver(&service, user_events.as_ref(), event).await;
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        skipped,
                        "meeting event bridge lagged; continuing from newest"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("meeting event bridge closed");
                    break;
                }
            }
        }
    })
}

async fn deliver(
    service: &MeetingSessionService,
    sink: &dyn UserEventSink,
    event: MeetingEvent,
) {
    let user_id = match resolve_owner(service, &event).await {
        Some(id) => id,
        None => {
            warn!(
                session_id = %event.session_id(),
                "meeting event dropped: owner unresolved"
            );
            return;
        }
    };
    let value = match serde_json::to_value(&event) {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "meeting event serialize failed");
            return;
        }
    };
    sink.send_to_user(
        &user_id,
        WebSocketMessage::new(MeetingEvent::WS_NAME, value),
    );
}

async fn resolve_owner(service: &MeetingSessionService, event: &MeetingEvent) -> Option<String> {
    match event {
        MeetingEvent::SessionUpdated { session } => Some(session.user_id.clone()),
        MeetingEvent::SegmentUpserted { segment } => service
            .get_session(&segment.session_id)
            .await
            .ok()
            .flatten()
            .map(|s| s.user_id),
        MeetingEvent::CapabilityDegraded { session_id, .. }
        | MeetingEvent::Error { session_id, .. } => service
            .get_session(session_id)
            .await
            .ok()
            .flatten()
            .map(|s| s.user_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{
        CreateMeetingSessionRequest, MeetingSessionStatus, SttBackendChoice,
    };
    use async_trait::async_trait;
    use nomifun_db::{
        DbError, IMeetingRepository, InsertMeetingSessionParams, MeetingSegmentRow,
        MeetingSessionRow, MeetingSpeakerRow, MeetingVoiceprintRow, UpdateMeetingSessionParams,
        UpsertMeetingSegmentParams, UpsertMeetingSpeakerParams, UpsertMeetingVoiceprintParams,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink {
        deliveries: Mutex<Vec<(String, WebSocketMessage<serde_json::Value>)>>,
    }

    impl UserEventSink for RecordingSink {
        fn send_to_user(&self, user_id: &str, event: WebSocketMessage<serde_json::Value>) {
            self.deliveries
                .lock()
                .unwrap()
                .push((user_id.to_owned(), event));
        }
    }

    struct MemRepo {
        sessions: Mutex<Vec<MeetingSessionRow>>,
        next_id: Mutex<i64>,
    }

    impl MemRepo {
        fn new() -> Self {
            Self {
                sessions: Mutex::new(Vec::new()),
                next_id: Mutex::new(1),
            }
        }
    }

    #[async_trait]
    impl IMeetingRepository for MemRepo {
        async fn insert_session(
            &self,
            params: &InsertMeetingSessionParams,
        ) -> Result<MeetingSessionRow, DbError> {
            let mut id = self.next_id.lock().unwrap();
            let row = MeetingSessionRow {
                id: *id,
                session_id: params.session_id.clone(),
                user_id: params.user_id.clone(),
                title: params.title.clone(),
                status: params.status.clone(),
                bound_conversation_id: params.bound_conversation_id.clone(),
                data_dir: params.data_dir.clone(),
                mic_available: if params.mic_available { 1 } else { 0 },
                loopback_available: if params.loopback_available { 1 } else { 0 },
                stt_backend: params.stt_backend.clone(),
                started_at: params.started_at,
                ended_at: params.ended_at,
                notes_json: None,
                notes_status: "none".into(),
                created_at: params.created_at,
                updated_at: params.updated_at,
            };
            *id += 1;
            self.sessions.lock().unwrap().push(row.clone());
            Ok(row)
        }
        async fn update_session(
            &self,
            session_id: &str,
            params: &UpdateMeetingSessionParams,
        ) -> Result<Option<MeetingSessionRow>, DbError> {
            let mut sessions = self.sessions.lock().unwrap();
            let Some(row) = sessions.iter_mut().find(|s| s.session_id == session_id) else {
                return Ok(None);
            };
            if let Some(title) = &params.title {
                row.title = title.clone();
            }
            if let Some(status) = &params.status {
                row.status = status.clone();
            }
            if let Some(notes_json) = &params.notes_json {
                row.notes_json = notes_json.clone();
            }
            if let Some(notes_status) = &params.notes_status {
                row.notes_status = notes_status.clone();
            }
            row.updated_at = params.updated_at;
            Ok(Some(row.clone()))
        }
        async fn get_session(
            &self,
            session_id: &str,
        ) -> Result<Option<MeetingSessionRow>, DbError> {
            Ok(self
                .sessions
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.session_id == session_id)
                .cloned())
        }
        async fn list_sessions_for_owner(
            &self,
            _user_id: &str,
            _limit: i64,
        ) -> Result<Vec<MeetingSessionRow>, DbError> {
            Ok(vec![])
        }
        async fn upsert_segment(
            &self,
            _params: &UpsertMeetingSegmentParams,
        ) -> Result<MeetingSegmentRow, DbError> {
            unimplemented!()
        }
        async fn list_segments(
            &self,
            _session_id: &str,
        ) -> Result<Vec<MeetingSegmentRow>, DbError> {
            Ok(vec![])
        }
        async fn search_segments(
            &self,
            _session_id: &str,
            _query: &str,
            _limit: i64,
        ) -> Result<Vec<MeetingSegmentRow>, DbError> {
            Ok(vec![])
        }
        async fn upsert_speaker(
            &self,
            _params: &UpsertMeetingSpeakerParams,
        ) -> Result<MeetingSpeakerRow, DbError> {
            unimplemented!()
        }
        async fn list_speakers(
            &self,
            _session_id: &str,
        ) -> Result<Vec<MeetingSpeakerRow>, DbError> {
            Ok(vec![])
        }
        async fn upsert_voiceprint(
            &self,
            _params: &UpsertMeetingVoiceprintParams,
        ) -> Result<MeetingVoiceprintRow, DbError> {
            unimplemented!()
        }
        async fn list_voiceprints(
            &self,
            _user_id: &str,
        ) -> Result<Vec<MeetingVoiceprintRow>, DbError> {
            Ok(vec![])
        }
        async fn delete_voiceprint(
            &self,
            _user_id: &str,
            _voiceprint_id: &str,
        ) -> Result<bool, DbError> {
            Ok(false)
        }
        async fn clear_voiceprints(&self, _user_id: &str) -> Result<u64, DbError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn bridge_emits_meeting_event_to_owner() {
        let tmp = tempfile::TempDir::new().unwrap();
        let service = MeetingSessionService::new(Arc::new(MemRepo::new()));
        let sink = Arc::new(RecordingSink::default());
        let _bridge = spawn_meeting_event_bridge(service.clone(), sink.clone());

        let snap = service
            .create_session(CreateMeetingSessionRequest {
                user_id: "owner-1".into(),
                title: "Standup".into(),
                data_dir: tmp.path().to_string_lossy().into_owned(),
                bound_conversation_id: None,
                stt_backend: SttBackendChoice::Auto,
                mic_available: true,
                loopback_available: true,
            })
            .await
            .unwrap();
        assert_eq!(snap.status, MeetingSessionStatus::Created);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let deliveries = sink.deliveries.lock().unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].0, "owner-1");
        assert_eq!(deliveries[0].1.name, MeetingEvent::WS_NAME);
        assert_eq!(deliveries[0].1.data["type"], "session_updated");
    }
}

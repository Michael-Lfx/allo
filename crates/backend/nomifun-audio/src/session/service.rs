//! Meeting session service: persist + in-process E1 event fan-out.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use nomifun_common::RequirementCreator;
use nomifun_db::{
    IMeetingRepository, InsertMeetingSessionParams, MeetingSegmentRow, MeetingSessionRow,
    UpdateMeetingSessionParams, UpsertMeetingSegmentParams,
};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::frame::AudioChannel;
use crate::session::notes::{
    self, GenerateMeetingNotesResult, MeetingNotesCompleter, MeetingNotesConversationSink,
    MeetingNotesStatus, MeetingNotesView,
};
use crate::session::types::{
    CreateMeetingSessionRequest, MeetingEvent, MeetingSegmentSnapshot, MeetingSessionSnapshot,
    MeetingSessionStatus, SttBackendChoice,
};

/// Latest caption line for tray tooltip / floating overlay (in-memory).
#[derive(Debug, Clone, Default)]
pub struct LatestMeetingCaption {
    pub session_id: String,
    pub text: String,
    pub speaker_label: String,
    pub is_partial: bool,
}

#[derive(Clone)]
pub struct MeetingSessionService {
    repo: Arc<dyn IMeetingRepository>,
    events: broadcast::Sender<MeetingEvent>,
    notes_completer: Arc<RwLock<Option<Arc<dyn MeetingNotesCompleter>>>>,
    conversation_sink: Arc<RwLock<Option<Arc<dyn MeetingNotesConversationSink>>>>,
    requirement_creator: Arc<RwLock<Option<Arc<dyn RequirementCreator>>>>,
    latest_captions: Arc<Mutex<HashMap<String, LatestMeetingCaption>>>,
}

impl MeetingSessionService {
    pub fn new(repo: Arc<dyn IMeetingRepository>) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            repo,
            events,
            notes_completer: Arc::new(RwLock::new(None)),
            conversation_sink: Arc::new(RwLock::new(None)),
            requirement_creator: Arc::new(RwLock::new(None)),
            latest_captions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_notes_completer(&self, completer: Arc<dyn MeetingNotesCompleter>) {
        if let Ok(mut g) = self.notes_completer.write() {
            *g = Some(completer);
        }
    }

    pub fn set_conversation_sink(&self, sink: Arc<dyn MeetingNotesConversationSink>) {
        if let Ok(mut g) = self.conversation_sink.write() {
            *g = Some(sink);
        }
    }

    pub fn set_requirement_creator(&self, creator: Arc<dyn RequirementCreator>) {
        if let Ok(mut g) = self.requirement_creator.write() {
            *g = Some(creator);
        }
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
        // Callers pass a meetings root; each session gets its own subdirectory.
        let data_dir = {
            let path = std::path::PathBuf::from(&req.data_dir).join(&session_id);
            if let Err(e) = std::fs::create_dir_all(&path) {
                tracing::warn!(error = %e, path = %path.display(), "meeting data_dir create failed");
            }
            path.to_string_lossy().into_owned()
        };
        let row = self
            .repo
            .insert_session(&InsertMeetingSessionParams {
                session_id,
                user_id: req.user_id,
                title: req.title,
                status: MeetingSessionStatus::Created.as_str().to_string(),
                bound_conversation_id: req.bound_conversation_id,
                data_dir,
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
            notes_json: None,
            notes_status: None,
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

    pub async fn update_title(
        &self,
        session_id: &str,
        title: String,
    ) -> Result<MeetingSessionSnapshot, String> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err("meeting title is required".into());
        }
        let row = self
            .repo
            .update_session(
                session_id,
                &UpdateMeetingSessionParams {
                    title: Some(title),
                    status: None,
                    bound_conversation_id: None,
                    mic_available: None,
                    loopback_available: None,
                    stt_backend: None,
                    started_at: None,
                    ended_at: None,
                    notes_json: None,
                    notes_status: None,
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
                    notes_json: None,
                    notes_status: None,
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
        self.remember_caption(&snap);
        self.publish(MeetingEvent::SegmentUpserted {
            segment: snap.clone(),
        });
        Ok(snap)
    }

    fn remember_caption(&self, segment: &MeetingSegmentSnapshot) {
        if segment.text.trim().is_empty() {
            return;
        }
        if let Ok(mut map) = self.latest_captions.lock() {
            map.insert(
                segment.session_id.clone(),
                LatestMeetingCaption {
                    session_id: segment.session_id.clone(),
                    text: segment.text.clone(),
                    speaker_label: segment.speaker_label.clone(),
                    is_partial: segment.is_partial,
                },
            );
        }
    }

    /// Latest caption line for a session (tray / floating overlay).
    pub fn latest_caption(&self, session_id: &str) -> Option<LatestMeetingCaption> {
        self.latest_captions
            .lock()
            .ok()
            .and_then(|m| m.get(session_id).cloned())
    }

    /// Recent transcript lines including live partials (newest last).
    pub async fn captions_recent(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<MeetingSegmentSnapshot>, String> {
        let mut items = self.list_segments(session_id).await?;
        items.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then(a.end_ms.cmp(&b.end_ms)));
        let limit = limit.max(1) as usize;
        if items.len() > limit {
            items = items.split_off(items.len() - limit);
        }
        Ok(items)
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

    pub async fn update_capture_availability(
        &self,
        session_id: &str,
        mic_available: bool,
        loopback_available: bool,
    ) -> Result<MeetingSessionSnapshot, String> {
        let row = self
            .repo
            .update_session(
                session_id,
                &UpdateMeetingSessionParams {
                    title: None,
                    status: None,
                    bound_conversation_id: None,
                    mic_available: Some(mic_available),
                    loopback_available: Some(loopback_available),
                    stt_backend: None,
                    started_at: None,
                    ended_at: None,
                    notes_json: None,
                    notes_status: None,
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

    pub async fn get_notes(&self, session_id: &str) -> Result<MeetingNotesView, String> {
        let session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| format!("meeting session not found: {session_id}"))?;
        Ok(MeetingNotesView {
            status: session.notes_status,
            notes: session.notes,
        })
    }

    /// Generate structured notes from the session transcript (LLM with template fallback),
    /// persist on the session, post to the bound conversation, and auto-create requirements.
    pub async fn generate_notes(
        &self,
        session_id: &str,
    ) -> Result<(MeetingSessionSnapshot, GenerateMeetingNotesResult), String> {
        let session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| format!("meeting session not found: {session_id}"))?;

        let _ = self
            .repo
            .update_session(
                session_id,
                &UpdateMeetingSessionParams {
                    title: None,
                    status: None,
                    bound_conversation_id: None,
                    mic_available: None,
                    loopback_available: None,
                    stt_backend: None,
                    started_at: None,
                    ended_at: None,
                    notes_json: None,
                    notes_status: Some(MeetingNotesStatus::Generating.as_str().to_string()),
                    updated_at: now_ms(),
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        let segments = self.list_segments(session_id).await?;
        let pairs: Vec<(String, String)> = segments
            .into_iter()
            .filter(|s| !s.is_partial)
            .map(|s| (s.speaker_label, s.text))
            .collect();
        let transcript = notes::build_transcript(&pairs);
        let now = now_ms();

        let completer = self
            .notes_completer
            .read()
            .ok()
            .and_then(|g| g.clone());
        let notes = match completer {
            Some(completer) => {
                match completer
                    .complete(notes::NOTES_SYSTEM, &transcript)
                    .await
                {
                    Ok(raw) => notes::parse_llm_notes(&raw, now)
                        .unwrap_or_else(|| notes::template_notes_from_transcript(&transcript, now)),
                    Err(err) => {
                        tracing::warn!(error = %err, session_id, "meeting notes LLM failed; using template");
                        notes::template_notes_from_transcript(&transcript, now)
                    }
                }
            }
            None => notes::template_notes_from_transcript(&transcript, now),
        };

        let notes_json = serde_json::to_string(&notes).map_err(|e| e.to_string())?;
        let notes_path = std::path::PathBuf::from(&session.data_dir).join("notes.json");
        if let Err(e) = std::fs::write(&notes_path, &notes_json) {
            tracing::warn!(
                error = %e,
                path = %notes_path.display(),
                "meeting notes.json write failed"
            );
        }

        let row = self
            .repo
            .update_session(
                session_id,
                &UpdateMeetingSessionParams {
                    title: None,
                    status: None,
                    bound_conversation_id: None,
                    mic_available: None,
                    loopback_available: None,
                    stt_backend: None,
                    started_at: None,
                    ended_at: None,
                    notes_json: Some(Some(notes_json)),
                    notes_status: Some(MeetingNotesStatus::Ready.as_str().to_string()),
                    updated_at: now_ms(),
                },
            )
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("meeting session not found: {session_id}"))?;

        let mut posted_to_conversation = false;
        let sink = self
            .conversation_sink
            .read()
            .ok()
            .and_then(|g| g.clone());
        if let (Some(conv_id), Some(sink)) = (session.bound_conversation_id.as_ref(), sink) {
            let markdown = notes::notes_to_markdown(&notes);
            match sink.post_notes(conv_id, &markdown).await {
                Ok(()) => posted_to_conversation = true,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        session_id,
                        conversation_id = %conv_id,
                        "failed to post meeting notes to conversation"
                    );
                }
            }
        }

        let mut created_requirement_ids = Vec::new();
        let creator = self
            .requirement_creator
            .read()
            .ok()
            .and_then(|g| g.clone());
        if let Some(creator) = creator {
            for todo in &notes.todos {
                let content = if todo.detail.trim().is_empty() {
                    format!("From meeting {session_id}")
                } else {
                    format!("{}\n\nFrom meeting {session_id}", todo.detail)
                };
                match creator
                    .create_from_message(&todo.title, &content, "inbox", "meeting_notes")
                    .await
                {
                    Ok(id) => created_requirement_ids.push(id),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            session_id,
                            title = %todo.title,
                            "failed to create requirement from meeting todo"
                        );
                    }
                }
            }
        }

        let snap = snapshot_from_row(row)?;
        self.publish(MeetingEvent::SessionUpdated {
            session: snap.clone(),
        });
        Ok((
            snap,
            GenerateMeetingNotesResult {
                notes,
                posted_to_conversation,
                created_requirement_ids,
            },
        ))
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

    pub fn publish_error(&self, session_id: impl Into<String>, message: impl Into<String>) {
        self.publish(MeetingEvent::Error {
            session_id: session_id.into(),
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
    let notes_status = MeetingNotesStatus::parse(&row.notes_status)
        .ok_or_else(|| format!("unknown meeting notes status: {}", row.notes_status))?;
    let notes = notes::parse_stored_notes(row.notes_json.as_deref());
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
        notes_status,
        notes,
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

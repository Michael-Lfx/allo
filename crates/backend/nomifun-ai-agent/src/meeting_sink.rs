//! Production `MeetingSink` over `MeetingSessionService` + `MeetingRuntime`.
//! Same sessions as the HTTP meeting API / tray / Meeting page.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use nomi_agent::meeting_tools::{MeetingSessionSummary, MeetingSink, MeetingTranscriptHit};
use nomifun_audio::{
    AudioDeviceManager, CreateMeetingSessionRequest, DeviceKind, MeetingRuntime,
    MeetingSessionService, MeetingSessionSnapshot, SttBackendChoice,
};

/// Conversation-bound meeting tool backend.
pub struct LiveMeetingSink {
    service: MeetingSessionService,
    runtime: Arc<MeetingRuntime>,
    meetings_root: PathBuf,
    user_id: String,
    conversation_id: String,
    /// Desktop hosts only. Headless / web must keep this false so
    /// `meeting.start` rejects capture.
    capture_allowed: bool,
}

impl LiveMeetingSink {
    pub fn new(
        service: MeetingSessionService,
        runtime: Arc<MeetingRuntime>,
        meetings_root: PathBuf,
        user_id: impl Into<String>,
        conversation_id: impl Into<String>,
        capture_allowed: bool,
    ) -> Self {
        Self {
            service,
            runtime,
            meetings_root,
            user_id: user_id.into(),
            conversation_id: conversation_id.into(),
            capture_allowed,
        }
    }

    pub fn into_arc(self) -> Arc<dyn MeetingSink> {
        Arc::new(self)
    }

    async fn require_owned(&self, session_id: &str) -> Result<MeetingSessionSnapshot, String> {
        let session = self
            .service
            .get_session(session_id)
            .await?
            .ok_or_else(|| format!("meeting session not found: {session_id}"))?;
        if session.user_id != self.user_id {
            return Err(format!("meeting session not found: {session_id}"));
        }
        Ok(session)
    }
}

fn to_summary(s: MeetingSessionSnapshot) -> MeetingSessionSummary {
    MeetingSessionSummary {
        session_id: s.session_id,
        title: s.title,
        status: s.status.as_str().to_string(),
        bound_conversation_id: s.bound_conversation_id,
        mic_available: s.mic_available,
        loopback_available: s.loopback_available,
        started_at_ms: s.started_at_ms,
        ended_at_ms: s.ended_at_ms,
    }
}

#[async_trait]
impl MeetingSink for LiveMeetingSink {
    async fn list(&self, limit: i64) -> Result<Vec<MeetingSessionSummary>, String> {
        let items = self.service.list_sessions(&self.user_id, limit).await?;
        Ok(items.into_iter().map(to_summary).collect())
    }

    async fn get(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
        Ok(to_summary(self.require_owned(session_id).await?))
    }

    async fn search_transcript(
        &self,
        session_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MeetingTranscriptHit>, String> {
        let _ = self.require_owned(session_id).await?;
        let rows = self
            .service
            .search_segments(session_id, query, limit)
            .await?;
        Ok(rows
            .into_iter()
            .map(|s| MeetingTranscriptHit {
                segment_id: s.segment_id,
                speaker_label: {
                    let label = s.speaker_label.trim();
                    if label.is_empty() {
                        None
                    } else {
                        Some(label.to_owned())
                    }
                },
                text: s.text,
                start_ms: Some(s.start_ms),
                end_ms: Some(s.end_ms),
                is_partial: s.is_partial,
            })
            .collect())
    }

    async fn get_notes(&self, session_id: &str) -> Result<String, String> {
        let _ = self.require_owned(session_id).await?;
        let view = self.service.get_notes(session_id).await?;
        match view.status {
            nomifun_audio::MeetingNotesStatus::None => Ok(
                "Notes status: none. Generate notes from the Meeting page (or wait until P2 automation)."
                    .into(),
            ),
            nomifun_audio::MeetingNotesStatus::Generating => {
                Ok("Notes status: generating. Try again shortly.".into())
            }
            nomifun_audio::MeetingNotesStatus::Failed => {
                Ok("Notes status: failed. Regenerate from the Meeting page.".into())
            }
            nomifun_audio::MeetingNotesStatus::Ready => {
                let Some(notes) = view.notes else {
                    return Ok("Notes status: ready but body is empty.".into());
                };
                let source = match notes.source {
                    nomifun_audio::MeetingNotesSource::Llm => "llm",
                    nomifun_audio::MeetingNotesSource::Template => "template",
                };
                let mut out = format!(
                    "Notes status: ready (source={source})\nSummary:\n{}\n",
                    notes.summary
                );
                if !notes.decisions.is_empty() {
                    out.push_str("\nDecisions:\n");
                    for d in &notes.decisions {
                        out.push_str(&format!("- {d}\n"));
                    }
                }
                if !notes.todos.is_empty() {
                    out.push_str("\nTodos:\n");
                    for t in &notes.todos {
                        out.push_str(&format!("- {}\n", t.title));
                    }
                }
                if !notes.risks.is_empty() {
                    out.push_str("\nRisks:\n");
                    for r in &notes.risks {
                        out.push_str(&format!("- {r}\n"));
                    }
                }
                Ok(out)
            }
        }
    }

    async fn start(
        &self,
        session_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<MeetingSessionSummary, String> {
        if !self.capture_allowed {
            return Err(
                "meeting.start is only available on Desktop with device permission; \
                 headless hosts reject capture start"
                    .into(),
            );
        }

        let session = if let Some(id) = session_id {
            self.require_owned(id).await?
        } else {
            let mgr = AudioDeviceManager::new();
            let mic_available = mgr
                .list_devices(DeviceKind::Input)
                .map(|d| !d.is_empty())
                .unwrap_or(false);
            let loopback_available = mgr
                .list_devices(DeviceKind::Output)
                .map(|d| !d.is_empty())
                .unwrap_or(false);
            self.service
                .create_session(CreateMeetingSessionRequest {
                    user_id: self.user_id.clone(),
                    title: title.unwrap_or("Meeting").to_string(),
                    data_dir: self.meetings_root.to_string_lossy().into_owned(),
                    bound_conversation_id: Some(self.conversation_id.clone()),
                    stt_backend: SttBackendChoice::Auto,
                    mic_available,
                    loopback_available,
                })
                .await?
        };

        let snap = self.runtime.start(session).await?;
        Ok(to_summary(snap))
    }

    async fn pause(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
        let _ = self.require_owned(session_id).await?;
        Ok(to_summary(self.runtime.pause(session_id).await?))
    }

    async fn resume(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
        let _ = self.require_owned(session_id).await?;
        Ok(to_summary(self.runtime.resume(session_id).await?))
    }

    async fn stop(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
        let _ = self.require_owned(session_id).await?;
        Ok(to_summary(self.runtime.stop(session_id).await?))
    }

    async fn ask(&self, session_id: &str, question: &str) -> Result<String, String> {
        let hits = self.search_transcript(session_id, question, 30).await?;
        if hits.is_empty() {
            return Ok(format!(
                "No transcript matches for question: {question}\n\
                 (Full listen-window answers land in P4.)"
            ));
        }
        let body: Vec<String> = hits
            .iter()
            .map(|h| {
                let speaker = h.speaker_label.as_deref().unwrap_or("?");
                format!("{speaker}: {}", h.text)
            })
            .collect();
        Ok(format!(
            "Question: {question}\nRelevant transcript:\n{}",
            body.join("\n")
        ))
    }
}

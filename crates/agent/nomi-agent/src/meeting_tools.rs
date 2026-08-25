//! Native meeting tools that drive the same `MeetingSession` as the HTTP API
//! through a `MeetingSink` trait object. The backend injects a concrete sink
//! over `MeetingSessionService` / `MeetingRuntime`; standalone `nomi-cli`
//! passes `None` and these are not registered. Mirrors `cron_tools`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::tool::{JsonSchema, ToolResult};

/// Summary of one meeting session for agent-facing list/get.
#[derive(Debug, Clone)]
pub struct MeetingSessionSummary {
    pub session_id: String,
    pub title: String,
    pub status: String,
    pub bound_conversation_id: Option<String>,
    pub mic_available: bool,
    pub loopback_available: bool,
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
}

/// One transcript segment hit.
#[derive(Debug, Clone)]
pub struct MeetingTranscriptHit {
    pub segment_id: String,
    pub speaker_label: Option<String>,
    pub text: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub is_partial: bool,
}

/// Backend seam for meeting session control + transcript access.
#[async_trait]
pub trait MeetingSink: Send + Sync {
    async fn list(&self, limit: i64) -> Result<Vec<MeetingSessionSummary>, String>;

    async fn get(&self, session_id: &str) -> Result<MeetingSessionSummary, String>;

    async fn search_transcript(
        &self,
        session_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MeetingTranscriptHit>, String>;

    /// Notes / summary stub until W11 / P2. Implementations may return a
    /// fixed "not ready" message.
    async fn get_notes(&self, session_id: &str) -> Result<String, String>;

    /// Start capture. Headless hosts must reject. When `session_id` is `None`,
    /// create a new session (optionally titled) then start it.
    async fn start(
        &self,
        session_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<MeetingSessionSummary, String>;

    async fn pause(&self, session_id: &str) -> Result<MeetingSessionSummary, String>;

    async fn resume(&self, session_id: &str) -> Result<MeetingSessionSummary, String>;

    async fn stop(&self, session_id: &str) -> Result<MeetingSessionSummary, String>;

    /// Answer a question from recent transcript + listen window when enabled.
    async fn ask(&self, session_id: &str, question: &str) -> Result<String, String>;

    /// Recent caption lines including live partials (oldest → newest).
    async fn captions_recent(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<MeetingTranscriptHit>, String>;

    /// Start agent listen mode for a meeting (passive context injection only).
    async fn listen_start(
        &self,
        session_id: &str,
        conversation_id: Option<&str>,
    ) -> Result<String, String>;

    /// Stop agent listen mode for a meeting.
    async fn listen_stop(&self, session_id: &str) -> Result<String, String>;
}

fn ok(content: String) -> ToolResult {
    ToolResult {
        content,
        is_error: false,
        images: Vec::new(),
    }
}

fn err(content: String) -> ToolResult {
    ToolResult {
        content,
        is_error: true,
        images: Vec::new(),
    }
}

fn format_session(s: &MeetingSessionSummary) -> String {
    format!(
        "session_id={} title={:?} status={} mic={} loopback={} bound_conversation_id={:?} started_at_ms={:?} ended_at_ms={:?}",
        s.session_id,
        s.title,
        s.status,
        s.mic_available,
        s.loopback_available,
        s.bound_conversation_id,
        s.started_at_ms,
        s.ended_at_ms
    )
}

fn format_hit(h: &MeetingTranscriptHit) -> String {
    let speaker = h.speaker_label.as_deref().unwrap_or("?");
    let t0 = h.start_ms.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    let t1 = h.end_ms.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    let partial = if h.is_partial { " partial" } else { "" };
    format!("[{t0}-{t1}ms] {speaker}: {}{partial}", h.text)
}

pub const MEETING_TOOL_NAMES: &[&str] = &[
    "meeting.list",
    "meeting.get",
    "meeting.search_transcript",
    "meeting.get_notes",
    "meeting.captions_recent",
    "meeting.start",
    "meeting.pause",
    "meeting.resume",
    "meeting.stop",
    "meeting.ask",
    "meeting.listen_start",
    "meeting.listen_stop",
];

/// `meeting.list` — list this owner's meeting sessions.
pub struct MeetingListTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingListTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingListTool {
    fn name(&self) -> &str {
        "meeting.list"
    }

    fn description(&self) -> &str {
        "List recent meeting recording sessions for this user (same sessions as the Meeting page \
         and tray). Returns session id, title, status, and device availability."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "description": "Max sessions to return (default 20)"
                }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let limit = input
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(20)
            .clamp(1, 100);
        match self.sink.list(limit).await {
            Ok(items) if items.is_empty() => ok("No meeting sessions.".into()),
            Ok(items) => {
                let lines: Vec<String> = items.iter().map(format_session).collect();
                ok(lines.join("\n"))
            }
            Err(e) => err(format!("meeting.list failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }
}

/// `meeting.get` — fetch one session by id.
pub struct MeetingGetTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingGetTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingGetTool {
    fn name(&self) -> &str {
        "meeting.get"
    }

    fn description(&self) -> &str {
        "Get one meeting session by session_id (status, title, device flags, timestamps). \
         Also returns a short preview of the latest captions when available via \
         meeting.captions_recent."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.get requires: session_id".into());
        };
        match self.sink.get(session_id).await {
            Ok(s) => {
                let mut out = format_session(&s);
                if let Ok(hits) = self.sink.captions_recent(session_id, 5).await {
                    if !hits.is_empty() {
                        out.push_str("\nRecent captions:\n");
                        for h in hits {
                            out.push_str(&format_hit(&h));
                            out.push('\n');
                        }
                    }
                }
                ok(out)
            }
            Err(e) => err(format!("meeting.get failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }
}

/// `meeting.search_transcript` — full-text search within a session transcript.
pub struct MeetingSearchTranscriptTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingSearchTranscriptTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingSearchTranscriptTool {
    fn name(&self) -> &str {
        "meeting.search_transcript"
    }

    fn description(&self) -> &str {
        "Search a meeting session's transcript for a query string. Returns matching segments \
         with speaker labels and timestamps."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" },
                "query": { "type": "string", "description": "Search text" },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "description": "Max hits (default 50)"
                }
            },
            "required": ["session_id", "query"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let (Some(session_id), Some(query)) = (
            input.get("session_id").and_then(|v| v.as_str()),
            input.get("query").and_then(|v| v.as_str()),
        ) else {
            return err("meeting.search_transcript requires: session_id, query".into());
        };
        let limit = input
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(50)
            .clamp(1, 100);
        match self.sink.search_transcript(session_id, query, limit).await {
            Ok(hits) if hits.is_empty() => ok("No matching transcript segments.".into()),
            Ok(hits) => {
                let lines: Vec<String> = hits.iter().map(format_hit).collect();
                ok(lines.join("\n"))
            }
            Err(e) => err(format!("meeting.search_transcript failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }
}

/// `meeting.get_notes` — meeting notes (stub until W11 / P2).
pub struct MeetingGetNotesTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingGetNotesTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingGetNotesTool {
    fn name(&self) -> &str {
        "meeting.get_notes"
    }

    fn description(&self) -> &str {
        "Get structured notes / summary for a meeting session when notes have been generated \
         (summary, decisions, todos, risks). Returns status if notes are not ready yet."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.get_notes requires: session_id".into());
        };
        match self.sink.get_notes(session_id).await {
            Ok(notes) => ok(notes),
            Err(e) => err(format!("meeting.get_notes failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }
}

/// `meeting.captions_recent` — recent transcript lines including live partials.
pub struct MeetingCaptionsRecentTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingCaptionsRecentTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingCaptionsRecentTool {
    fn name(&self) -> &str {
        "meeting.captions_recent"
    }

    fn description(&self) -> &str {
        "Fetch the most recent meeting caption / transcript lines for a session, including \
         live partial (in-progress) captions. Use this to poll streaming captions while a \
         meeting is recording."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "description": "Max lines to return (default 20)"
                }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.captions_recent requires: session_id".into());
        };
        let limit = input
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(20)
            .clamp(1, 100);
        match self.sink.captions_recent(session_id, limit).await {
            Ok(hits) if hits.is_empty() => ok("No captions yet.".into()),
            Ok(hits) => {
                let lines: Vec<String> = hits.iter().map(format_hit).collect();
                ok(lines.join("\n"))
            }
            Err(e) => err(format!("meeting.captions_recent failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }
}

/// `meeting.start` — create (optional) and start dual-track capture.
pub struct MeetingStartTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingStartTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingStartTool {
    fn name(&self) -> &str {
        "meeting.start"
    }

    fn description(&self) -> &str {
        "Start meeting dual-track (mic + loopback) recording on the Desktop host. Provide an \
         existing session_id, or omit it to create a new session (optional title). Headless \
         hosts reject this tool. Uses the same MeetingSession as the Meeting page and tray."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Existing session to start; omit to create a new one"
                },
                "title": {
                    "type": "string",
                    "description": "Title when creating a new session (ignored if session_id set)"
                }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let session_id = input.get("session_id").and_then(|v| v.as_str());
        let title = input.get("title").and_then(|v| v.as_str());
        match self.sink.start(session_id, title).await {
            Ok(s) => ok(format!("Started: {}", format_session(&s))),
            Err(e) => err(format!("meeting.start failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }
}

/// `meeting.pause`
pub struct MeetingPauseTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingPauseTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingPauseTool {
    fn name(&self) -> &str {
        "meeting.pause"
    }

    fn description(&self) -> &str {
        "Pause an active meeting recording session."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.pause requires: session_id".into());
        };
        match self.sink.pause(session_id).await {
            Ok(s) => ok(format!("Paused: {}", format_session(&s))),
            Err(e) => err(format!("meeting.pause failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }
}

/// `meeting.resume`
pub struct MeetingResumeTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingResumeTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingResumeTool {
    fn name(&self) -> &str {
        "meeting.resume"
    }

    fn description(&self) -> &str {
        "Resume a paused meeting recording session."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.resume requires: session_id".into());
        };
        match self.sink.resume(session_id).await {
            Ok(s) => ok(format!("Resumed: {}", format_session(&s))),
            Err(e) => err(format!("meeting.resume failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }
}

/// `meeting.stop`
pub struct MeetingStopTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingStopTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingStopTool {
    fn name(&self) -> &str {
        "meeting.stop"
    }

    fn description(&self) -> &str {
        "Stop a meeting recording session and finalize tracks."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.stop requires: session_id".into());
        };
        match self.sink.stop(session_id).await {
            Ok(s) => ok(format!("Stopped: {}", format_session(&s))),
            Err(e) => err(format!("meeting.stop failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }
}

/// `meeting.ask` — question over a session transcript (P1 search+concat).
pub struct MeetingAskTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingAskTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingAskTool {
    fn name(&self) -> &str {
        "meeting.ask"
    }

    fn description(&self) -> &str {
        "Ask a question about a meeting session's transcript. Returns matching segments \
         and, when listen mode is on, the recent listen window plus rolling summary."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" },
                "question": { "type": "string", "description": "Question to answer from the transcript" }
            },
            "required": ["session_id", "question"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let (Some(session_id), Some(question)) = (
            input.get("session_id").and_then(|v| v.as_str()),
            input.get("question").and_then(|v| v.as_str()),
        ) else {
            return err("meeting.ask requires: session_id, question".into());
        };
        match self.sink.ask(session_id, question).await {
            Ok(answer) => ok(answer),
            Err(e) => err(format!("meeting.ask failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }
}

/// `meeting.listen_start` — enable passive listen context for the bound conversation.
pub struct MeetingListenStartTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingListenStartTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingListenStartTool {
    fn name(&self) -> &str {
        "meeting.listen_start"
    }

    fn description(&self) -> &str {
        "Start Agent listen mode for a meeting session. Injects a rolling transcript \
         window + summary into the bound conversation on each user turn. Does NOT \
         proactively interrupt or start turns. Pass conversation_id or use the \
         session's already-bound conversation (same binding as meeting notes)."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" },
                "conversation_id": {
                    "type": "string",
                    "description": "Optional conversation to bind/listen; defaults to session bound conversation"
                }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.listen_start requires: session_id".into());
        };
        let conversation_id = input.get("conversation_id").and_then(|v| v.as_str());
        match self.sink.listen_start(session_id, conversation_id).await {
            Ok(msg) => ok(msg),
            Err(e) => err(format!("meeting.listen_start failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }
}

/// `meeting.listen_stop` — disable listen mode for a meeting session.
pub struct MeetingListenStopTool {
    sink: Arc<dyn MeetingSink>,
}

impl MeetingListenStopTool {
    pub fn new(sink: Arc<dyn MeetingSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for MeetingListenStopTool {
    fn name(&self) -> &str {
        "meeting.listen_stop"
    }

    fn description(&self) -> &str {
        "Stop Agent listen mode for a meeting session. Removes listen context injection \
         from the bound conversation."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Meeting session id" }
            },
            "required": ["session_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
            return err("meeting.listen_stop requires: session_id".into());
        };
        match self.sink.listen_stop(session_id).await {
            Ok(msg) => ok(msg),
            Err(e) => err(format!("meeting.listen_stop failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }
}

/// Per-turn listen context for a bound conversation (turn-tail injection).
#[async_trait]
pub trait MeetingListenContextSink: Send + Sync {
    async fn resolve_context(&self) -> Option<String>;
    async fn retrieve_for_question(&self, question: &str) -> Option<String>;
}

/// ContextContributor that injects listen window + rolling summary when enabled.
pub struct MeetingListenContributor {
    sink: Arc<dyn MeetingListenContextSink>,
}

impl MeetingListenContributor {
    pub fn new(sink: Arc<dyn MeetingListenContextSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl crate::context_contributor::ContextContributor for MeetingListenContributor {
    async fn pre_turn_context(&self) -> Option<String> {
        self.sink
            .resolve_context()
            .await
            .filter(|s| !s.trim().is_empty())
    }

    fn label(&self) -> &str {
        "meeting_listen"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockMeetingSink {
        sessions: Mutex<Vec<MeetingSessionSummary>>,
        hits: Mutex<Vec<MeetingTranscriptHit>>,
        start_calls: Mutex<Vec<(Option<String>, Option<String>)>>,
        capture_allowed: bool,
        fail_start: bool,
    }

    #[async_trait]
    impl MeetingSink for MockMeetingSink {
        async fn list(&self, _limit: i64) -> Result<Vec<MeetingSessionSummary>, String> {
            Ok(self.sessions.lock().unwrap().clone())
        }

        async fn get(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
            self.sessions
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.session_id == session_id)
                .cloned()
                .ok_or_else(|| format!("meeting session not found: {session_id}"))
        }

        async fn search_transcript(
            &self,
            _session_id: &str,
            _query: &str,
            _limit: i64,
        ) -> Result<Vec<MeetingTranscriptHit>, String> {
            Ok(self.hits.lock().unwrap().clone())
        }

        async fn get_notes(&self, session_id: &str) -> Result<String, String> {
            let _ = self.get(session_id).await?;
            Ok("Meeting notes are not ready until W11 (P2).".into())
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
            if self.fail_start {
                return Err("boom".into());
            }
            self.start_calls.lock().unwrap().push((
                session_id.map(str::to_owned),
                title.map(str::to_owned),
            ));
            Ok(MeetingSessionSummary {
                session_id: session_id.unwrap_or("new-1").into(),
                title: title.unwrap_or("Meeting").into(),
                status: "recording".into(),
                bound_conversation_id: None,
                mic_available: true,
                loopback_available: true,
                started_at_ms: Some(1),
                ended_at_ms: None,
            })
        }

        async fn pause(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
            let mut s = self.get(session_id).await?;
            s.status = "paused".into();
            Ok(s)
        }

        async fn resume(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
            let mut s = self.get(session_id).await?;
            s.status = "recording".into();
            Ok(s)
        }

        async fn stop(&self, session_id: &str) -> Result<MeetingSessionSummary, String> {
            let mut s = self.get(session_id).await?;
            s.status = "stopped".into();
            Ok(s)
        }

        async fn ask(&self, session_id: &str, question: &str) -> Result<String, String> {
            let hits = self.search_transcript(session_id, question, 20).await?;
            if hits.is_empty() {
                return Ok(format!(
                    "No transcript matches for question: {question}"
                ));
            }
            let body: Vec<String> = hits.iter().map(format_hit).collect();
            Ok(format!(
                "Question: {question}\nRelevant transcript:\n{}",
                body.join("\n")
            ))
        }

        async fn captions_recent(
            &self,
            _session_id: &str,
            limit: i64,
        ) -> Result<Vec<MeetingTranscriptHit>, String> {
            let hits = self.hits.lock().unwrap();
            let limit = limit.max(1) as usize;
            if hits.len() <= limit {
                Ok(hits.clone())
            } else {
                Ok(hits[hits.len() - limit..].to_vec())
            }
        }

        async fn listen_start(
            &self,
            session_id: &str,
            conversation_id: Option<&str>,
        ) -> Result<String, String> {
            let _ = self.get(session_id).await?;
            Ok(format!(
                "Listen started for {session_id} conversation={conversation_id:?}"
            ))
        }

        async fn listen_stop(&self, session_id: &str) -> Result<String, String> {
            let _ = self.get(session_id).await?;
            Ok(format!("Listen stopped for {session_id}"))
        }
    }

    fn sample_session() -> MeetingSessionSummary {
        MeetingSessionSummary {
            session_id: "s1".into(),
            title: "Standup".into(),
            status: "stopped".into(),
            bound_conversation_id: None,
            mic_available: true,
            loopback_available: false,
            started_at_ms: Some(10),
            ended_at_ms: Some(20),
        }
    }

    #[tokio::test]
    async fn meeting_list_formats_and_empty() {
        let sink = Arc::new(MockMeetingSink {
            capture_allowed: true,
            ..Default::default()
        });
        let empty = MeetingListTool::new(sink.clone())
            .execute(json!({}))
            .await;
        assert!(empty.content.contains("No meeting sessions"));

        sink.sessions.lock().unwrap().push(sample_session());
        let listed = MeetingListTool::new(sink).execute(json!({})).await;
        assert!(listed.content.contains("s1"));
        assert!(listed.content.contains("Standup"));
    }

    #[tokio::test]
    async fn meeting_start_rejects_when_headless() {
        let sink = Arc::new(MockMeetingSink {
            capture_allowed: false,
            ..Default::default()
        });
        let r = MeetingStartTool::new(sink).execute(json!({})).await;
        assert!(r.is_error);
        assert!(r.content.contains("headless"));
    }

    #[tokio::test]
    async fn meeting_start_creates_when_allowed() {
        let sink = Arc::new(MockMeetingSink {
            capture_allowed: true,
            ..Default::default()
        });
        let r = MeetingStartTool::new(sink.clone())
            .execute(json!({ "title": "Demo" }))
            .await;
        assert!(!r.is_error, "{}", r.content);
        assert!(r.content.contains("recording"));
        assert_eq!(
            sink.start_calls.lock().unwrap()[0],
            (None, Some("Demo".into()))
        );
    }

    #[tokio::test]
    async fn meeting_get_notes_stub() {
        let sink = Arc::new(MockMeetingSink {
            capture_allowed: true,
            sessions: Mutex::new(vec![sample_session()]),
            ..Default::default()
        });
        let r = MeetingGetNotesTool::new(sink)
            .execute(json!({ "session_id": "s1" }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("W11") || r.content.contains("not ready"));
    }

    #[tokio::test]
    async fn meeting_ask_concatenates_hits() {
        let sink = Arc::new(MockMeetingSink {
            capture_allowed: true,
            sessions: Mutex::new(vec![sample_session()]),
            hits: Mutex::new(vec![MeetingTranscriptHit {
                segment_id: "seg1".into(),
                speaker_label: Some("Alice".into()),
                text: "ship Friday".into(),
                start_ms: Some(100),
                end_ms: Some(200),
                is_partial: false,
            }]),
            ..Default::default()
        });
        let r = MeetingAskTool::new(sink)
            .execute(json!({ "session_id": "s1", "question": "deadline" }))
            .await;
        assert!(!r.is_error, "{}", r.content);
        assert!(r.content.contains("ship Friday"));
        assert!(r.content.contains("deadline"));
    }

    #[tokio::test]
    async fn meeting_search_requires_params() {
        let sink = Arc::new(MockMeetingSink {
            capture_allowed: true,
            ..Default::default()
        });
        let r = MeetingSearchTranscriptTool::new(sink)
            .execute(json!({ "session_id": "s1" }))
            .await;
        assert!(r.is_error);
    }
}

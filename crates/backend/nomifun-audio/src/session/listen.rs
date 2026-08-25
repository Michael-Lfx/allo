//! Meeting listen mode (P4 A2): rolling transcript window + summary for bound
//! Agent conversations. Passive injection only — never starts agent turns.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::session::notes::MeetingNotesCompleter;
use crate::session::types::{MeetingEvent, MeetingSegmentSnapshot};

/// Max final(+partial) segments retained in the live window.
pub const DEFAULT_MAX_SEGMENTS: usize = 48;
/// Drop segments older than this relative to the newest `end_ms` in the window.
pub const DEFAULT_WINDOW_MS: i64 = 5 * 60 * 1000;
/// Compact rolling summary after this many new final segments.
pub const DEFAULT_COMPACT_EVERY: usize = 8;
/// Caps extractive / LLM summary length for turn injection.
pub const SUMMARY_MAX_CHARS: usize = 1200;

const LISTEN_SUMMARY_SYSTEM: &str = "You summarize a live meeting transcript window for an \
agent that is listening silently. Output ONLY a short plain-text rolling summary \
(3-8 sentences, same language as the transcript). No markdown fences, no bullet \
lists unless the content itself needs them. Focus on decisions, open questions, \
and action items. If the window is empty or useless, say so briefly.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListenWindowSegment {
    pub segment_id: String,
    pub speaker_label: String,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_partial: bool,
}

impl From<&MeetingSegmentSnapshot> for ListenWindowSegment {
    fn from(s: &MeetingSegmentSnapshot) -> Self {
        Self {
            segment_id: s.segment_id.clone(),
            speaker_label: if s.speaker_label.trim().is_empty() {
                "Speaker".into()
            } else {
                s.speaker_label.trim().to_string()
            },
            text: s.text.clone(),
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            is_partial: s.is_partial,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingListenStatus {
    pub enabled: bool,
    pub session_id: String,
    pub conversation_id: Option<String>,
    pub window_segment_count: usize,
    pub rolling_summary: Option<String>,
    pub last_compacted_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ListenConfig {
    pub max_segments: usize,
    pub window_ms: i64,
    pub compact_every: usize,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            max_segments: DEFAULT_MAX_SEGMENTS,
            window_ms: DEFAULT_WINDOW_MS,
            compact_every: DEFAULT_COMPACT_EVERY,
        }
    }
}

/// Format one structured listen block line: `[t0-t1ms] speaker: text`.
pub fn format_listen_segment(seg: &ListenWindowSegment) -> String {
    let partial = if seg.is_partial { " partial" } else { "" };
    format!(
        "[{}-{}ms] {}: {}{partial}",
        seg.start_ms, seg.end_ms, seg.speaker_label, seg.text
    )
}

/// Build the turn-tail context block injected when listen is enabled.
pub fn format_listen_context_block(
    session_id: &str,
    summary: Option<&str>,
    segments: &[ListenWindowSegment],
) -> String {
    let mut out = format!("## Meeting listen context\nSession: {session_id}\n");
    if let Some(summary) = summary.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n### Rolling summary\n");
        out.push_str(summary);
        out.push('\n');
    }
    let finals: Vec<&ListenWindowSegment> = segments.iter().filter(|s| !s.is_partial).collect();
    if !finals.is_empty() {
        out.push_str("\n### Recent transcript\n");
        for seg in finals {
            out.push_str(&format_listen_segment(seg));
            out.push('\n');
        }
    }
    out
}

/// Extractive fallback when no LLM completer is wired.
pub fn extractive_summary(segments: &[ListenWindowSegment], max_chars: usize) -> String {
    let mut lines = Vec::new();
    for seg in segments.iter().filter(|s| !s.is_partial) {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        lines.push(format!("{}: {text}", seg.speaker_label));
    }
    if lines.is_empty() {
        return "No final transcript in the current listen window.".into();
    }
    let joined = lines.join("\n");
    if joined.chars().count() <= max_chars {
        return format!("Meeting listen summary (extractive):\n{joined}");
    }
    let preview: String = joined.chars().take(max_chars.saturating_sub(3)).collect();
    format!("Meeting listen summary (extractive):\n{preview}...")
}

/// Upsert a segment into the rolling window, trimming by count and time span.
pub fn upsert_into_window(
    window: &mut VecDeque<ListenWindowSegment>,
    seg: ListenWindowSegment,
    max_segments: usize,
    window_ms: i64,
) {
    if let Some(existing) = window.iter_mut().find(|s| s.segment_id == seg.segment_id) {
        *existing = seg;
    } else {
        window.push_back(seg);
    }

    while window.len() > max_segments {
        window.pop_front();
    }

    let newest_end = window.iter().map(|s| s.end_ms).max().unwrap_or(0);
    let cutoff = newest_end.saturating_sub(window_ms);
    while let Some(front) = window.front() {
        if front.end_ms < cutoff && window.len() > 1 {
            window.pop_front();
        } else {
            break;
        }
    }
}

/// Pick window segments whose text overlaps question tokens (simple retrieval).
pub fn select_relevant_segments(
    segments: &[ListenWindowSegment],
    question: &str,
    limit: usize,
) -> Vec<ListenWindowSegment> {
    if limit == 0 {
        return Vec::new();
    }
    let tokens: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric() && !is_cjk(c))
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| t.chars().count() >= 2)
        .collect();
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, &ListenWindowSegment)> = segments
        .iter()
        .filter(|s| !s.is_partial && !s.text.trim().is_empty())
        .filter_map(|s| {
            let lower = s.text.to_ascii_lowercase();
            let score = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
            if score == 0 {
                None
            } else {
                Some((score, s))
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.end_ms.cmp(&a.1.end_ms)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, s)| s.clone())
        .collect()
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{3040}'..='\u{30ff}').contains(&c)
}

#[derive(Debug)]
struct ListenSessionState {
    enabled: bool,
    conversation_id: Option<String>,
    window: VecDeque<ListenWindowSegment>,
    rolling_summary: Option<String>,
    last_compacted_at_ms: Option<i64>,
    finals_since_compact: usize,
}

impl ListenSessionState {
    fn status(&self, session_id: &str) -> MeetingListenStatus {
        MeetingListenStatus {
            enabled: self.enabled,
            session_id: session_id.to_string(),
            conversation_id: self.conversation_id.clone(),
            window_segment_count: self.window.len(),
            rolling_summary: self.rolling_summary.clone(),
            last_compacted_at_ms: self.last_compacted_at_ms,
        }
    }
}

/// In-process listen windows keyed by meeting session / bound conversation.
#[derive(Clone)]
pub struct MeetingListenService {
    inner: Arc<ListenInner>,
}

struct ListenInner {
    by_session: RwLock<HashMap<String, ListenSessionState>>,
    by_conversation: RwLock<HashMap<String, String>>,
    completer: RwLock<Option<Arc<dyn MeetingNotesCompleter>>>,
    config: ListenConfig,
}

impl MeetingListenService {
    pub fn new() -> Self {
        Self::with_config(ListenConfig::default())
    }

    pub fn with_config(config: ListenConfig) -> Self {
        Self {
            inner: Arc::new(ListenInner {
                by_session: RwLock::new(HashMap::new()),
                by_conversation: RwLock::new(HashMap::new()),
                completer: RwLock::new(None),
                config,
            }),
        }
    }

    pub fn set_completer(&self, completer: Arc<dyn MeetingNotesCompleter>) {
        if let Ok(mut g) = self.inner.completer.write() {
            *g = Some(completer);
        }
    }

    /// Subscribe to meeting events and feed active listen windows.
    pub fn spawn_event_loop(
        self: &Arc<Self>,
        mut rx: broadcast::Receiver<MeetingEvent>,
    ) -> tokio::task::JoinHandle<()> {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(MeetingEvent::SegmentUpserted { segment }) => {
                        this.on_segment(segment).await;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    pub async fn start(
        &self,
        session_id: &str,
        conversation_id: Option<String>,
        seed_segments: Vec<MeetingSegmentSnapshot>,
    ) -> Result<MeetingListenStatus, String> {
        let conversation_id = conversation_id.filter(|s| !s.trim().is_empty());
        let Some(conversation_id) = conversation_id else {
            return Err(
                "listen start requires a bound conversation_id (pass conversation_id or bind first)"
                    .into(),
            );
        };

        {
            let mut by_conv = self
                .inner
                .by_conversation
                .write()
                .map_err(|_| "listen state lock poisoned".to_string())?;
            if let Some(other) = by_conv.get(&conversation_id)
                && other != session_id
            {
                return Err(format!(
                    "conversation {conversation_id} is already listening to meeting {other}"
                ));
            }
            by_conv.insert(conversation_id.clone(), session_id.to_string());
        }

        let mut window = VecDeque::new();
        let cfg = self.inner.config.clone();
        for seg in seed_segments {
            upsert_into_window(
                &mut window,
                ListenWindowSegment::from(&seg),
                cfg.max_segments,
                cfg.window_ms,
            );
        }
        let summary = if window.iter().any(|s| !s.is_partial) {
            Some(extractive_summary(
                &window.iter().cloned().collect::<Vec<_>>(),
                SUMMARY_MAX_CHARS,
            ))
        } else {
            None
        };
        let now = now_ms();
        let state = ListenSessionState {
            enabled: true,
            conversation_id: Some(conversation_id),
            window,
            rolling_summary: summary.clone(),
            last_compacted_at_ms: summary.as_ref().map(|_| now),
            finals_since_compact: 0,
        };
        let status = state.status(session_id);
        self.inner
            .by_session
            .write()
            .map_err(|_| "listen state lock poisoned".to_string())?
            .insert(session_id.to_string(), state);
        Ok(status)
    }

    pub fn stop(&self, session_id: &str) -> Result<MeetingListenStatus, String> {
        let mut by_session = self
            .inner
            .by_session
            .write()
            .map_err(|_| "listen state lock poisoned".to_string())?;
        let Some(state) = by_session.get_mut(session_id) else {
            return Ok(MeetingListenStatus {
                enabled: false,
                session_id: session_id.to_string(),
                conversation_id: None,
                window_segment_count: 0,
                rolling_summary: None,
                last_compacted_at_ms: None,
            });
        };
        if let Some(cid) = state.conversation_id.clone()
            && let Ok(mut by_conv) = self.inner.by_conversation.write()
        {
            if by_conv.get(&cid).map(|s| s.as_str()) == Some(session_id) {
                by_conv.remove(&cid);
            }
        }
        state.enabled = false;
        Ok(state.status(session_id))
    }

    pub fn status(&self, session_id: &str) -> MeetingListenStatus {
        self.inner
            .by_session
            .read()
            .ok()
            .and_then(|g| g.get(session_id).map(|s| s.status(session_id)))
            .unwrap_or(MeetingListenStatus {
                enabled: false,
                session_id: session_id.to_string(),
                conversation_id: None,
                window_segment_count: 0,
                rolling_summary: None,
                last_compacted_at_ms: None,
            })
    }

    pub fn is_listening_for_conversation(&self, conversation_id: &str) -> bool {
        let Ok(by_conv) = self.inner.by_conversation.read() else {
            return false;
        };
        let Some(session_id) = by_conv.get(conversation_id) else {
            return false;
        };
        self.inner
            .by_session
            .read()
            .ok()
            .and_then(|g| g.get(session_id).map(|s| s.enabled))
            .unwrap_or(false)
    }

    /// Turn-tail block for a bound conversation, or `None` when not listening.
    pub fn context_for_conversation(&self, conversation_id: &str) -> Option<String> {
        let session_id = {
            let by_conv = self.inner.by_conversation.read().ok()?;
            by_conv.get(conversation_id)?.clone()
        };
        let by_session = self.inner.by_session.read().ok()?;
        let state = by_session.get(&session_id)?;
        if !state.enabled {
            return None;
        }
        let segs: Vec<_> = state.window.iter().cloned().collect();
        let block = format_listen_context_block(
            &session_id,
            state.rolling_summary.as_deref(),
            &segs,
        );
        if block.trim().is_empty() {
            None
        } else {
            Some(block)
        }
    }

    /// Question-scoped recent segments for optional send-path retrieval.
    pub fn retrieve_for_question(
        &self,
        conversation_id: &str,
        question: &str,
        limit: usize,
    ) -> Option<String> {
        let session_id = {
            let by_conv = self.inner.by_conversation.read().ok()?;
            by_conv.get(conversation_id)?.clone()
        };
        let by_session = self.inner.by_session.read().ok()?;
        let state = by_session.get(&session_id)?;
        if !state.enabled {
            return None;
        }
        let segs: Vec<_> = state.window.iter().cloned().collect();
        let hits = select_relevant_segments(&segs, question, limit);
        if hits.is_empty() {
            return None;
        }
        let mut out = format!(
            "[Relevant recent meeting segments for this question (listen window, session {session_id})]:\n"
        );
        for seg in &hits {
            out.push_str(&format_listen_segment(seg));
            out.push('\n');
        }
        Some(out)
    }

    /// Snapshot used by `meeting.ask` to prepend listen window + summary.
    pub fn ask_context(&self, session_id: &str) -> Option<(Option<String>, Vec<ListenWindowSegment>)> {
        let by_session = self.inner.by_session.read().ok()?;
        let state = by_session.get(session_id)?;
        if !state.enabled {
            return None;
        }
        Some((
            state.rolling_summary.clone(),
            state.window.iter().cloned().collect(),
        ))
    }

    async fn on_segment(&self, segment: MeetingSegmentSnapshot) {
        let session_id = segment.session_id.clone();
        let should_compact = {
            let Ok(mut by_session) = self.inner.by_session.write() else {
                return;
            };
            let Some(state) = by_session.get_mut(&session_id) else {
                return;
            };
            if !state.enabled {
                return;
            }
            let existing_was_partial = state
                .window
                .iter()
                .find(|s| s.segment_id == segment.segment_id)
                .map(|s| s.is_partial);
            let is_final = !segment.is_partial;
            upsert_into_window(
                &mut state.window,
                ListenWindowSegment::from(&segment),
                self.inner.config.max_segments,
                self.inner.config.window_ms,
            );
            let counts_toward_compact = is_final
                && match existing_was_partial {
                    None => true,
                    Some(true) => true,
                    Some(false) => false,
                };
            if counts_toward_compact {
                state.finals_since_compact =
                    state.finals_since_compact.saturating_add(1);
            }
            is_final && state.finals_since_compact >= self.inner.config.compact_every
        };

        if should_compact {
            self.compact_session(&session_id).await;
        }
    }

    async fn compact_session(&self, session_id: &str) {
        let (segments, previous_summary) = {
            let Ok(by_session) = self.inner.by_session.read() else {
                return;
            };
            let Some(state) = by_session.get(session_id) else {
                return;
            };
            if !state.enabled {
                return;
            }
            (
                state.window.iter().cloned().collect::<Vec<_>>(),
                state.rolling_summary.clone(),
            )
        };

        let transcript = segments
            .iter()
            .filter(|s| !s.is_partial)
            .map(|s| format!("{}: {}", s.speaker_label, s.text.trim()))
            .filter(|line| !line.ends_with(": "))
            .collect::<Vec<_>>()
            .join("\n");

        let summary = match self.complete_summary(&transcript, previous_summary.as_deref()).await {
            Some(s) => s,
            None => extractive_summary(&segments, SUMMARY_MAX_CHARS),
        };
        let now = now_ms();
        if let Ok(mut by_session) = self.inner.by_session.write()
            && let Some(state) = by_session.get_mut(session_id)
            && state.enabled
        {
            state.rolling_summary = Some(summary);
            state.last_compacted_at_ms = Some(now);
            state.finals_since_compact = 0;
        }
    }

    async fn complete_summary(
        &self,
        transcript: &str,
        previous: Option<&str>,
    ) -> Option<String> {
        if transcript.trim().is_empty() {
            return None;
        }
        let completer = self.inner.completer.read().ok()?.clone()?;
        let mut user = String::new();
        if let Some(prev) = previous.map(str::trim).filter(|s| !s.is_empty()) {
            user.push_str("Previous rolling summary:\n");
            user.push_str(prev);
            user.push_str("\n\n");
        }
        user.push_str("Current transcript window:\n");
        user.push_str(transcript);
        match completer.complete(LISTEN_SUMMARY_SYSTEM, &user).await {
            Ok(raw) => {
                let trimmed = raw.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else if trimmed.chars().count() > SUMMARY_MAX_CHARS {
                    Some(
                        trimmed
                            .chars()
                            .take(SUMMARY_MAX_CHARS.saturating_sub(3))
                            .collect::<String>()
                            + "...",
                    )
                } else {
                    Some(trimmed)
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "meeting listen summary LLM failed; using extractive");
                None
            }
        }
    }
}

impl Default for MeetingListenService {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: &str, speaker: &str, text: &str, start: i64, end: i64) -> ListenWindowSegment {
        ListenWindowSegment {
            segment_id: id.into(),
            speaker_label: speaker.into(),
            text: text.into(),
            start_ms: start,
            end_ms: end,
            is_partial: false,
        }
    }

    #[test]
    fn upsert_replaces_partial_and_trims_by_count() {
        let mut window = VecDeque::new();
        upsert_into_window(
            &mut window,
            ListenWindowSegment {
                segment_id: "a".into(),
                speaker_label: "A".into(),
                text: "hello".into(),
                start_ms: 0,
                end_ms: 100,
                is_partial: true,
            },
            3,
            60_000,
        );
        upsert_into_window(
            &mut window,
            ListenWindowSegment {
                segment_id: "a".into(),
                speaker_label: "A".into(),
                text: "hello world".into(),
                start_ms: 0,
                end_ms: 120,
                is_partial: false,
            },
            3,
            60_000,
        );
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].text, "hello world");
        assert!(!window[0].is_partial);

        for i in 0..5 {
            upsert_into_window(
                &mut window,
                seg(&format!("s{i}"), "B", "x", i * 10, i * 10 + 5),
                3,
                60_000,
            );
        }
        assert_eq!(window.len(), 3);
        assert_eq!(window.front().unwrap().segment_id, "s2");
    }

    #[test]
    fn upsert_trims_by_time_window() {
        let mut window = VecDeque::new();
        upsert_into_window(&mut window, seg("old", "A", "old", 0, 100), 20, 1_000);
        upsert_into_window(
            &mut window,
            seg("new", "B", "new", 5_000, 5_100),
            20,
            1_000,
        );
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].segment_id, "new");
    }

    #[test]
    fn format_context_includes_summary_and_structured_lines() {
        let block = format_listen_context_block(
            "sess-1",
            Some("We discussed the launch."),
            &[seg("1", "Alice", "ship Friday", 100, 200)],
        );
        assert!(block.contains("sess-1"));
        assert!(block.contains("We discussed the launch."));
        assert!(block.contains("[100-200ms] Alice: ship Friday"));
    }

    #[test]
    fn extractive_summary_skips_partials() {
        let summary = extractive_summary(
            &[
                ListenWindowSegment {
                    segment_id: "p".into(),
                    speaker_label: "A".into(),
                    text: "partial only".into(),
                    start_ms: 0,
                    end_ms: 10,
                    is_partial: true,
                },
                seg("f", "Bob", "final line", 10, 20),
            ],
            400,
        );
        assert!(summary.contains("Bob: final line"));
        assert!(!summary.contains("partial only"));
    }

    #[test]
    fn select_relevant_scores_keyword_overlap() {
        let segs = vec![
            seg("1", "A", "budget review next quarter", 0, 10),
            seg("2", "B", "ship the Friday release", 10, 20),
            seg("3", "C", "lunch plans", 20, 30),
        ];
        let hits = select_relevant_segments(&segs, "When is the Friday release?", 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].segment_id, "2");
    }

    #[tokio::test]
    async fn service_start_stop_and_context() {
        let svc = MeetingListenService::with_config(ListenConfig {
            max_segments: 10,
            window_ms: 60_000,
            compact_every: 100,
        });
        let status = svc
            .start(
                "s1",
                Some("conv-1".into()),
                vec![MeetingSegmentSnapshot {
                    segment_id: "seg1".into(),
                    session_id: "s1".into(),
                    channel: None,
                    speaker_id: None,
                    speaker_label: "Alice".into(),
                    text: "hello listen".into(),
                    is_partial: false,
                    is_manual_edit: false,
                    start_ms: 0,
                    end_ms: 50,
                }],
            )
            .await
            .unwrap();
        assert!(status.enabled);
        assert_eq!(status.conversation_id.as_deref(), Some("conv-1"));
        assert!(svc.is_listening_for_conversation("conv-1"));
        let ctx = svc.context_for_conversation("conv-1").unwrap();
        assert!(ctx.contains("Alice"));
        assert!(ctx.contains("hello listen"));

        let retrieved = svc
            .retrieve_for_question("conv-1", "hello?", 3)
            .unwrap();
        assert!(retrieved.contains("hello listen"));

        let stopped = svc.stop("s1").unwrap();
        assert!(!stopped.enabled);
        assert!(svc.context_for_conversation("conv-1").is_none());
    }

    #[tokio::test]
    async fn start_requires_conversation() {
        let svc = MeetingListenService::new();
        let err = svc.start("s1", None, vec![]).await.unwrap_err();
        assert!(err.contains("conversation_id"));
    }
}

//! Session listing, metadata updates (rename/pin/archive), and local full-text search.

use std::collections::{BTreeSet, HashMap};

use flowy_agent_core::{AcceptanceQueue, SessionRole};

use super::observer::{
    self, ensure_engine_session, extract_session_preview, persist_index,
};
use super::{
    AgentHandle, AgentUpdate, ChatLine, SessionSearchHit, SessionSummary,
    TITLE_MAX_CHARS, lock, log_storage_io_error, title,
};

impl AgentHandle {
    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        let live = lock(&self.inner.state.live);
        let mut rows: Vec<SessionSummary> = live
            .iter()
            .filter(|(_, session)| session.role != SessionRole::Worker)
            .map(|(id, session)| SessionSummary {
                id: id.clone(),
                title: session.title.clone(),
                updated_at: session.updated_at,
                project_id: session
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.project_id.clone()),
                pinned: session.pinned,
                archived: session.archived,
            })
            .collect();
        drop(live);
        rows.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        rows
    }

    /// Manually rename a session, persisting atomically to both `history.json`
    /// and `index.json` (rolling back on error) and locking out any subsequent
    /// automatic title overwrite. Direct metadata mutation on hidden internal
    /// `SessionRole::Worker` sessions is rejected.
    pub fn rename_session(
        &self,
        session_id: &str,
        new_title: &str,
    ) -> Result<(), &'static str> {
        let cleaned = title::clamp_manual_title(new_title, TITLE_MAX_CHARS);
        if cleaned.is_empty() {
            return Err("chat.rename_empty");
        }
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let previous = lock(&self.inner.state.live)
            .get(session_id)
            .map(|session| {
                if session.role == SessionRole::Worker {
                    return Err("notice.agent_failed");
                }
                session.title_signal.cancel();
                Ok((
                    session.title.clone(),
                    session.titled,
                    session.auto_title_pending,
                    session.auto_title_in_flight,
                ))
            })
            .ok_or("notice.session_storage_failed")??;
        ensure_engine_session(&engine, session_id)
            .and_then(|()| {
                engine.set_session_title(session_id, cleaned.clone(), true)
            })
            .map_err(|error| {
                log_storage_io_error(
                    "rename session",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.title = cleaned;
            session.titled = true;
            session.auto_title_pending = false;
            session.auto_title_in_flight = false;
        }
        if let Err(err) = persist_index(&self.inner.state) {
            let (old_title, old_titled, old_pending, old_in_flight) = previous;
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.title.clone_from(&old_title);
                session.titled = old_titled;
                session.auto_title_pending = old_pending;
                session.auto_title_in_flight = old_in_flight;
            }
            let _ = engine.set_session_title(session_id, old_title, old_titled);
            self.inner.state.report_storage_error();
            return Err(err);
        }
        observer::sync_artifact_title(&self.inner.state, &engine, session_id);
        self.inner.state.push_update(AgentUpdate::Session {
            id: session_id.to_owned(),
        });
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(())
    }

    /// Pin or unpin a session durably across both `history.json` and
    /// `index.json` (rolling back on error) without changing its activity
    /// timestamp. Direct metadata mutation on hidden internal
    /// `SessionRole::Worker` sessions is rejected.
    pub fn set_session_pinned(
        &self,
        session_id: &str,
        pinned: bool,
    ) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let old_pinned = lock(&self.inner.state.live)
            .get(session_id)
            .map(|session| {
                if session.role == SessionRole::Worker {
                    return Err("notice.agent_failed");
                }
                Ok(session.pinned)
            })
            .ok_or("notice.session_storage_failed")??;
        ensure_engine_session(&engine, session_id)
            .and_then(|()| engine.set_session_pinned(session_id, pinned))
            .map_err(|error| {
                log_storage_io_error(
                    "set session pinned",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.pinned = pinned;
        }
        if let Err(err) = persist_index(&self.inner.state) {
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.pinned = old_pinned;
            }
            let _ = engine.set_session_pinned(session_id, old_pinned);
            self.inner.state.report_storage_error();
            return Err(err);
        }
        self.inner.state.push_update(AgentUpdate::Session {
            id: session_id.to_owned(),
        });
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(())
    }

    /// Archive or unarchive a session durably across both `history.json` and
    /// `index.json` (rolling back on error) without deleting history or
    /// artifacts. Active/running sessions — including any Router whose child
    /// Workers are active, hold `active_claims`, or have non-terminal queued
    /// receipts — cannot be archived until settled so background workers and
    /// directory leases are never hidden from view. Direct metadata mutation on
    /// hidden internal `SessionRole::Worker` sessions is rejected.
    pub fn set_session_archived(
        &self,
        session_id: &str,
        archived: bool,
    ) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let mut family_ids: BTreeSet<String> =
            std::iter::once(session_id.to_owned())
                .chain(engine.child_session_ids(session_id))
                .collect();
        let has_pending_followups = archived
            && engine
                .with_session(session_id, |rec| {
                    !rec.pending_router_followups.is_empty()
                })
                .unwrap_or(false);
        let old_archived = {
            let live = lock(&self.inner.state.live);
            let Some(session) = live.get(session_id) else {
                return Err("notice.session_storage_failed");
            };
            if session.role == SessionRole::Worker {
                return Err("notice.agent_failed");
            }
            let prev = session.archived;
            if archived {
                for (cid, child) in live.iter() {
                    if child.root_session_id.as_deref() == Some(session_id) {
                        family_ids.insert(cid.clone());
                    }
                }
            }
            let any_live_busy = archived
                && family_ids.iter().any(|member| {
                    live.get(member).is_some_and(|s| s.status.is_busy())
                });
            let any_claimed = archived
                && lock(&self.inner.state.active_claims)
                    .values()
                    .any(|claim| family_ids.contains(&claim.session_id));
            drop(live);
            if archived {
                for member in &family_ids {
                    self.inner.settle_finishing_fence(&engine, member);
                }
            }
            let any_engine_active = archived
                && family_ids.iter().any(|member| engine.is_active(member));
            let any_queued_receipt = archived
                && AcceptanceQueue::new(self.inner.state.root.clone())
                    .all()
                    .unwrap_or_default()
                    .into_iter()
                    .any(|receipt| {
                        receipt.completed_at_ms.is_none()
                            && !receipt.cancelled
                            && !receipt.dead
                            && receipt
                                .target_session_id
                                .as_deref()
                                .or(receipt.session_hint.as_deref())
                                .is_some_and(|sid| family_ids.contains(sid))
                    });
            if archived
                && (has_pending_followups
                    || any_live_busy
                    || any_engine_active
                    || any_claimed
                    || any_queued_receipt)
            {
                return Err("notice.agent_busy");
            }
            prev
        };
        ensure_engine_session(&engine, session_id)
            .and_then(|()| engine.set_session_archived(session_id, archived))
            .map_err(|error| {
                log_storage_io_error(
                    "set session archived",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        let cleared_active = if archived {
            let mut active = lock(&self.inner.state.active);
            if active.as_deref() == Some(session_id) {
                active.take()
            } else {
                None
            }
        } else {
            None
        };
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.archived = archived;
        }
        if let Err(err) = persist_index(&self.inner.state) {
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.archived = old_archived;
            }
            if let Some(prev_active) = cleared_active {
                *lock(&self.inner.state.active) = Some(prev_active);
            }
            let _ = engine.set_session_archived(session_id, old_archived);
            self.inner.state.report_storage_error();
            return Err(err);
        }
        self.inner.state.push_update(AgentUpdate::Session {
            id: session_id.to_owned(),
        });
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(())
    }

    /// Zero-dependency in-memory search across redacted session titles and
    /// projected message lines. Empty queries return the most recent active
    /// top-level sessions (`Standard` and `Router`). Hidden internal
    /// `SessionRole::Worker` sessions are never returned directly: non-empty
    /// matches inside a Worker fold their redacted snippet and score into the
    /// owning top-level Router (deduplicated by `root_session_id`), while
    /// internal `SystemEvent` / `CardResume` control rows are skipped.
    #[must_use]
    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> Vec<SessionSearchHit> {
        if limit == 0 {
            return Vec::new();
        }
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(str::to_lowercase)
            .filter(|t| !t.is_empty())
            .collect();
        let full_query = tokens.join(" ");
        let live = lock(&self.inner.state.live);
        if tokens.is_empty() {
            let mut hits: Vec<SessionSearchHit> = live
                .iter()
                .filter(|(_, session)| {
                    session.role != SessionRole::Worker && !session.archived
                })
                .map(|(id, session)| {
                    let project_id = session
                        .workspace
                        .as_ref()
                        .map(|ws| ws.project_id.clone());
                    let safe_title = title::redact_search_text(&session.title);
                    let score = if session.pinned { 20 } else { 10 };
                    let safe_preview =
                        extract_session_preview(&session.projection.lines)
                            .map(|text| title::redact_search_text(&text));
                    SessionSearchHit {
                        session_id: id.clone(),
                        title: safe_title,
                        project_id,
                        updated_at: session.updated_at,
                        pinned: session.pinned,
                        archived: session.archived,
                        snippet: safe_preview,
                        score,
                    }
                })
                .collect();
            drop(live);
            hits.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| b.updated_at.cmp(&a.updated_at))
            });
            hits.truncate(limit);
            return hits;
        }

        // `(hit, has_matched_snippet)` keyed by top-level session ID.
        let mut by_root: HashMap<String, (SessionSearchHit, bool)> =
            HashMap::new();

        // Pass 1: score visible top-level sessions (`Standard` | `Router`).
        for (id, session) in live.iter() {
            if session.role == SessionRole::Worker {
                continue;
            }
            let project_id =
                session.workspace.as_ref().map(|ws| ws.project_id.clone());
            let safe_title = title::redact_search_text(&session.title);
            let title_lower = safe_title.to_lowercase();
            let mut best_score = score_text_match(
                &title_lower,
                &tokens,
                &full_query,
                120,
                95,
                75,
                45,
            );
            let (body_score, body_snippet) = score_projected_lines(
                &session.projection.lines,
                &title_lower,
                &tokens,
                best_score,
            );
            best_score = best_score.max(body_score);
            if best_score == 0 {
                continue;
            }
            if !session.archived {
                best_score += 10;
            }
            if session.pinned {
                best_score += 5;
            }
            let has_match_snippet = body_snippet.is_some();
            let snippet = body_snippet.or_else(|| {
                extract_session_preview(&session.projection.lines)
                    .map(|text| title::redact_search_text(&text))
            });
            by_root.insert(
                id.clone(),
                (
                    SessionSearchHit {
                        session_id: id.clone(),
                        title: safe_title,
                        project_id,
                        updated_at: session.updated_at,
                        pinned: session.pinned,
                        archived: session.archived,
                        snippet,
                        score: best_score,
                    },
                    has_match_snippet,
                ),
            );
        }

        // Pass 2: fold hidden `Worker` body matches into their owning `Router`.
        for session in live.values() {
            if session.role != SessionRole::Worker {
                continue;
            }
            let Some(root_id) = session.root_session_id.as_deref() else {
                continue;
            };
            let Some(parent) = live.get(root_id) else {
                continue;
            };
            if parent.role == SessionRole::Worker {
                continue;
            }
            let parent_safe_title = title::redact_search_text(&parent.title);
            let parent_title_lower = parent_safe_title.to_lowercase();
            let (body_score, body_snippet) = score_projected_lines(
                &session.projection.lines,
                &parent_title_lower,
                &tokens,
                0,
            );
            if body_score == 0 {
                continue;
            }
            let mut folded_score = body_score;
            if !parent.archived {
                folded_score += 10;
            }
            if parent.pinned {
                folded_score += 5;
            }
            if let Some((existing, has_match)) = by_root.get_mut(root_id) {
                if folded_score > existing.score {
                    existing.score = folded_score;
                    if body_snippet.is_some() {
                        existing.snippet = body_snippet;
                        *has_match = true;
                    }
                } else if !*has_match && body_snippet.is_some() {
                    existing.snippet = body_snippet;
                    *has_match = true;
                }
                existing.updated_at =
                    existing.updated_at.max(session.updated_at);
            } else {
                let project_id =
                    parent.workspace.as_ref().map(|ws| ws.project_id.clone());
                let snippet = body_snippet.or_else(|| {
                    extract_session_preview(&parent.projection.lines)
                        .map(|text| title::redact_search_text(&text))
                });
                by_root.insert(
                    root_id.to_owned(),
                    (
                        SessionSearchHit {
                            session_id: root_id.to_owned(),
                            title: parent_safe_title,
                            project_id,
                            updated_at: parent
                                .updated_at
                                .max(session.updated_at),
                            pinned: parent.pinned,
                            archived: parent.archived,
                            snippet,
                            score: folded_score,
                        },
                        true,
                    ),
                );
            }
        }
        drop(live);
        let mut hits: Vec<SessionSearchHit> =
            by_root.into_values().map(|(hit, _)| hit).collect();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        hits.truncate(limit);
        hits
    }
}

/// Scans projected conversation rows (skipping internal host control/event
/// prompts) against tokenized lowercased search terms and returns the best
/// body match score plus its redacted snippet.
fn score_projected_lines(
    lines: &[ChatLine],
    title_lower: &str,
    tokens: &[String],
    existing_title_score: u32,
) -> (u32, Option<String>) {
    let mut best_score = existing_title_score;
    let mut best_snippet = None;
    for line in lines.iter().rev() {
        let raw_text = match line {
            ChatLine::User { text }
                if !observer::is_internal_control_line_text(text) =>
            {
                text.as_str()
            }
            ChatLine::Assistant { text } => text.as_str(),
            _ => continue,
        };
        let cleaned =
            title::redact_search_text(&title::strip_markdown_inline(raw_text));
        if cleaned.is_empty() {
            continue;
        }
        let lower = cleaned.to_lowercase();
        if tokens.iter().all(|tok| lower.contains(tok.as_str())) {
            let snippet = extract_match_snippet(&cleaned, &tokens[0]);
            return (best_score.max(55), Some(snippet));
        }
        if best_score == 0
            && best_snippet.is_none()
            && tokens.iter().all(|tok| {
                title_lower.contains(tok.as_str())
                    || lower.contains(tok.as_str())
            })
        {
            let first_body_tok = tokens
                .iter()
                .find(|tok| lower.contains(tok.as_str()))
                .map_or(tokens[0].as_str(), String::as_str);
            best_snippet =
                Some(extract_match_snippet(&cleaned, first_body_tok));
            best_score = 40;
        }
    }
    (best_score, best_snippet)
}

/// Scores a lowercased candidate string against tokenized lowercased search terms.
fn score_text_match(
    haystack_lower: &str,
    tokens: &[String],
    full_query: &str,
    exact_score: u32,
    prefix_score: u32,
    substring_score: u32,
    fuzzy_score: u32,
) -> u32 {
    if haystack_lower.is_empty() || tokens.is_empty() {
        return 0;
    }
    if haystack_lower == full_query {
        return exact_score;
    }
    if haystack_lower.starts_with(full_query) {
        return prefix_score;
    }
    if tokens
        .iter()
        .all(|tok| haystack_lower.contains(tok.as_str()))
    {
        return substring_score;
    }
    if is_subsequence(haystack_lower, full_query) {
        return fuzzy_score;
    }
    0
}

fn is_subsequence(haystack: &str, needle: &str) -> bool {
    let mut needle_chars = needle.chars().filter(|ch| !ch.is_whitespace());
    let Some(mut target) = needle_chars.next() else {
        return false;
    };
    for ch in haystack.chars() {
        if ch == target {
            match needle_chars.next() {
                Some(next) => target = next,
                None => return true,
            }
        }
    }
    false
}

/// Extracts a char-safe ~64-char snippet centered around the first match of
/// `needle_lower` inside `text` (plain text with Markdown already stripped).
fn extract_match_snippet(text: &str, needle_lower: &str) -> String {
    const WINDOW: usize = 64;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= WINDOW {
        return text.to_owned();
    }
    let lower_chars: Vec<char> = text.to_lowercase().chars().collect();
    let needle: Vec<char> = needle_lower.chars().collect();
    let match_pos = if needle.is_empty() || needle.len() > lower_chars.len() {
        0
    } else {
        lower_chars
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
            .unwrap_or(0)
    };
    let start = match_pos.saturating_sub(18).min(chars.len());
    let end = (start + WINDOW).min(chars.len());
    let mut snippet: String = chars[start..end].iter().collect();
    if start > 0 {
        snippet = format!("\u{2026}{}", snippet.trim_start());
    }
    if end < chars.len() {
        snippet = format!("{}\u{2026}", snippet.trim_end());
    }
    snippet
}

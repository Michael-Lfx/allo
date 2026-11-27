//! Bounded, version-safe in-memory Work event inbox and priority-ordered
//! Router turn scheduler (`pending_router_followups` FIFO > `[WorkEventBatch]`).
//!
//! A batch stays owned by the inbox until its announcement turn settles: a
//! failed announcement (e.g. a transient model transport error) puts the
//! batch back and retries with backoff, and once the retries are used up the
//! host posts the Worker results into the conversation itself, so a finished
//! Worker's result is never dropped silently.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use flowy_agent_core::{MessageSource, PendingRouterFollowup, SessionRole};

use super::observer::persist_index;
use super::{
    ChatLine, Harness, Ordering, ResumedSubmission, UiState, lock, now_ms,
};

/// Maximum coalesced terminal Worker events buffered per Router inbox.
pub(super) const MAX_ROUTER_INBOX_EVENTS: usize = 32;

/// Maximum durable FIFO user follow-ups accepted while a Router is streaming.
pub(super) const MAX_ROUTER_FOLLOWUPS: usize = 16;

/// Delays before re-announcing a batch whose announcement turn failed. Once
/// they are used up, the host posts the Worker results without a model turn.
#[cfg(not(test))]
const ANNOUNCEMENT_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(120),
];
#[cfg(test)]
const ANNOUNCEMENT_RETRY_DELAYS: [Duration; 3] = [Duration::from_millis(30); 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkEventKind {
    Completed,
    Failed,
    WaitingForUser,
    Cancelled,
}

impl WorkEventKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::WaitingForUser => "waiting_for_user",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkEvent {
    pub(super) worker_id: String,
    pub(super) submission_id: Option<String>,
    pub(super) contract_revision: Option<u32>,
    pub(super) kind: WorkEventKind,
    pub(super) observed_at_ms: u64,
}

/// Per-Router bounded memory inbox for coalescing asynchronous Worker events.
///
/// Lock ordering: always a leaf lock under `UiState::router_inboxes`. Never
/// acquire `dispatch`, `engine`, `index_write`, or `live` while holding it.
#[derive(Default)]
pub(super) struct RouterInbox {
    pub(super) events: VecDeque<WorkEvent>,
    pub(super) flush_scheduled: bool,
    pub(super) announcement_in_flight: bool,
    /// The batch the in-flight announcement turn is reporting. Kept until the
    /// turn settles so a failed announcement can put it back.
    pub(super) in_flight: Vec<WorkEvent>,
    /// Consecutive failed announcements of the current batch.
    pub(super) failed_attempts: usize,
    /// No batch is announced before this epoch ms (retry backoff); `0` = none.
    pub(super) retry_not_before_ms: u64,
}

impl RouterInbox {
    /// Puts `events` back at the front, skipping any that a newer event for
    /// the same `(worker_id, submission_id)` has replaced meanwhile.
    fn requeue(&mut self, events: Vec<WorkEvent>) {
        for event in events.into_iter().rev() {
            let superseded = self.events.iter().any(|item| {
                item.worker_id == event.worker_id
                    && item.submission_id == event.submission_id
            });
            if !superseded && self.events.len() < MAX_ROUTER_INBOX_EVENTS {
                self.events.push_front(event);
            }
        }
    }
}

/// Record one Worker terminal/waiting event into its parent Router's inbox,
/// coalescing strictly by `(worker_id, submission_id)` so distinct revisions on
/// the same Worker never overwrite each other.
pub(super) fn push_work_event(
    state: &UiState,
    router_id: &str,
    event: WorkEvent,
) {
    let mut inboxes = lock(&state.router_inboxes);
    let inbox = inboxes.entry(router_id.to_owned()).or_default();
    if let Some(existing) = inbox.events.iter_mut().find(|item| {
        item.worker_id == event.worker_id
            && item.submission_id == event.submission_id
    }) {
        *existing = event;
    } else {
        while inbox.events.len() >= MAX_ROUTER_INBOX_EVENTS {
            inbox.events.pop_front();
        }
        inbox.events.push_back(event);
    }
    drop(inboxes);
}

pub(super) fn clear_announcement_in_flight(state: &UiState, router_id: &str) {
    let mut inboxes = lock(&state.router_inboxes);
    if let Some(inbox) = inboxes.get_mut(router_id) {
        inbox.announcement_in_flight = false;
        inbox.in_flight.clear();
    }
    drop(inboxes);
}

enum Settled {
    Retry(Duration),
    GiveUp(Vec<WorkEvent>),
}

/// Settles the Router's announcement turn. A failed one puts its batch back
/// and retries after a backoff; once the retries are used up the host posts
/// the Worker results itself.
pub(super) fn settle_announcement(
    harness: &Arc<Harness>,
    router_id: &str,
    failed: bool,
) {
    let settled = {
        let mut inboxes = lock(&harness.state.router_inboxes);
        let Some(inbox) = inboxes.get_mut(router_id) else {
            return;
        };
        inbox.announcement_in_flight = false;
        let batch = std::mem::take(&mut inbox.in_flight);
        let settled = if !failed || batch.is_empty() {
            inbox.failed_attempts = 0;
            None
        } else if let Some(delay) = ANNOUNCEMENT_RETRY_DELAYS
            .get(inbox.failed_attempts)
            .copied()
        {
            inbox.failed_attempts += 1;
            inbox.requeue(batch);
            let delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
            inbox.retry_not_before_ms = now_ms().saturating_add(delay_ms);
            Some(Settled::Retry(delay))
        } else {
            inbox.failed_attempts = 0;
            Some(Settled::GiveUp(batch))
        };
        if !matches!(settled, Some(Settled::Retry(_))) {
            inbox.retry_not_before_ms = 0;
        }
        drop(inboxes);
        settled
    };
    match settled {
        Some(Settled::Retry(delay)) => {
            tracing::warn!(
                target: "flowy.router",
                router_id = %router_id,
                delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                "work event announcement failed; retrying"
            );
            schedule_router_inbox_flush_after(harness, router_id, delay);
        }
        Some(Settled::GiveUp(batch)) => {
            tracing::warn!(
                target: "flowy.router",
                router_id = %router_id,
                events = batch.len(),
                "work event announcement retries exhausted; posting results directly"
            );
            post_announcement_fallback(harness, router_id, batch);
        }
        None => {}
    }
}

fn schedule_router_inbox_flush_after(
    harness: &Arc<Harness>,
    router_id: &str,
    delay: Duration,
) {
    let weak = Arc::downgrade(harness);
    let router_id = router_id.to_owned();
    harness.runtime.spawn(async move {
        tokio::time::sleep(delay).await;
        if let Some(harness) = weak.upgrade() {
            schedule_router_inbox_flush(&harness, &router_id);
        }
    });
}

fn post_announcement_fallback(
    harness: &Arc<Harness>,
    router_id: &str,
    batch: Vec<WorkEvent>,
) {
    let weak = Arc::downgrade(harness);
    let router_id = router_id.to_owned();
    harness.runtime.spawn_blocking(move || {
        let Some(harness) = weak.upgrade() else {
            return;
        };
        if let Err(err) = harness.post_work_results(&router_id, &batch) {
            tracing::warn!(
                target: "flowy.router",
                router_id = %router_id,
                err,
                "could not post work results after failed announcements"
            );
        }
    });
}

fn last_assistant_text(lines: &[ChatLine]) -> Option<String> {
    lines.iter().rev().find_map(|line| match line {
        ChatLine::Assistant { text } if !text.trim().is_empty() => {
            Some(text.clone())
        }
        _ => None,
    })
}

impl Harness {
    /// Writes the batch's results into the Router conversation without a
    /// model turn: each completed Worker's final reply under its title, and
    /// one error status if any Worker failed.
    fn post_work_results(
        &self,
        router_id: &str,
        batch: &[WorkEvent],
    ) -> Result<(), &'static str> {
        let works = self.child_works(router_id);
        let lines = work_result_lines(&lock(&self.state.live), &works, batch);
        if lines.is_empty() {
            return Ok(());
        }
        lock(&self.state.live)
            .get_mut(router_id)
            .filter(|session| session.role == SessionRole::Router)
            .ok_or("ROUTER_NOT_FOUND")?
            .projection
            .lines
            .extend(lines);
        persist_index(&self.state)?;
        self.state.stream_changed(router_id);
        Ok(())
    }
}

/// The lines `post_work_results` writes for a batch: each completed Worker's
/// final reply under its title, and one error status if any Worker failed.
fn work_result_lines(
    live: &std::collections::HashMap<String, super::Live>,
    works: &[super::router::ChildWorkSummary],
    batch: &[WorkEvent],
) -> Vec<ChatLine> {
    let mut lines = Vec::new();
    let mut any_failed = false;
    for event in batch {
        match event.kind {
            WorkEventKind::Completed => {}
            WorkEventKind::Failed => {
                any_failed = true;
                continue;
            }
            WorkEventKind::WaitingForUser | WorkEventKind::Cancelled => {
                continue;
            }
        }
        let Some(reply) = live
            .get(&event.worker_id)
            .and_then(|session| last_assistant_text(&session.projection.lines))
        else {
            continue;
        };
        let title = works
            .iter()
            .find(|work| work.work_id == event.worker_id)
            .map(|work| work.title.trim())
            .filter(|title| !title.is_empty());
        lines.push(ChatLine::Assistant {
            text: match title {
                Some(title) => format!("**{title}**\n\n{reply}"),
                None => reply,
            },
        });
    }
    if any_failed {
        lines.push(ChatLine::Status {
            text: "chat.status_error".into(),
        });
    }
    lines
}

fn parse_submission_u128(submission_id: &str) -> Option<u128> {
    let hex = submission_id.strip_prefix("submission-")?;
    u128::from_str_radix(hex, 16).ok()
}

/// Count non-terminal `(active_running, waiting_user)` sibling Workers so
/// both `[WorkEventBatch]` metadata and Router prompts stay consistent.
pub(super) fn sibling_work_counts(
    works: &[super::router::ChildWorkSummary],
) -> (usize, usize) {
    let mut active_running = 0_usize;
    let mut waiting_user = 0_usize;
    for work in works {
        if matches!(work.state.as_str(), "running" | "queued") {
            active_running += 1;
        }
        if work.state == "waiting_for_user" || !work.pending_cards.is_empty() {
            waiting_user += 1;
        }
    }
    (active_running, waiting_user)
}

/// Format `[WorkEventBatch]` ONLY after re-reading each Worker's authoritative
/// current state and `contract_revision`, ensuring delayed older-revision
/// events can never overwrite or misrepresent a newer revision.
fn format_work_event_batch(
    harness: &Harness,
    router_id: &str,
    events: &[WorkEvent],
) -> String {
    let works = harness.child_works(router_id);
    let mut out = String::from("[WorkEventBatch]\n");
    for event in events {
        let current = works.iter().find(|w| w.work_id == event.worker_id);
        match current {
            Some(work) => {
                let stale = matches!(
                    (event.contract_revision, work.contract_revision),
                    (Some(ev_rev), Some(cur_rev)) if ev_rev < cur_rev
                );
                let rev_note = match (
                    event.contract_revision,
                    work.contract_revision,
                ) {
                    (Some(ev_rev), Some(cur_rev)) if ev_rev < cur_rev => {
                        format!(
                            "stale_event_superseded(event_rev=v{ev_rev},current_rev=v{cur_rev})"
                        )
                    }
                    (_, Some(cur_rev)) => format!("rev=v{cur_rev}"),
                    (Some(ev_rev), None) => format!("rev=v{ev_rev}"),
                    (None, None) => "rev=-".to_owned(),
                };
                let _ = writeln!(
                    out,
                    "- work_id={} event={} authoritative_state={} {} delivered={} missing=[{}] pending_cards={} stale={}",
                    work.work_id,
                    event.kind.as_str(),
                    work.state,
                    rev_note,
                    work.delivered_artifact_ids.len(),
                    work.missing_items.join(","),
                    work.pending_cards.len(),
                    stale,
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "- work_id={} event={} authoritative_state=removed",
                    event.worker_id,
                    event.kind.as_str(),
                );
            }
        }
    }
    let (active_running, waiting_user) = sibling_work_counts(&works);
    let _ = writeln!(
        out,
        "[SiblingWorks active_running={active_running} waiting_user={waiting_user}]"
    );
    out
}

/// Schedule an asynchronous flush for `router_id` enforcing strict priority:
/// 1. Never auto-run during shutdown, when Router is busy, or while Router is
///    parked in `WaitingForUser` awaiting a choice card.
/// 2. Durable FIFO `pending_router_followups` strictly preempts `WorkEventBatch`.
/// 3. Coalesced `WorkEventBatch` runs only when follow-ups are empty,
///    `autonomous_drain` is enabled, no announcement is already in flight,
///    and no retry backoff is pending.
pub(super) fn schedule_router_inbox_flush(
    harness: &Arc<Harness>,
    router_id: &str,
) {
    if harness.state.shutdown_requested.load(Ordering::Acquire)
        || harness.is_initializing()
    {
        return;
    }
    {
        let mut inboxes = lock(&harness.state.router_inboxes);
        let inbox = inboxes.entry(router_id.to_owned()).or_default();
        if inbox.flush_scheduled {
            return;
        }
        inbox.flush_scheduled = true;
        drop(inboxes);
    }
    let weak = Arc::downgrade(harness);
    let router_id = router_id.to_owned();
    harness.runtime.spawn(async move {
        let Some(harness) = weak.upgrade() else {
            return;
        };
        {
            let mut inboxes = lock(&harness.state.router_inboxes);
            if let Some(inbox) = inboxes.get_mut(&router_id) {
                inbox.flush_scheduled = false;
            }
            drop(inboxes);
        }
        if harness.state.shutdown_requested.load(Ordering::Acquire)
            || !harness.can_run_turns()
        {
            return;
        }
        if harness.session_role(&router_id) != SessionRole::Router {
            return;
        }
        if harness
            .state
            .artifact_store
            .get()
            .is_some_and(|store| store.has_active_deletion_plan(&router_id))
        {
            return;
        }
        if harness.is_session_busy_or_active(&router_id)
            || harness.is_session_waiting_for_user(&router_id)
        {
            return;
        }

        // Priority 1: durable FIFO user follow-ups strictly preempt work-event announcements.
        let next_followup: Option<PendingRouterFollowup> = {
            let engine = lock(&harness.engine).clone();
            engine.and_then(|eng| eng.peek_router_followup(&router_id))
        };
        if let Some(followup) = next_followup {
            let submission = parse_submission_u128(&followup.submission_id);
            let res = harness.submit_message_targeted(
                Some(&router_id),
                &followup.text,
                submission,
                None,
                None,
                followup.reply_to_object_id.as_deref(),
                Some(ResumedSubmission {
                    skip_contract_evolve: true,
                    skip_push_user_line: true,
                    bypass_busy_queue: true,
                    ..ResumedSubmission::default()
                }),
            );
            if let Err(err) = res {
                tracing::debug!(
                    target: "flowy.router",
                    router_id = %router_id,
                    submission_id = %followup.submission_id,
                    err,
                    "deferred durable router follow-up remained queued"
                );
            }
            return;
        }

        // Priority 2: coalesced WorkEventBatch announcement (only when autonomous_drain is on).
        if !harness.state.autonomous_drain.load(Ordering::Acquire) {
            return;
        }
        let now = now_ms();
        let maybe_drained: Option<Vec<WorkEvent>> =
            lock(&harness.state.router_inboxes)
                .get_mut(&router_id)
                .and_then(|inbox| {
                    (!inbox.announcement_in_flight
                        && !inbox.events.is_empty()
                        && now >= inbox.retry_not_before_ms)
                        .then(|| {
                            inbox.announcement_in_flight = true;
                            inbox.retry_not_before_ms = 0;
                            let drained: Vec<WorkEvent> =
                                inbox.events.drain(..).collect();
                            inbox.in_flight.clone_from(&drained);
                            drained
                        })
                });
        let Some(drained) = maybe_drained else {
            return;
        };
        let batch_text =
            format_work_event_batch(&harness, &router_id, &drained);
        let mut bytes = [0_u8; 16];
        let submission = getrandom::fill(&mut bytes)
            .ok()
            .map(|()| u128::from_le_bytes(bytes));
        let res = harness.submit_message_targeted(
            Some(&router_id),
            &batch_text,
            submission,
            None,
            None,
            None,
            Some(ResumedSubmission {
                skip_contract_evolve: true,
                model_prompt_override: Some(batch_text.clone()),
                display_text_override: Some(String::new()),
                skip_push_user_line: true,
                prompt_source: MessageSource::SystemEvent {
                    event_kind: "work_event_batch".to_owned(),
                },
                is_router_announcement: true,
                bypass_busy_queue: true,
                ..ResumedSubmission::default()
            }),
        );
        if res.is_err() {
            let still_live = lock(&harness.state.live)
                .get(&router_id)
                .is_some_and(|s| s.role == SessionRole::Router);
            let mut inboxes = lock(&harness.state.router_inboxes);
            if still_live {
                let inbox = inboxes.entry(router_id).or_default();
                inbox.announcement_in_flight = false;
                inbox.in_flight.clear();
                inbox.requeue(drained);
            } else {
                inboxes.remove(&router_id);
            }
            drop(inboxes);
        }
    });
}

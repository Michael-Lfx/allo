//! Bounded, version-safe in-memory Work event inbox and priority-ordered
//! Router turn scheduler (`pending_router_followups` FIFO > `[WorkEventBatch]`).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;

use flowy_agent_core::{MessageSource, PendingRouterFollowup, SessionRole};

use super::{Harness, Ordering, ResumedSubmission, UiState, lock};

/// Maximum coalesced terminal Worker events buffered per Router inbox.
pub(super) const MAX_ROUTER_INBOX_EVENTS: usize = 32;

/// Maximum durable FIFO user follow-ups accepted while a Router is streaming.
pub(super) const MAX_ROUTER_FOLLOWUPS: usize = 16;

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
    }
    drop(inboxes);
}

fn parse_submission_u128(submission_id: &str) -> Option<u128> {
    let hex = submission_id.strip_prefix("submission-")?;
    u128::from_str_radix(hex, 16).ok()
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
    out
}

/// Schedule an asynchronous flush for `router_id` enforcing strict priority:
/// 1. Never auto-run during shutdown, when Router is busy, or while Router is
///    parked in `WaitingForUser` awaiting a choice card.
/// 2. Durable FIFO `pending_router_followups` strictly preempts `WorkEventBatch`.
/// 3. Coalesced `WorkEventBatch` runs only when follow-ups are empty,
///    `autonomous_drain` is enabled, and no announcement is already in flight.
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
                    contract: None,
                    skip_contract_evolve: true,
                    model_prompt_override: None,
                    model_prompt_suffix: None,
                    display_text_override: None,
                    skip_push_user_line: true,
                    preserve_turn_denial: false,
                    preserve_tool_approval: false,
                    prompt_source: MessageSource::User,
                    is_router_announcement: false,
                    bypass_busy_queue: true,
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
        let maybe_drained: Option<Vec<WorkEvent>> =
            lock(&harness.state.router_inboxes)
                .get_mut(&router_id)
                .and_then(|inbox| {
                    (!inbox.announcement_in_flight && !inbox.events.is_empty())
                        .then(|| {
                            inbox.announcement_in_flight = true;
                            inbox.events.drain(..).collect()
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
                contract: None,
                skip_contract_evolve: true,
                model_prompt_override: Some(batch_text.clone()),
                model_prompt_suffix: None,
                display_text_override: Some(String::new()),
                skip_push_user_line: true,
                preserve_turn_denial: false,
                preserve_tool_approval: false,
                prompt_source: MessageSource::SystemEvent {
                    event_kind: "work_event_batch".to_owned(),
                },
                is_router_announcement: true,
                bypass_busy_queue: true,
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
                for ev in drained.into_iter().rev() {
                    if inbox.events.len() < MAX_ROUTER_INBOX_EVENTS {
                        inbox.events.push_front(ev);
                    }
                }
            } else {
                inboxes.remove(&router_id);
            }
            drop(inboxes);
        }
    });
}

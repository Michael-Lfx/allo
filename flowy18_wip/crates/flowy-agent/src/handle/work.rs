//! Work-level status reconciliation, UI-decoupled cancellation, card actions, and graceful shutdown.

use super::{
    AcceptanceQueue, AcceptedMessage, AgentHandle, AgentUpdate, ArtifactStore,
    BTreeSet, CancelOutcome, CardAction, CardActionOutcome, ChatLine, Engine,
    MessageSource, Ordering, ReceiptState, Role, SessionRecord,
    SubmissionOutcome, TaskTarget, TurnState, TurnStatus, WorkStatus, lock,
    log_storage_io_error, now_ms, persist_index,
};

impl AgentHandle {
    /// Unified host view of a work item identified by either its `receipt_id`
    /// or its `submission_id`, combining persisted contract revision, terminal
    /// outcome, missing deliverables, and currently visible committed files.
    #[must_use]
    pub fn work_status(&self, work_id: &str) -> Option<WorkStatus> {
        let receipt = self.receipt_by_id(work_id);
        let mut session_id = receipt
            .as_ref()
            .and_then(|item| item.target_session_id.clone());
        let mut submission_id =
            receipt.as_ref().and_then(|item| item.submission_id.clone());
        let engine = lock(&self.inner.engine).clone();
        let record = engine.as_ref().and_then(|engine| {
            if let Some(sid) = session_id.as_deref() {
                engine.session(sid)
            } else {
                engine.sessions().into_iter().find(|rec| {
                    rec.submission_outcomes.contains_key(work_id)
                        || rec.submission_revisions.contains_key(work_id)
                        || rec
                            .delivery_commits
                            .iter()
                            .any(|c| c.submission_id == work_id)
                        || rec.messages.iter().any(|m| m.id == work_id)
                })
            }
        });
        if receipt.is_none() && record.is_none() {
            return None;
        }
        if let Some(rec) = &record {
            if session_id.is_none() {
                session_id = Some(rec.id.clone());
            }
            if submission_id.is_none() && receipt.is_none() {
                submission_id = Some(work_id.to_owned());
            }
        }
        Some(self.build_work_status(
            work_id,
            receipt,
            record,
            session_id,
            submission_id,
        ))
    }

    /// Unified host view of the latest work item in `session_id` (covering
    /// both queued receipts and direct `submit_chat` / `send` turns), without
    /// requiring callers to construct internal `submission-*` IDs.
    #[must_use]
    pub fn session_work_status(&self, session_id: &str) -> Option<WorkStatus> {
        let record = lock(&self.inner.engine)
            .as_ref()
            .and_then(|engine| engine.session(session_id))?;
        let submission_id = latest_session_submission_id(&record);
        if submission_id.is_none()
            && record.work_contract.is_none()
            && record.delivery_commits.is_empty()
        {
            return None;
        }
        let matched_receipt =
            AcceptanceQueue::new(self.inner.state.root.clone())
                .all()
                .ok()
                .and_then(|receipts| {
                    receipts
                        .into_iter()
                        .filter(|receipt| {
                            receipt.target_session_id.as_deref()
                                == Some(session_id)
                                && match submission_id.as_deref() {
                                    Some(sub) => {
                                        receipt.submission_id.as_deref()
                                            == Some(sub)
                                    }
                                    None => true,
                                }
                        })
                        .max_by_key(|receipt| receipt.created_at_ms)
                });
        let work_id = matched_receipt
            .as_ref()
            .map(|receipt| receipt.id.clone())
            .or_else(|| submission_id.clone())
            .unwrap_or_else(|| session_id.to_owned());
        Some(self.build_work_status(
            &work_id,
            matched_receipt,
            Some(record),
            Some(session_id.to_owned()),
            submission_id,
        ))
    }

    fn build_work_status(
        &self,
        work_id: &str,
        receipt: Option<AcceptedMessage>,
        record: Option<SessionRecord>,
        session_id: Option<String>,
        submission_id: Option<String>,
    ) -> WorkStatus {
        let receipt_state =
            receipt.as_ref().map(|item| item.state_at(now_ms()));
        let store = self.inner.state.artifact_store.get();
        let committed_ids = store
            .and_then(|s| s.committed_ids().ok())
            .unwrap_or_default();
        let mut delivered_artifact_ids = Vec::new();
        let mut contract_revision = None;
        let mut required_items = Vec::new();
        let mut outcome = None;
        let mut missing_items = Vec::new();
        if let Some(rec) = &record {
            contract_revision = submission_id
                .as_deref()
                .and_then(|sub| rec.submission_revisions.get(sub).copied())
                .or_else(|| rec.work_contract.as_ref().map(|c| c.revision));
            required_items = rec
                .work_contract
                .as_ref()
                .map_or_default(|c| c.required_items.clone());
            outcome = submission_id
                .as_deref()
                .and_then(|sub| rec.submission_outcomes.get(sub).cloned());
            for commit in &rec.delivery_commits {
                let belongs = match submission_id.as_deref() {
                    Some(sub) => {
                        commit.submission_id == sub
                            || commit
                                .submission_id
                                .starts_with("submission-override-")
                            || contract_revision.is_some_and(|rev| {
                                rec.submission_revisions
                                    .get(&commit.submission_id)
                                    .copied()
                                    == Some(rev)
                            })
                    }
                    None => true,
                };
                if belongs {
                    for artifact_id in &commit.artifact_ids {
                        if committed_ids.contains(artifact_id)
                            && !delivered_artifact_ids.contains(artifact_id)
                        {
                            delivered_artifact_ids.push(artifact_id.clone());
                        }
                    }
                }
            }
            if let Some(SubmissionOutcome::Partial { missing }) = &outcome {
                missing_items.clone_from(missing);
            } else if !matches!(outcome, Some(SubmissionOutcome::Completed))
                && let Some(contract) = &rec.work_contract
                && !contract.required_items.is_empty()
            {
                let delivered_meta: Vec<(String, String)> =
                    delivered_artifact_ids
                        .iter()
                        .filter_map(|id| {
                            store.and_then(|s| s.get(id).ok()).flatten().map(
                                |item| {
                                    (
                                        item.manifest.receipt.display_name,
                                        item.manifest.receipt.delivery_key,
                                    )
                                },
                            )
                        })
                        .collect();
                let pairs: Vec<(&str, &str)> = delivered_meta
                    .iter()
                    .map(|(n, k)| (n.as_str(), k.as_str()))
                    .collect();
                missing_items = contract.missing_deliverables(&pairs);
            }
        }
        WorkStatus {
            work_id: work_id.to_owned(),
            session_id,
            submission_id,
            contract_revision,
            receipt_state,
            outcome,
            required_items,
            delivered_artifact_ids,
            missing_items,
        }
    }

    /// Strongly-typed task cancellation decoupled from whichever session the
    /// UI is currently viewing. Never reads `active`.
    pub fn cancel_task(
        &self,
        target: TaskTarget,
    ) -> Result<CancelOutcome, &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        match target {
            TaskTarget::Session(session_id) => {
                self.cancel_session_locked(&session_id)
            }
            TaskTarget::Receipt(receipt_id) => {
                self.cancel_receipt_locked(&receipt_id)
            }
        }
    }

    /// Apply a structured card-level action inside the host kernel without
    /// coupling presentation widgets to storage or queue internals.
    pub fn apply_card_action(
        &self,
        action: CardAction,
    ) -> Result<CardActionOutcome, &'static str> {
        match action {
            CardAction::OverrideDeliver {
                session_id,
                report_id,
                artifact_path,
            } => {
                let artifact_id = self.override_deliver_artifact(
                    &session_id,
                    &report_id,
                    &artifact_path,
                )?;
                Ok(CardActionOutcome::ArtifactDelivered { artifact_id })
            }
            CardAction::RetryDelivery {
                session_id,
                submission_id,
            } => {
                if let Some(sub) = submission_id.as_deref() {
                    let has_commit = lock(&self.inner.engine)
                        .as_ref()
                        .and_then(|engine| engine.session(&session_id))
                        .is_some_and(|rec| {
                            rec.delivery_commits
                                .iter()
                                .any(|commit| commit.submission_id == sub)
                        });
                    if !has_commit {
                        return Err("chat.status_delivery_failed");
                    }
                }
                self.retry_delivery(Some(&session_id))?;
                Ok(CardActionOutcome::DeliveryRetried)
            }
            CardAction::RetryDeadReceipt { receipt_id } => {
                let queue = AcceptanceQueue::new(self.inner.state.root.clone());
                let Some(requeued) = queue
                    .retry_dead(&receipt_id, now_ms())
                    .map_err(|_| "notice.storage_error")?
                else {
                    return Err("notice.agent_failed");
                };
                self.inner.state.push_update(AgentUpdate::Receipt {
                    id: receipt_id,
                    state: ReceiptState::Cancelled,
                });
                self.inner.state.push_update(AgentUpdate::Receipt {
                    id: requeued.id.clone(),
                    state: ReceiptState::Queued,
                });
                if self.inner.state.autonomous_drain.load(Ordering::Acquire) {
                    self.kick_autonomous_drain();
                }
                Ok(CardActionOutcome::ReceiptRequeued {
                    receipt_id: requeued.id,
                })
            }
        }
    }

    /// Optional graceful-close hint: pauses autonomous draining, cancels
    /// cancellable busy turns (preserving finalizing deliveries), waits briefly
    /// for cancelled turns/tools to finish reaping, and flushes the UI index.
    /// Crash correctness never depends on calling this.
    pub fn prepare_shutdown(&self) {
        self.inner
            .state
            .autonomous_drain
            .store(false, Ordering::Release);
        self.inner.state.drain_cancel.cancel();
        let busy = self.busy_session_ids();
        let mut cancelled = Vec::new();
        for session_id in busy {
            if matches!(
                self.cancel_task(TaskTarget::Session(session_id.clone())),
                Ok(CancelOutcome::Cancelled)
            ) {
                cancelled.push(session_id);
            }
        }
        if !cancelled.is_empty()
            && let Some(engine) = lock(&self.inner.engine).clone()
        {
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(2);
            for session_id in cancelled {
                let remaining = deadline
                    .saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let _ = engine.wait_until_idle(&session_id, remaining);
            }
        }
        let _ = persist_index(&self.inner.state);
    }

    pub fn stop(&self) {
        let _dispatch = lock(&self.inner.state.dispatch);
        self.stop_locked();
    }

    pub fn stop_if_active(&self, expected: Option<&str>) {
        let _dispatch = lock(&self.inner.state.dispatch);
        if lock(&self.inner.state.active).as_deref() == expected {
            self.stop_locked();
        }
    }

    fn stop_locked(&self) {
        let Some(id) = lock(&self.inner.state.active).clone() else {
            return;
        };
        let _ = self.cancel_session_locked(&id);
    }

    fn cancel_session_locked(
        &self,
        id: &str,
    ) -> Result<CancelOutcome, &'static str> {
        let engine = lock(&self.inner.engine).clone();
        let live_busy = lock(&self.inner.state.live)
            .get(id)
            .is_some_and(|session| session.status.is_busy());
        let engine_busy = engine
            .as_ref()
            .and_then(|eng| eng.session(id))
            .is_some_and(|rec| rec.status.is_busy());
        if !live_busy && !engine_busy {
            return Ok(CancelOutcome::AlreadySettled);
        }
        if let Some(engine) = engine {
            match engine.request_cancel(id) {
                Ok(false) => {
                    return Ok(CancelOutcome::FinalizingNotCancellable);
                }
                Ok(true) => {}
                Err(error) => {
                    log_storage_io_error(
                        "cancel agent turn",
                        &flowy_agent_core::SessionIndex::path(
                            &self.inner.state.root,
                        ),
                        &error,
                    );
                    self.inner.state.report_storage_error();
                    return Err("notice.storage_error");
                }
            }
        }
        {
            let mut live = lock(&self.inner.state.live);
            if let Some(session) = live.get_mut(id) {
                session.title_signal.cancel();
                session.projection.flush();
                if session.status.is_busy() {
                    session.status = TurnStatus::Stopped;
                    session.projection.lines.push(ChatLine::Status {
                        text: session.status.key().into(),
                    });
                }
                session.turn_started_at = None;
                session.updated_at = now_ms();
            }
        }
        let matching_claim = {
            let mut slot = lock(&self.inner.state.accepted_claim);
            if slot.as_ref().is_some_and(|claim| claim.session_id == id) {
                slot.take()
            } else {
                None
            }
        };
        if let Some(claim) = matching_claim {
            claim.heartbeat_cancel.cancel();
            let queue = AcceptanceQueue::new(self.inner.state.root.clone());
            if queue.cancel(&claim.receipt_id, now_ms()).unwrap_or(false) {
                self.inner.state.push_update(AgentUpdate::Receipt {
                    id: claim.receipt_id,
                    state: ReceiptState::Cancelled,
                });
            }
        }
        if persist_index(&self.inner.state).is_err() {
            self.inner.state.report_storage_error();
        }
        self.inner
            .state
            .push_update(AgentUpdate::Session { id: id.to_owned() });
        Ok(CancelOutcome::Cancelled)
    }

    fn cancel_receipt_locked(
        &self,
        receipt_id: &str,
    ) -> Result<CancelOutcome, &'static str> {
        let active_session = lock(&self.inner.state.accepted_claim)
            .as_ref()
            .filter(|claim| claim.receipt_id == receipt_id)
            .map(|claim| claim.session_id.clone());
        if let Some(session_id) = active_session {
            return self.cancel_session_locked(&session_id);
        }
        let queue = AcceptanceQueue::new(self.inner.state.root.clone());
        match queue.cancel(receipt_id, now_ms()) {
            Ok(true) => {
                self.inner.state.push_update(AgentUpdate::Receipt {
                    id: receipt_id.to_owned(),
                    state: ReceiptState::Cancelled,
                });
                Ok(CancelOutcome::Cancelled)
            }
            Ok(false) => Ok(CancelOutcome::AlreadySettled),
            Err(_) => {
                self.inner.state.report_storage_error();
                Err("notice.storage_error")
            }
        }
    }
}

pub(super) fn latest_session_submission_id(
    record: &SessionRecord,
) -> Option<String> {
    record
        .messages
        .iter()
        .rev()
        .find(|msg| {
            msg.role == Role::User
                && (msg.id.starts_with("submission-")
                    || record.submission_outcomes.contains_key(&msg.id)
                    || record.submission_revisions.contains_key(&msg.id))
        })
        .map(|msg| msg.id.clone())
        .or_else(|| {
            record
                .delivery_commits
                .iter()
                .rev()
                .find(|commit| {
                    !commit.submission_id.starts_with("submission-override-")
                })
                .or_else(|| record.delivery_commits.last())
                .map(|commit| commit.submission_id.clone())
        })
        .or_else(|| {
            record
                .submission_revisions
                .iter()
                .max_by_key(|(_, rev)| **rev)
                .map(|(sub, _)| sub.clone())
        })
        .or_else(|| record.submission_outcomes.keys().next_back().cloned())
}

/// Returns `true` only when `submission_id` has an explicit durable
/// `Completed` outcome (and, if it produced a `DeliveryCommit`, every committed
/// artifact is already visible in `ArtifactStore`). Never mistakes an `Error`,
/// `Aborted`, `Partial`, or pre-run `User`-only prompt for a completed work.
pub(super) fn submission_committed_in_session(
    record: &SessionRecord,
    submission_id: &str,
    committed_artifacts: &BTreeSet<String>,
) -> bool {
    let commit_visible = |sub_id: &str| {
        record
            .delivery_commits
            .iter()
            .find(|commit| commit.submission_id == sub_id)
            .is_none_or(|commit| {
                commit.completed_successfully
                    && commit
                        .artifact_ids
                        .iter()
                        .all(|id| committed_artifacts.contains(id))
            })
    };
    if let Some(outcome) = record.submission_outcomes.get(submission_id) {
        return matches!(outcome, SubmissionOutcome::Completed)
            && commit_visible(submission_id);
    }
    if let Some(commit) = record
        .delivery_commits
        .iter()
        .find(|commit| commit.submission_id == submission_id)
    {
        return commit.completed_successfully
            && commit
                .artifact_ids
                .iter()
                .all(|id| committed_artifacts.contains(id));
    }
    // Legacy fallback for sessions written before `submission_outcomes`
    // existed: only an explicitly Completed session with post-prompt output
    // qualifies; Error / Stopped / Running never reconcile as Completed.
    if record.status != TurnState::Completed {
        return false;
    }
    let Some(user_idx) = record
        .messages
        .iter()
        .position(|msg| msg.id == submission_id && msg.role == Role::User)
    else {
        return false;
    };
    record.messages[user_idx + 1..]
        .iter()
        .take_while(|msg| !matches!(msg.source, MessageSource::User))
        .any(|msg| {
            msg.role == Role::Assistant
                || matches!(msg.source, MessageSource::Tool { .. })
        })
}

/// Re-evaluate any `Partial` submissions on `session_id` after a user override
/// delivers a previously missing required file.
pub(super) fn reconcile_partial_work_after_override(
    engine: &Engine,
    store: &ArtifactStore,
    session_id: &str,
) {
    let Some(record) = engine.session(session_id) else {
        return;
    };
    let Some(contract) = record.work_contract.as_ref() else {
        return;
    };
    if contract.required_items.is_empty() {
        return;
    }
    let Ok(committed_ids) = store.committed_ids() else {
        return;
    };
    let mut delivered: Vec<(String, String)> = Vec::new();
    for commit in &record.delivery_commits {
        for artifact_id in &commit.artifact_ids {
            if committed_ids.contains(artifact_id)
                && let Ok(Some(item)) = store.get(artifact_id)
            {
                delivered.push((
                    item.manifest.receipt.display_name,
                    item.manifest.receipt.delivery_key,
                ));
            }
        }
    }
    let pairs: Vec<(&str, &str)> = delivered
        .iter()
        .map(|(n, k)| (n.as_str(), k.as_str()))
        .collect();
    let missing = contract.missing_deliverables(&pairs);
    let partial_subs: Vec<String> = record
        .submission_outcomes
        .iter()
        .filter(|&(_, outcome)| {
            matches!(outcome, SubmissionOutcome::Partial { .. })
        })
        .map(|(sub_id, _)| sub_id.clone())
        .collect();
    for sub_id in partial_subs {
        let next = if missing.is_empty() {
            SubmissionOutcome::Completed
        } else {
            SubmissionOutcome::Partial {
                missing: missing.clone(),
            }
        };
        let _ = engine.set_submission_outcome(session_id, &sub_id, next);
    }
}

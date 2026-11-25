//! Work-level status reconciliation, UI-decoupled cancellation, card actions, and graceful shutdown.

use super::{
    AcceptanceQueue, AcceptedMessage, AgentHandle, AgentUpdate, ArtifactStore,
    BTreeSet, CancelOutcome, CardAction, CardActionOutcome, CardSelection,
    ChatLine, Engine, MessageSource, Ordering, PendingCard, ReceiptState,
    ResumedSubmission, Role, SessionRecord, SubmissionOutcome, TaskTarget,
    TurnState, TurnStatus, WorkStatus, lock, log_storage_io_error, now_ms,
    persist_index,
};
use crate::artifact_store::PublicationState;

impl AgentHandle {
    /// Unified host view of a work item identified by either its `receipt_id`
    /// or its `submission_id`, combining persisted contract revision, terminal
    /// outcome, missing deliverables, and currently visible committed files.
    #[must_use]
    pub fn work_status(&self, work_id: &str) -> Option<WorkStatus> {
        let receipt = self.receipt_by_id(work_id).or_else(|| {
            AcceptanceQueue::new(self.inner.state.root.clone())
                .all()
                .ok()
                .and_then(|receipts| {
                    receipts
                        .into_iter()
                        .filter(|item| {
                            item.submission_id.as_deref() == Some(work_id)
                        })
                        .max_by_key(|item| item.created_at_ms)
                })
        });
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

    /// Single-pass batch projection of work contracts for the Work Board:
    /// avoids `O(S + R)` replays of `acceptance/journal.jsonl`, `committed_ids`,
    /// and full `SessionRecord` clones on the UI thread.
    ///
    /// Returns `(by_session_id, by_work_id)`.
    #[must_use]
    pub fn board_work_contracts(
        &self,
        visible_receipts: &[(AcceptedMessage, ReceiptState)],
    ) -> (
        std::collections::HashMap<String, WorkStatus>,
        std::collections::HashMap<String, WorkStatus>,
    ) {
        use std::collections::HashMap;
        let store = self.inner.state.artifact_store.get();
        let committed_ids = store
            .and_then(|s| s.committed_ids().ok())
            .unwrap_or_default();
        let mut artifact_cache: HashMap<
            String,
            Option<(PublicationState, String, String)>,
        > = HashMap::new();
        let mut lookup_artifact = |id: &str| {
            artifact_cache
                .entry(id.to_owned())
                .or_insert_with(|| {
                    store.and_then(|s| s.get(id).ok()).flatten().map(|item| {
                        (
                            item.manifest.publication_state,
                            item.manifest.receipt.display_name,
                            item.manifest.receipt.delivery_key,
                        )
                    })
                })
                .clone()
        };

        let mut by_session: HashMap<String, WorkStatus> = HashMap::new();
        let mut by_work: HashMap<String, WorkStatus> = HashMap::new();
        let now = now_ms();

        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = &engine {
            engine.with_sessions(|records| {
                for rec in records {
                    let submission_id = latest_session_submission_id(rec);
                    if submission_id.is_none()
                        && rec.work_contract.is_none()
                        && rec.delivery_commits.is_empty()
                    {
                        continue;
                    }
                    let matched_receipt = visible_receipts
                        .iter()
                        .filter(|(receipt, _)| {
                            receipt.target_session_id.as_deref()
                                == Some(rec.id.as_str())
                                && match submission_id.as_deref() {
                                    Some(sub) => {
                                        receipt.submission_id.as_deref()
                                            == Some(sub)
                                    }
                                    None => true,
                                }
                        })
                        .max_by_key(|(receipt, _)| receipt.created_at_ms);
                    let work_id = matched_receipt
                        .map(|(r, _)| r.id.clone())
                        .or_else(|| submission_id.clone())
                        .unwrap_or_else(|| rec.id.clone());
                    let status = build_work_status_borrowed(
                        &work_id,
                        matched_receipt.map(|(_, st)| *st),
                        Some(rec),
                        Some(rec.id.clone()),
                        submission_id,
                        &committed_ids,
                        &mut lookup_artifact,
                    );
                    by_session.insert(rec.id.clone(), status);
                }

                for (receipt, state) in visible_receipts {
                    let session_id = receipt.target_session_id.clone();
                    let submission_id = receipt.submission_id.clone();
                    let rec = session_id
                        .as_deref()
                        .and_then(|sid| records.iter().find(|r| r.id == sid))
                        .or_else(|| {
                            submission_id.as_deref().and_then(|sub| {
                                records.iter().find(|r| {
                                    r.submission_outcomes.contains_key(sub)
                                        || r.submission_revisions
                                            .contains_key(sub)
                                        || r.delivery_commits
                                            .iter()
                                            .any(|c| c.submission_id == sub)
                                })
                            })
                        });
                    let resolved_sid =
                        session_id.or_else(|| rec.map(|r| r.id.clone()));
                    let status = build_work_status_borrowed(
                        &receipt.id,
                        Some(*state),
                        rec,
                        resolved_sid,
                        submission_id,
                        &committed_ids,
                        &mut lookup_artifact,
                    );
                    by_work.insert(receipt.id.clone(), status);
                }
            });
        } else {
            for (receipt, state) in visible_receipts {
                let status = WorkStatus {
                    work_id: receipt.id.clone(),
                    session_id: receipt.target_session_id.clone(),
                    submission_id: receipt.submission_id.clone(),
                    contract_revision: None,
                    receipt_state: Some(*state)
                        .or_else(|| Some(receipt.state_at(now))),
                    outcome: None,
                    required_items: Vec::new(),
                    delivered_artifact_ids: Vec::new(),
                    missing_items: Vec::new(),
                };
                by_work.insert(receipt.id.clone(), status);
            }
        }
        (by_session, by_work)
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
        let mut lookup_artifact = |id: &str| {
            store.and_then(|s| s.get(id).ok()).flatten().map(|item| {
                (
                    item.manifest.publication_state,
                    item.manifest.receipt.display_name,
                    item.manifest.receipt.delivery_key,
                )
            })
        };
        build_work_status_borrowed(
            work_id,
            receipt_state,
            record.as_ref(),
            session_id,
            submission_id,
            &committed_ids,
            &mut lookup_artifact,
        )
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
                let Some(requeued) =
                    queue.retry_dead(&receipt_id, now_ms()).map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?
                else {
                    return Err("notice.receipt_gave_up");
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
                    self.inner.kick_autonomous_drain();
                }
                Ok(CardActionOutcome::ReceiptRequeued {
                    receipt_id: requeued.id,
                })
            }
            CardAction::ResolveCard {
                session_id,
                card_id,
                selection,
            } => self.resolve_card_action(&session_id, &card_id, selection),
        }
    }

    fn resolve_card_action(
        &self,
        session_id: &str,
        card_id: &str,
        selection: CardSelection,
    ) -> Result<CardActionOutcome, &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        if self.is_initializing() {
            return Err("library.loading");
        }
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let record = engine.session(session_id).ok_or("notice.agent_failed")?;
        let Some(card) = record
            .pending_cards
            .iter()
            .find(|item| item.card_id() == card_id)
            .cloned()
        else {
            return Err("notice.card_not_found");
        };
        match (card, selection) {
            (
                PendingCard::Disambiguation {
                    prompt,
                    impact_boundary,
                    options,
                    allow_multi,
                    ..
                },
                CardSelection::Disambiguate { object_ids },
            ) => {
                if object_ids.is_empty() {
                    return Err("notice.card_selection_empty");
                }
                if !allow_multi && object_ids.len() > 1 {
                    return Err("notice.card_selection_invalid");
                }
                let valid = object_ids
                    .iter()
                    .all(|oid| options.iter().any(|opt| opt.object_id == *oid));
                if !valid {
                    return Err("notice.card_selection_invalid");
                }
                let prev_contract = record
                    .work_contract
                    .as_ref()
                    .ok_or("notice.card_selection_invalid")?;
                let updated_brief =
                    crate::verify::apply_disambiguation_selection(
                        prev_contract,
                        &prompt,
                        &object_ids,
                        now_ms(),
                    )?;
                let updated_wc = updated_brief.to_work_contract();
                engine
                    .take_pending_card(
                        session_id,
                        card_id,
                        TurnState::Idle,
                        Some(updated_wc),
                    )
                    .map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?;
                {
                    let mut live = lock(&self.inner.state.live);
                    if let Some(session) = live.get_mut(session_id) {
                        session.status = TurnStatus::Idle;
                        session.projection.lines.retain(|line| {
                            !matches!(
                                line,
                                ChatLine::Status { text }
                                    if text == TurnStatus::WaitingForUser.key()
                            )
                        });
                    }
                }
                let selected_labels: Vec<String> = options
                    .iter()
                    .filter(|opt| object_ids.contains(&opt.object_id))
                    .map(|opt| opt.label.clone())
                    .collect();
                let model_prompt = format!(
                    "{prompt}\n\n[已确认修改目标：{}；影响边界：{impact_boundary}]",
                    selected_labels.join("、")
                );
                let mut bytes = [0_u8; 16];
                getrandom::fill(&mut bytes)
                    .map_err(|_| "notice.agent_failed")?;
                let submission = u128::from_le_bytes(bytes);
                self.inner.submit_message_targeted_locked(
                    Some(session_id),
                    &prompt,
                    Some(submission),
                    None,
                    None,
                    None,
                    Some(ResumedSubmission {
                        contract: Some(updated_brief),
                        model_prompt_override: Some(model_prompt),
                        skip_push_user_line: true,
                    }),
                )?;
                Ok(CardActionOutcome::CardResolved {
                    card_id: card_id.to_owned(),
                    resumed: true,
                })
            }
            (
                PendingCard::ToolApproval {
                    args_hash: expected_hash,
                    ..
                },
                CardSelection::ApproveTool { args_hash },
            ) => {
                if args_hash != expected_hash {
                    return Err("notice.approval_hash_mismatch");
                }
                engine
                    .take_pending_card(
                        session_id,
                        card_id,
                        TurnState::Idle,
                        None,
                    )
                    .map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?;
                if let Some(session) =
                    lock(&self.inner.state.live).get_mut(session_id)
                {
                    session.status = TurnStatus::Idle;
                    session.updated_at = now_ms();
                }
                if persist_index(&self.inner.state).is_err() {
                    self.inner.state.report_storage_error();
                }
                self.inner.state.push_update(AgentUpdate::Session {
                    id: session_id.to_owned(),
                });
                Ok(CardActionOutcome::CardResolved {
                    card_id: card_id.to_owned(),
                    resumed: false,
                })
            }
            (
                PendingCard::ToolApproval { .. },
                CardSelection::DenyTool { .. },
            ) => {
                engine
                    .take_pending_card(
                        session_id,
                        card_id,
                        TurnState::Stopped,
                        None,
                    )
                    .map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?;
                if let Some(session) =
                    lock(&self.inner.state.live).get_mut(session_id)
                {
                    session.status = TurnStatus::Stopped;
                    session.updated_at = now_ms();
                }
                if persist_index(&self.inner.state).is_err() {
                    self.inner.state.report_storage_error();
                }
                self.inner.state.push_update(AgentUpdate::Session {
                    id: session_id.to_owned(),
                });
                Ok(CardActionOutcome::CardResolved {
                    card_id: card_id.to_owned(),
                    resumed: false,
                })
            }
            _ => Err("notice.card_selection_invalid"),
        }
    }

    /// Optional graceful-close hint: pauses autonomous draining, cancels
    /// cancellable busy turns (preserving and waiting briefly for finalizing
    /// deliveries), flushes the UI index before and after reaping, and returns
    /// within the bounded timeout. Crash correctness never depends on calling
    /// this.
    pub fn prepare_shutdown(&self) {
        self.prepare_shutdown_with_timeout(std::time::Duration::from_secs(2));
    }

    /// Bounded variant of [`Self::prepare_shutdown`] so UI frameworks with
    /// strict quit deadlines can align the grace wait without losing the
    /// initial `persist_index` flush.
    pub fn prepare_shutdown_with_timeout(&self, timeout: std::time::Duration) {
        self.inner
            .state
            .autonomous_drain
            .store(false, Ordering::Release);
        self.inner.state.drain_cancel.cancel();
        // Persist immediately so index updates survive even if the host exits
        // before child-process reaping completes.
        let _ = persist_index(&self.inner.state);
        let mut to_wait: BTreeSet<String> =
            self.busy_session_ids().into_iter().collect();
        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = &engine {
            for active_id in engine.active_session_ids() {
                to_wait.insert(active_id);
            }
        }
        let mut wait_list = Vec::new();
        for session_id in to_wait {
            if self.inner.is_session_waiting_for_user(&session_id) {
                continue;
            }
            match self.cancel_task(TaskTarget::Session(session_id.clone())) {
                Ok(
                    CancelOutcome::Cancelled
                    | CancelOutcome::FinalizingNotCancellable,
                ) => {
                    // Wait for both cancelled turns reaping child processes AND
                    // finalizing deliveries finishing their atomic commit.
                    wait_list.push(session_id);
                }
                Ok(CancelOutcome::AlreadySettled) => {
                    // May still be reaping after an earlier cancel.
                    if engine
                        .as_ref()
                        .is_some_and(|eng| eng.is_active(&session_id))
                    {
                        wait_list.push(session_id);
                    }
                }
                Err(_) => {}
            }
        }
        if !wait_list.is_empty()
            && !timeout.is_zero()
            && let Some(engine) = engine
        {
            let deadline = std::time::Instant::now() + timeout;
            for session_id in wait_list {
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
        let engine_busy = engine.as_ref().is_some_and(|eng| {
            eng.session(id).is_some_and(|rec| rec.status.is_busy())
                || (eng.is_active(id) && !eng.is_cancelling(id))
        });
        if !live_busy && !engine_busy {
            return Ok(CancelOutcome::AlreadySettled);
        }
        let mut storage_err: Option<&'static str> = None;
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
                    // `request_cancel` already fired `fence.cancel_active()`
                    // before persisting `history.json`, so still stop live UI
                    // state and cancel any owned receipt claim before returning
                    // the storage error (prevents stuck Running UI + orphaned
                    // receipt state).
                    storage_err = Some("notice.session_storage_failed");
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
        self.inner.state.push_update(AgentUpdate::Sessions);
        if let Some(err) = storage_err {
            return Err(err);
        }
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
                Err("notice.session_storage_failed")
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

/// Returns `true` when `submission_id` has crossed its durable LLM commit point.
///
/// Matches `SubmissionOutcome::Completed | Partial` or a successful
/// `DeliveryCommit` in `history.json`. Even if the Library marker still needs
/// `retry_delivery`, the LLM turn must never be re-executed on restart. Never
/// mistakes an `Error`, `Aborted`, or pre-run `User`-only prompt for committed.
pub(super) fn submission_committed_in_session(
    record: &SessionRecord,
    submission_id: &str,
) -> bool {
    if let Some(outcome) = record.submission_outcomes.get(submission_id) {
        return matches!(
            outcome,
            SubmissionOutcome::Completed | SubmissionOutcome::Partial { .. }
        );
    }
    if let Some(commit) = record
        .delivery_commits
        .iter()
        .find(|commit| commit.submission_id == submission_id)
    {
        return commit.completed_successfully;
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

/// Re-evaluate `Partial` submissions for `target_revision` after an override.
///
/// `extra_delivered` passes a single reused snapshot's `(display_name,
/// delivery_key)` without mutating a prior turn's `submission_revisions`.
pub(super) fn reconcile_partial_work_after_override(
    engine: &Engine,
    store: &ArtifactStore,
    session_id: &str,
    target_revision: Option<u32>,
    extra_delivered: Option<(&str, &str)>,
) {
    reconcile_partial_work_inner(
        engine,
        store,
        session_id,
        target_revision,
        extra_delivered,
    );
}

pub(super) fn reconcile_partial_work_for_revision(
    engine: &Engine,
    store: &ArtifactStore,
    session_id: &str,
    target_revision: Option<u32>,
) {
    reconcile_partial_work_inner(
        engine,
        store,
        session_id,
        target_revision,
        None,
    );
}

fn missing_item_satisfied(
    missing_label: &str,
    delivered: &[(&str, &str)],
) -> bool {
    let label = missing_label.trim();
    if label.is_empty() {
        return false;
    }
    let lower = label.to_ascii_lowercase();
    delivered.iter().any(|(name, key)| {
        let name_lower = name.to_ascii_lowercase();
        let key_lower = key.to_ascii_lowercase();
        if let Some(ext) = lower.strip_prefix("*.") {
            let suffix = format!(".{ext}");
            name_lower.ends_with(&suffix) || key_lower.ends_with(&suffix)
        } else if lower.contains('.') {
            name_lower == lower || key_lower == lower
        } else {
            let suffix = format!(".{lower}");
            name_lower.ends_with(&suffix) || key_lower.ends_with(&suffix)
        }
    })
}

fn reconcile_partial_work_inner(
    engine: &Engine,
    store: &ArtifactStore,
    session_id: &str,
    target_revision: Option<u32>,
    extra_delivered: Option<(&str, &str)>,
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
    let active_rev = target_revision.unwrap_or(contract.revision);
    let Ok(committed_ids) = store.committed_ids() else {
        return;
    };
    let mut delivered: Vec<(String, String)> = Vec::new();
    for commit in &record.delivery_commits {
        let commit_rev = record
            .submission_revisions
            .get(&commit.submission_id)
            .copied();
        if commit_rev != Some(active_rev) {
            continue;
        }
        for artifact_id in &commit.artifact_ids {
            if committed_ids.contains(artifact_id)
                && let Ok(Some(item)) = store.get(artifact_id)
                && !matches!(
                    item.manifest.publication_state,
                    PublicationState::Deleted | PublicationState::DeletePending
                )
            {
                delivered.push((
                    item.manifest.receipt.display_name,
                    item.manifest.receipt.delivery_key,
                ));
            }
        }
    }
    if let Some((name, key)) = extra_delivered {
        delivered.push((name.to_owned(), key.to_owned()));
    }
    let pairs: Vec<(&str, &str)> = delivered
        .iter()
        .map(|(n, k)| (n.as_str(), k.as_str()))
        .collect();
    let legacy_untagged = record.submission_revisions.is_empty()
        && active_rev == contract.revision;
    let partial_subs: Vec<(String, Vec<String>)> = record
        .submission_outcomes
        .iter()
        .filter_map(|(sub_id, outcome)| {
            let SubmissionOutcome::Partial { missing } = outcome else {
                return None;
            };
            let matches_rev = match record.submission_revisions.get(sub_id) {
                Some(&rev) => rev == active_rev,
                None => legacy_untagged,
            };
            matches_rev.then(|| (sub_id.clone(), missing.clone()))
        })
        .collect();
    if partial_subs.is_empty() {
        return;
    }
    let current_rev_missing = (active_rev == contract.revision)
        .then(|| contract.missing_deliverables(&pairs));
    for (sub_id, old_missing) in partial_subs {
        let missing = if let Some(computed) = &current_rev_missing {
            computed.clone()
        } else {
            // Older revision after `record.work_contract` evolved: narrow the
            // recorded `Partial { missing }` list against `active_rev`'s
            // delivered items rather than injecting newer-revision items.
            old_missing
                .into_iter()
                .filter(|item| !missing_item_satisfied(item, &pairs))
                .collect()
        };
        let next = if missing.is_empty() {
            SubmissionOutcome::Completed
        } else {
            SubmissionOutcome::Partial { missing }
        };
        let _ = engine.set_submission_outcome(session_id, &sub_id, next);
    }
}

fn build_work_status_borrowed<F>(
    work_id: &str,
    receipt_state: Option<ReceiptState>,
    record: Option<&SessionRecord>,
    session_id: Option<String>,
    submission_id: Option<String>,
    committed_ids: &BTreeSet<String>,
    lookup_artifact: &mut F,
) -> WorkStatus
where
    F: FnMut(&str) -> Option<(PublicationState, String, String)>,
{
    let mut delivered_artifact_ids = Vec::new();
    let mut contract_revision = None;
    let mut required_items = Vec::new();
    let mut outcome = None;
    let mut missing_items = Vec::new();
    if let Some(rec) = record {
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
                        && lookup_artifact(artifact_id).is_none_or(
                            |(pub_state, _, _)| {
                                !matches!(
                                    pub_state,
                                    PublicationState::Deleted
                                        | PublicationState::DeletePending
                                )
                            },
                        )
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
            let delivered_meta: Vec<(String, String)> = delivered_artifact_ids
                .iter()
                .filter_map(|id| {
                    lookup_artifact(id).map(|(_, name, key)| (name, key))
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

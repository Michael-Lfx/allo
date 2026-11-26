//! Durable acceptance queue, lease heartbeat, autonomous draining, and startup reconciliation.

#[cfg(test)]
use super::AcceptedSubmitHook;
use super::{
    ACCEPTED_MAX_ATTEMPTS, ACCEPTED_RELEASE_BASE_MS, ACCEPTED_RELEASE_MAX_MS,
    AcceptanceQueue, AcceptedClaim, AcceptedMessage, AcceptedRun, AgentHandle,
    AgentUpdate, Arc, ArtifactStore, CancelToken, Engine, Harness, Ordering,
    Path, RECEIPT_SUPERSEDED_ERR, ReceiptState, TurnStatus, UiState, lock,
    now_ms, submission_committed_in_session,
};

impl AgentHandle {
    /// Accept a prompt that cannot run yet, and hand back a durable receipt.
    ///
    /// Unlike [`Self::submit_message`], this never answers `agent_busy`. A
    /// message that arrives while a turn is running is still accepted; it only
    /// has to wait. The receipt lives in its own file, so a restart can still
    /// show what was accepted before anything ran it.
    pub fn accept_deferred(
        &self,
        text: &str,
    ) -> Result<AcceptedMessage, &'static str> {
        let text = text.trim();
        if text.is_empty() {
            return Err("notice.prompt_empty");
        }
        if self.is_initializing() {
            return Err("library.loading");
        }
        // Accepting must never create a session. `ensure_active_session()`
        // does exactly that when nothing is live, which would pin the receipt
        // to an empty conversation the user never had. The session is a hint
        // here, not a binding (see `AcceptedMessage::session_hint`).
        // Always release `active` before acquiring `live` (lock ordering).
        let active = lock(&self.inner.state.active).clone();
        let hint =
            active.filter(|id| lock(&self.inner.state.live).contains_key(id));
        let receipt = AcceptanceQueue::new(self.inner.state.root.clone())
            .accept(hint.as_deref(), text, now_ms())
            .map_err(|_| {
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        let root_session_id =
            hint.as_deref().map(|id| self.root_session_id_for(id));
        tracing::info!(
            target: "flowy.acceptance",
            event = "accepted",
            receipt_id = %receipt.id,
            session_id = hint.as_deref().unwrap_or(""),
            root_session_id = root_session_id.as_deref().unwrap_or(""),
            "accepted prompt into durable queue"
        );
        self.inner.state.push_update(AgentUpdate::Receipt {
            id: receipt.id.clone(),
            state: ReceiptState::Queued,
        });
        if self.inner.state.autonomous_drain.load(Ordering::Acquire) {
            self.inner.kick_autonomous_drain();
        }
        Ok(receipt)
    }

    /// Receipts accepted but not picked up yet, oldest first.
    #[must_use]
    pub fn pending_acceptances(&self) -> Vec<AcceptedMessage> {
        AcceptanceQueue::new(self.inner.state.root.clone())
            .pending()
            .unwrap_or_default()
    }

    /// All receipts that are neither `Completed` nor `Cancelled` (including
    /// `Queued`, `Running`, `Retrying` in backoff, `Quarantined`, and `Dead`),
    /// sorted by `created_at_ms` ascending.
    #[must_use]
    pub fn visible_acceptances(&self) -> Vec<(AcceptedMessage, ReceiptState)> {
        AcceptanceQueue::new(self.inner.state.root.clone())
            .visible()
            .unwrap_or_default()
    }

    /// Look up any accepted receipt by id across all lifecycle states
    /// (`Queued`, `Running`, `Retrying`, `Completed`, `Cancelled`,
    /// `Quarantined`, `Dead`).
    #[must_use]
    pub fn receipt_by_id(&self, id: &str) -> Option<AcceptedMessage> {
        AcceptanceQueue::new(self.inner.state.root.clone())
            .get(id)
            .ok()
            .flatten()
    }

    /// Current lifecycle state of receipt `id`, if it exists in the journal.
    #[must_use]
    pub fn receipt_state(&self, id: &str) -> Option<ReceiptState> {
        self.receipt_by_id(id)
            .map(|receipt| receipt.state_at(now_ms()))
    }

    /// Claim and start the oldest accepted receipt, if any.
    ///
    /// Reconciles any previously bound receipt whose engine turn already
    /// committed on disk before claiming new work, binds `(session_id,
    /// submission_id)` fail-closed before execution, and submits directly to
    /// the bound session without mutating the UI's active selection.
    pub fn run_accepted_once(
        &self,
    ) -> Result<Option<AcceptedRun>, &'static str> {
        self.inner.run_accepted_once()
    }

    #[cfg(test)]
    pub(crate) fn set_accepted_lease_ms_for_test(&self, lease_ms: u64) {
        self.inner
            .state
            .accepted_lease_ms
            .store(lease_ms, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn set_before_accepted_bind_hook(
        &self,
        hook: Option<AcceptedSubmitHook>,
    ) {
        *lock(&self.inner.state.before_accepted_bind_hook) = hook;
    }

    #[cfg(test)]
    pub(crate) fn set_before_accepted_submit_hook(
        &self,
        hook: Option<AcceptedSubmitHook>,
    ) {
        *lock(&self.inner.state.before_accepted_submit_hook) = hook;
    }

    /// Finish the receipt started by [`Self::run_accepted_once`].
    ///
    /// Returns `true` when the retry budget was exhausted and the receipt was
    /// buried instead of released.
    pub fn finish_accepted(
        &self,
        completed: bool,
    ) -> Result<bool, &'static str> {
        self.inner.finish_accepted(completed)
    }

    /// Enable or disable kernel-owned autonomous queue draining on the host
    /// Tokio runtime (no UI polling required). Defaults to `false` so legacy
    /// caller-driven loops remain deterministic.
    pub fn set_autonomous_drain(&self, enabled: bool) {
        self.inner
            .state
            .autonomous_drain
            .store(enabled, Ordering::Release);
        if enabled {
            self.inner.kick_autonomous_drain();
        }
    }

    #[must_use]
    pub fn autonomous_drain(&self) -> bool {
        self.inner.state.autonomous_drain.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn has_active_claim(&self) -> bool {
        self.inner.has_active_claim()
    }

    /// Status of the session that owns the active receipt claim, independent of
    /// whichever session the user is currently viewing.
    #[must_use]
    pub fn claimed_session_status(&self) -> Option<TurnStatus> {
        let session_id = lock(&self.inner.state.accepted_claim)
            .as_ref()
            .map(|claim| claim.session_id.clone())?;
        lock(&self.inner.state.live)
            .get(&session_id)
            .map(|session| session.status.clone())
    }

    /// The attempt number of the receipt currently claimed, if any.
    #[must_use]
    pub fn accepted_attempts(&self) -> Option<u32> {
        lock(&self.inner.state.accepted_claim)
            .as_ref()
            .map(|claim| claim.attempts)
    }
}

impl Harness {
    pub(super) fn has_active_claim(&self) -> bool {
        lock(&self.state.accepted_claim).is_some()
    }

    pub(super) fn run_accepted_once(
        self: &Arc<Self>,
    ) -> Result<Option<AcceptedRun>, &'static str> {
        let _drain = lock(&self.state.accept_drain);
        if !self.can_run_turns()
            || self.has_active_claim()
            || self.has_busy_session()
        {
            return Ok(None);
        }
        let engine = lock(&self.engine).clone();
        if let Some(engine) = engine {
            push_reconciled_receipt_updates(
                &self.state,
                reconcile_committed_acceptances(
                    &self.state.root,
                    &engine,
                    self.state.artifact_store.get(),
                ),
            );
        }
        let state = &self.state;
        let queue = AcceptanceQueue::new(state.root.clone());
        let Some(next) = queue
            .pending()
            .map_err(|_| "notice.session_storage_failed")?
            .into_iter()
            .find(|receipt| {
                let target = receipt
                    .target_session_id
                    .as_deref()
                    .or(receipt.session_hint.as_deref());
                match target {
                    // A receipt belongs to the session it was bound/submitted
                    // from. While that session is running, reaping after cancel,
                    // or parked in WaitingForUser, wait for it instead of taking
                    // the receipt and inflating its attempt count.
                    Some(sid) => {
                        !self.is_session_busy_or_active(sid)
                            && !self.is_session_waiting_for_user(sid)
                    }
                    None => true,
                }
            })
        else {
            return Ok(None);
        };
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| "notice.agent_failed")?;
        let submission = u128::from_le_bytes(bytes);
        let attempt_id = format!("attempt-{submission:032x}");
        let submission_id = format!("submission-{submission:032x}");
        let lease_ms = state.accepted_lease_ms.load(Ordering::Acquire).max(50);
        let now = now_ms();
        let Some(claimed) = queue
            .take(&next.id, &attempt_id, now, lease_ms)
            .map_err(|_| "notice.session_storage_failed")?
        else {
            return Ok(None);
        };
        let session = match claimed
            .target_session_id
            .as_deref()
            .or(claimed.session_hint.as_deref())
        {
            Some(target) if lock(&state.live).contains_key(target) => {
                if self.is_session_busy_or_active(target)
                    || self.is_session_waiting_for_user(target)
                {
                    // Lost the race after selection: wait with 0 ms backoff,
                    // do not fork.
                    let _ =
                        queue.release(&claimed.id, &attempt_id, now_ms(), 0);
                    return Ok(None);
                }
                target.to_owned()
            }
            _ => match self.create_background_home_session() {
                Ok(session) => session,
                Err(error) => {
                    settle_and_notify_failed_attempt(
                        state,
                        &queue,
                        &claimed.id,
                        &attempt_id,
                        claimed.attempts,
                    );
                    return Err(error);
                }
            },
        };
        #[cfg(test)]
        {
            let hook = lock(&state.before_accepted_bind_hook).clone();
            if let Some(hook) = hook {
                hook(&claimed.id);
            }
        }
        // Persist (session_id, submission_id) fail-closed before starting the
        // engine: a write error or stale lease must never dispatch work.
        match queue.bind(
            &claimed.id,
            &attempt_id,
            &session,
            &submission_id,
            now_ms(),
        ) {
            Ok(true) => {}
            Ok(false) => return Ok(None),
            Err(_) => {
                settle_and_notify_failed_attempt(
                    state,
                    &queue,
                    &claimed.id,
                    &attempt_id,
                    claimed.attempts,
                );
                return Err("notice.session_storage_failed");
            }
        }
        #[cfg(test)]
        {
            let hook = lock(&state.before_accepted_submit_hook).clone();
            if let Some(hook) = hook {
                hook(&session);
            }
        }
        let heartbeat_cancel = CancelToken::new();
        let pending_claim = AcceptedClaim {
            receipt_id: claimed.id.clone(),
            attempt_id: attempt_id.clone(),
            session_id: session.clone(),
            submission_id: submission_id.clone(),
            attempts: claimed.attempts,
            heartbeat_cancel: heartbeat_cancel.clone(),
        };
        // `submit_message_targeted` re-checks receipt liveness inside
        // `dispatch`, starts the engine, transitions `live[session].status` to
        // `Running`, and only then publishes `accepted_claim` +
        // `AgentUpdate::Receipt(Running)` — closing both TOCTOU windows.
        if let Err(error) = self.submit_message_targeted(
            Some(&session),
            &claimed.text,
            Some(submission),
            None,
            Some(pending_claim),
            None,
        ) {
            heartbeat_cancel.cancel();
            if error == RECEIPT_SUPERSEDED_ERR {
                return Ok(None);
            }
            if error == "notice.agent_busy" || error == "notice.card_pending" {
                if queue
                    .release(&claimed.id, &attempt_id, now_ms(), 0)
                    .unwrap_or(false)
                {
                    state.push_update(AgentUpdate::Receipt {
                        id: claimed.id.clone(),
                        state: ReceiptState::Queued,
                    });
                }
                return Ok(None);
            }
            settle_and_notify_failed_attempt(
                state,
                &queue,
                &claimed.id,
                &attempt_id,
                claimed.attempts,
            );
            return Err(error);
        }
        if !heartbeat_cancel.is_cancelled() {
            let hb_queue = AcceptanceQueue::new(state.root.clone());
            let hb_receipt = claimed.id.clone();
            let hb_attempt = attempt_id.clone();
            let interval_ms = (lease_ms / 3).max(20);
            self.runtime.spawn(async move {
                let mut ticker = tokio::time::interval(
                    std::time::Duration::from_millis(interval_ms),
                );
                ticker.tick().await;
                loop {
                    tokio::select! {
                        () = heartbeat_cancel.cancelled() => break,
                        _ = ticker.tick() => {
                            match hb_queue.renew_lease(
                                &hb_receipt,
                                &hb_attempt,
                                now_ms(),
                                lease_ms,
                            ) {
                                Ok(true) => {}
                                Ok(false) => break,
                                Err(error) => {
                                    tracing::warn!(
                                        target: "flowy.acceptance",
                                        receipt_id = %hb_receipt,
                                        attempt_id = %hb_attempt,
                                        %error,
                                        "lease heartbeat renewal failed"
                                    );
                                }
                            }
                        }
                    }
                }
            });
        }
        let root_session_id = self.root_session_id_for(&session);
        tracing::info!(
            target: "flowy.acceptance",
            event = "claimed",
            receipt_id = %claimed.id,
            attempt_id = %attempt_id,
            attempts = claimed.attempts,
            session_id = %session,
            root_session_id = %root_session_id,
            submission_id = %submission_id,
            "claimed accepted receipt"
        );
        Ok(Some(AcceptedRun {
            receipt_id: claimed.id,
            attempt_id,
            session_id: session,
            submission,
        }))
    }

    pub(super) fn finish_accepted(
        &self,
        completed: bool,
    ) -> Result<bool, &'static str> {
        self.finish_accepted_locked(None, None, completed)
    }

    pub(super) fn finish_accepted_if_owned(
        &self,
        session_id: &str,
        submission_id: Option<&str>,
        completed: bool,
    ) -> Result<bool, &'static str> {
        self.finish_accepted_locked(Some(session_id), submission_id, completed)
    }

    fn finish_accepted_locked(
        &self,
        expected_session: Option<&str>,
        expected_submission: Option<&str>,
        completed: bool,
    ) -> Result<bool, &'static str> {
        let _drain = lock(&self.state.accept_drain);
        let _dispatch = lock(&self.state.dispatch);
        let claim = lock(&self.state.accepted_claim).clone();
        let Some(claim) = claim else {
            return Ok(false);
        };
        if let Some(session_id) = expected_session
            && (claim.session_id != session_id
                || expected_submission != Some(claim.submission_id.as_str()))
        {
            return Ok(false);
        }
        let queue = AcceptanceQueue::new(self.state.root.clone());
        let now = now_ms();
        let gave_up = !completed && claim.attempts >= ACCEPTED_MAX_ATTEMPTS;
        let outcome = if completed {
            "completed"
        } else if gave_up {
            "gave_up"
        } else {
            "released"
        };
        let result = if completed {
            queue.complete(&claim.receipt_id, &claim.attempt_id, now)
        } else if gave_up {
            queue.give_up(&claim.receipt_id, &claim.attempt_id, now)
        } else {
            queue.release(
                &claim.receipt_id,
                &claim.attempt_id,
                now,
                release_backoff(claim.attempts),
            )
        };
        match result {
            Ok(applied) => {
                claim.heartbeat_cancel.cancel();
                {
                    let mut slot = lock(&self.state.accepted_claim);
                    if slot.as_ref().is_some_and(|current| {
                        current.receipt_id == claim.receipt_id
                            && current.attempt_id == claim.attempt_id
                    }) {
                        *slot = None;
                    }
                }
                if applied {
                    let root_session_id =
                        self.root_session_id_for(&claim.session_id);
                    tracing::info!(
                        target: "flowy.acceptance",
                        event = outcome,
                        receipt_id = %claim.receipt_id,
                        attempt_id = %claim.attempt_id,
                        attempts = claim.attempts,
                        session_id = %claim.session_id,
                        root_session_id = %root_session_id,
                        "settled accepted receipt"
                    );
                    let next_state = if completed {
                        ReceiptState::Completed
                    } else if gave_up {
                        ReceiptState::Dead
                    } else {
                        ReceiptState::Retrying
                    };
                    self.state.push_update(AgentUpdate::Receipt {
                        id: claim.receipt_id,
                        state: next_state,
                    });
                }
                Ok(applied && gave_up)
            }
            Err(_) => Err("notice.session_storage_failed"),
        }
    }

    pub(super) fn kick_autonomous_drain(self: &Arc<Self>) {
        if !self.state.autonomous_drain.load(Ordering::Acquire)
            || self.is_initializing()
        {
            return;
        }
        if self
            .state
            .drain_wakeup_scheduled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let weak = Arc::downgrade(self);
        self.runtime.spawn(async move {
            let Some(harness) = weak.upgrade() else {
                return;
            };
            harness
                .state
                .drain_wakeup_scheduled
                .store(false, Ordering::Release);
            if !harness.state.autonomous_drain.load(Ordering::Acquire) {
                return;
            }
            let needs_wakeup = match harness.run_accepted_once() {
                Ok(Some(_)) => !harness.has_active_claim(),
                Ok(None) => true,
                Err(error) => {
                    tracing::warn!(
                        target: "flowy.acceptance",
                        error,
                        "autonomous drain could not start next receipt"
                    );
                    true
                }
            };
            if !needs_wakeup {
                return;
            }
            let delay_ms = harness.next_autonomous_wakeup_delay_ms();
            let Some(delay_ms) = delay_ms else {
                return;
            };
            let target_ms = now_ms().saturating_add(delay_ms).max(1);
            let mut scheduled = false;
            let mut observed =
                harness.state.drain_sleep_until_ms.load(Ordering::Acquire);
            while observed == 0 || target_ms < observed {
                match harness.state.drain_sleep_until_ms.compare_exchange_weak(
                    observed,
                    target_ms,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        scheduled = true;
                        break;
                    }
                    Err(actual) => observed = actual,
                }
            }
            if !scheduled {
                return;
            }
            let cancel = harness.state.drain_cancel.clone();
            let sleep_flag = Arc::clone(&harness.state);
            // Drop strong Harness reference before sleeping.
            drop(harness);
            tokio::select! {
                () = cancel.cancelled() => {
                    let _ = sleep_flag.drain_sleep_until_ms.compare_exchange(
                        target_ms,
                        0,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                    return;
                }
                () = tokio::time::sleep(
                    std::time::Duration::from_millis(delay_ms),
                ) => {
                    let _ = sleep_flag.drain_sleep_until_ms.compare_exchange(
                        target_ms,
                        0,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
            }
            if let Some(harness) = weak.upgrade() {
                harness.kick_autonomous_drain();
            }
        });
    }

    fn next_autonomous_wakeup_delay_ms(&self) -> Option<u64> {
        if !self.can_run_turns() {
            return None;
        }
        let live_busy = lock(&self.state.live)
            .values()
            .any(|session| session.status.is_busy());
        if self.has_active_claim() || live_busy {
            // A running turn will call `kick_autonomous_drain` upon completion.
            return None;
        }
        let engine_reaping = lock(&self.engine)
            .clone()
            .is_some_and(|engine| !engine.active_session_ids().is_empty());
        let now = now_ms();
        let receipts =
            AcceptanceQueue::new(self.state.root.clone()).all().ok()?;
        let next_ready = receipts
            .into_iter()
            .filter(|r| {
                !r.dead
                    && !r.cancelled
                    && !r.quarantined
                    && r.completed_at_ms.is_none()
                    && !r
                        .target_session_id
                        .as_deref()
                        .or(r.session_hint.as_deref())
                        .is_some_and(|sid| {
                            self.is_session_waiting_for_user(sid)
                        })
            })
            .map(|r| {
                let avail = r.available_at_ms.unwrap_or(0);
                let lease_exp = r.lease.as_ref().map_or(0, |l| l.expires_at_ms);
                avail.max(lease_exp)
            })
            .min()?;
        if engine_reaping || next_ready <= now {
            Some(30)
        } else {
            Some(next_ready.saturating_sub(now).clamp(20, 30_000))
        }
    }
}

fn settle_and_notify_failed_attempt(
    state: &UiState,
    queue: &AcceptanceQueue,
    receipt_id: &str,
    attempt_id: &str,
    attempts: u32,
) {
    if let Some(next_state) =
        settle_failed_attempt(queue, receipt_id, attempt_id, attempts, now_ms())
    {
        state.push_update(AgentUpdate::Receipt {
            id: receipt_id.to_owned(),
            state: next_state,
        });
    }
}

pub(super) fn release_backoff(attempts: u32) -> u64 {
    let shift = attempts.saturating_sub(1).min(6);
    ACCEPTED_RELEASE_BASE_MS
        .saturating_mul(1 << shift)
        .min(ACCEPTED_RELEASE_MAX_MS)
}

pub(super) fn settle_failed_attempt(
    queue: &AcceptanceQueue,
    receipt_id: &str,
    attempt_id: &str,
    attempts: u32,
    now: u64,
) -> Option<ReceiptState> {
    if attempts >= ACCEPTED_MAX_ATTEMPTS {
        queue
            .give_up(receipt_id, attempt_id, now)
            .ok()
            .filter(|&ok| ok)
            .map(|_| ReceiptState::Dead)
    } else {
        queue
            .release(receipt_id, attempt_id, now, release_backoff(attempts))
            .ok()
            .filter(|&ok| ok)
            .map(|_| ReceiptState::Retrying)
    }
}

pub(super) fn push_reconciled_receipt_updates(
    state: &UiState,
    events: Vec<(String, ReceiptState)>,
) {
    for (id, receipt_state) in events {
        if receipt_state == ReceiptState::Quarantined {
            let mut seen = lock(&state.notified_quarantined);
            if !seen.insert(id.clone()) {
                continue;
            }
        }
        state.push_update(AgentUpdate::Receipt {
            id,
            state: receipt_state,
        });
    }
}

/// Reconcile any previously bound receipt whose engine turn already committed
/// on disk before the process could append `Completed` to the journal, and
/// surface quarantined receipts detected during journal replay.
pub(super) fn reconcile_committed_acceptances(
    root: &Path,
    engine: &Engine,
    _store: Option<&ArtifactStore>,
) -> Vec<(String, ReceiptState)> {
    let mut reconciled = Vec::new();
    let queue = AcceptanceQueue::new(root.to_path_buf());
    let Ok(receipts) = queue.replay() else {
        return reconciled;
    };
    let now = now_ms();
    for receipt in receipts {
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
        {
            continue;
        }
        // Respect explicit `Released` retries (`lease.is_none() && !quarantined`),
        // while still reconciling unsettled claims (`lease.is_some()`) and
        // quarantined receipts (`receipt.quarantined`) whose turn already
        // durably committed on disk.
        if receipt.lease.is_none() && !receipt.quarantined {
            continue;
        }
        let committed_on_disk =
            (|| {
                let attempt_id = receipt.last_attempt_id.as_deref()?;
                let session_id = receipt.target_session_id.as_deref()?;
                let submission_id = receipt.submission_id.as_deref()?;
                let record = engine.session(session_id)?;
                submission_committed_in_session(&record, submission_id)
                    .then_some((attempt_id, session_id, submission_id))
            })();
        if let Some((attempt_id, session_id, submission_id)) = committed_on_disk
            && queue
                .complete_reconciled(&receipt.id, attempt_id, now)
                .unwrap_or(false)
        {
            reconciled.push((receipt.id.clone(), ReceiptState::Completed));
            tracing::info!(
                target: "flowy.acceptance",
                event = "reconciled_completed",
                receipt_id = %receipt.id,
                attempt_id,
                session_id,
                submission_id,
                "reconciled committed engine turn to prevent duplicate rerun"
            );
            continue;
        }
        if receipt.quarantined {
            reconciled.push((receipt.id, ReceiptState::Quarantined));
        }
    }
    reconciled
}

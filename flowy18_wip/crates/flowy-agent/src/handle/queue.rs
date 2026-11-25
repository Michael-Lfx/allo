//! Durable acceptance queue, lease heartbeat, autonomous draining, and startup reconciliation.

#[cfg(test)]
use super::AcceptedSubmitHook;
use super::{
    ACCEPTED_MAX_ATTEMPTS, ACCEPTED_RELEASE_BASE_MS, ACCEPTED_RELEASE_MAX_MS,
    AcceptanceQueue, AcceptedClaim, AcceptedMessage, AcceptedRun, AgentHandle,
    AgentUpdate, Arc, ArtifactStore, CancelToken, Engine, Ordering, Path,
    ReceiptState, TurnStatus, UiState, lock, now_ms,
    submission_committed_in_session,
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
        let hint = lock(&self.inner.state.active)
            .clone()
            .filter(|id| lock(&self.inner.state.live).contains_key(id));
        let receipt = AcceptanceQueue::new(self.inner.state.root.clone())
            .accept(hint.as_deref(), text, now_ms())
            .map_err(|_| "notice.storage_error")?;
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
            self.kick_autonomous_drain();
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
        let _drain = lock(&self.inner.state.accept_drain);
        if !self.can_run_turns()
            || self.has_active_claim()
            || self.has_busy_session()
        {
            return Ok(None);
        }
        if let Some(engine) = lock(&self.inner.engine).as_ref() {
            push_reconciled_receipt_updates(
                &self.inner.state,
                reconcile_committed_acceptances(
                    &self.inner.state.root,
                    engine,
                    self.inner.state.artifact_store.get(),
                ),
            );
        }
        let state = &self.inner.state;
        let previous_active = lock(&state.active).clone();
        let queue = AcceptanceQueue::new(state.root.clone());
        let Some(next) = queue
            .pending()
            .map_err(|_| "notice.storage_error")?
            .into_iter()
            .find(|receipt| match receipt.session_hint.as_deref() {
                // A receipt belongs to the session it was submitted from.
                // While that session is still running, wait for it instead
                // of taking the receipt and inflating its attempt count.
                Some(hint) => lock(&state.live)
                    .get(hint)
                    .is_none_or(|session| !session.status.is_busy()),
                None => true,
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
            .map_err(|_| "notice.storage_error")?
        else {
            return Ok(None);
        };
        let session = match claimed.session_hint.as_deref() {
            Some(hint) => {
                let hinted_state = lock(&state.live)
                    .get(hint)
                    .map(|session| session.status.is_busy());
                match hinted_state {
                    Some(true) => {
                        // Lost the race after selection: wait, do not fork.
                        let _ = queue.release(
                            &claimed.id,
                            &attempt_id,
                            now_ms(),
                            0,
                        );
                        return Ok(None);
                    }
                    Some(false) => hint.to_owned(),
                    None => match self.create_background_home_session(
                        previous_active.is_none(),
                    ) {
                        Ok(session) => session,
                        Err(error) => {
                            if let Some(next_state) = settle_failed_attempt(
                                &queue,
                                &claimed.id,
                                &attempt_id,
                                claimed.attempts,
                                now_ms(),
                            ) {
                                state.push_update(AgentUpdate::Receipt {
                                    id: claimed.id.clone(),
                                    state: next_state,
                                });
                            }
                            return Err(error);
                        }
                    },
                }
            }
            None => match self
                .create_background_home_session(previous_active.is_none())
            {
                Ok(session) => session,
                Err(error) => {
                    if let Some(next_state) = settle_failed_attempt(
                        &queue,
                        &claimed.id,
                        &attempt_id,
                        claimed.attempts,
                        now_ms(),
                    ) {
                        state.push_update(AgentUpdate::Receipt {
                            id: claimed.id.clone(),
                            state: next_state,
                        });
                    }
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
                if let Some(next_state) = settle_failed_attempt(
                    &queue,
                    &claimed.id,
                    &attempt_id,
                    claimed.attempts,
                    now_ms(),
                ) {
                    state.push_update(AgentUpdate::Receipt {
                        id: claimed.id.clone(),
                        state: next_state,
                    });
                }
                return Err("notice.storage_error");
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
        *lock(&state.accepted_claim) = Some(AcceptedClaim {
            receipt_id: claimed.id.clone(),
            attempt_id: attempt_id.clone(),
            session_id: session.clone(),
            submission_id: submission_id.clone(),
            attempts: claimed.attempts,
            heartbeat_cancel: heartbeat_cancel.clone(),
        });
        state.push_update(AgentUpdate::Receipt {
            id: claimed.id.clone(),
            state: ReceiptState::Running,
        });
        if let Err(error) = self.submit_message_targeted(
            Some(&session),
            &claimed.text,
            Some(submission),
            None,
            Some(heartbeat_cancel.clone()),
        ) {
            heartbeat_cancel.cancel();
            *lock(&state.accepted_claim) = None;
            if let Some(next_state) = settle_failed_attempt(
                &queue,
                &claimed.id,
                &attempt_id,
                claimed.attempts,
                now_ms(),
            ) {
                state.push_update(AgentUpdate::Receipt {
                    id: claimed.id.clone(),
                    state: next_state,
                });
            }
            return Err(error);
        }
        let hb_queue = AcceptanceQueue::new(state.root.clone());
        let hb_receipt = claimed.id.clone();
        let hb_attempt = attempt_id.clone();
        let interval_ms = (lease_ms / 3).max(20);
        self.inner.runtime.spawn(async move {
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
        let claim = lock(&self.inner.state.accepted_claim).clone();
        let Some(claim) = claim else {
            return Ok(false);
        };
        let queue = AcceptanceQueue::new(self.inner.state.root.clone());
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
        // Only drop the claim once the journal accepted the outcome; a failed
        // write keeps it so a retry can finish the same attempt.
        if result.is_ok() {
            claim.heartbeat_cancel.cancel();
            let root_session_id = self.root_session_id_for(&claim.session_id);
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
            self.inner.state.push_update(AgentUpdate::Receipt {
                id: claim.receipt_id,
                state: next_state,
            });
            *lock(&self.inner.state.accepted_claim) = None;
        }
        result.map(|_| gave_up).map_err(|_| "notice.storage_error")
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
            self.kick_autonomous_drain();
        }
    }

    #[must_use]
    pub fn autonomous_drain(&self) -> bool {
        self.inner.state.autonomous_drain.load(Ordering::Acquire)
    }

    pub(super) fn kick_autonomous_drain(&self) {
        if !self.inner.state.autonomous_drain.load(Ordering::Acquire)
            || self.is_initializing()
        {
            return;
        }
        if self
            .inner
            .state
            .drain_wakeup_scheduled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        self.inner.runtime.spawn(async move {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let handle = Self { inner };
            handle
                .inner
                .state
                .drain_wakeup_scheduled
                .store(false, Ordering::Release);
            if !handle.inner.state.autonomous_drain.load(Ordering::Acquire) {
                return;
            }
            match handle.run_accepted_once() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if handle.has_active_claim() || handle.has_busy_session() {
                        return;
                    }
                    let now = now_ms();
                    let next_wake_ms =
                        AcceptanceQueue::new(handle.inner.state.root.clone())
                            .all()
                            .ok()
                            .and_then(|receipts| {
                                receipts
                                    .into_iter()
                                    .filter(|r| {
                                        !r.dead
                                            && !r.cancelled
                                            && !r.quarantined
                                            && r.completed_at_ms.is_none()
                                    })
                                    .filter_map(|r| {
                                        let avail =
                                            r.available_at_ms.unwrap_or(0);
                                        let lease_exp = r
                                            .lease
                                            .as_ref()
                                            .map_or(0, |l| l.expires_at_ms);
                                        let ready_at = avail.max(lease_exp);
                                        (ready_at > now).then_some(ready_at)
                                    })
                                    .min()
                            });
                    if let Some(ready_at) = next_wake_ms {
                        let delay_ms =
                            ready_at.saturating_sub(now).clamp(20, 30_000);
                        let cancel = handle.inner.state.drain_cancel.clone();
                        // Drop strong Harness reference before sleeping so
                        // AgentHandle::drop never blocks or leaks.
                        drop(handle);
                        tokio::select! {
                            () = cancel.cancelled() => return,
                            () = tokio::time::sleep(
                                std::time::Duration::from_millis(delay_ms),
                            ) => {}
                        }
                        if let Some(inner) = weak.upgrade() {
                            Self { inner }.kick_autonomous_drain();
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        target: "flowy.acceptance",
                        error,
                        "autonomous drain could not start next receipt"
                    );
                }
            }
        });
    }

    #[must_use]
    pub fn has_active_claim(&self) -> bool {
        lock(&self.inner.state.accepted_claim).is_some()
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
    store: Option<&ArtifactStore>,
) -> Vec<(String, ReceiptState)> {
    let mut reconciled = Vec::new();
    let queue = AcceptanceQueue::new(root.to_path_buf());
    let Ok(receipts) = queue.replay() else {
        return reconciled;
    };
    let committed_artifacts = store
        .and_then(|store| store.committed_ids().ok())
        .unwrap_or_default();
    let now = now_ms();
    for receipt in receipts {
        if receipt.quarantined
            && receipt.completed_at_ms.is_none()
            && !receipt.dead
            && !receipt.cancelled
        {
            reconciled.push((receipt.id, ReceiptState::Quarantined));
            continue;
        }
        // Only reconcile unsettled claims (`Taken` + `Bound` with no
        // subsequent `Completed` / `Released` / `Dead` / `Cancelled`): an
        // explicit `Released` clears `lease` to request a retry.
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
            || receipt.lease.is_none()
        {
            continue;
        }
        let (Some(attempt_id), Some(session_id), Some(submission_id)) = (
            receipt.last_attempt_id.as_deref(),
            receipt.target_session_id.as_deref(),
            receipt.submission_id.as_deref(),
        ) else {
            continue;
        };
        let Some(record) = engine.session(session_id) else {
            continue;
        };
        if submission_committed_in_session(
            &record,
            submission_id,
            &committed_artifacts,
        ) && queue
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
        }
    }
    reconciled
}

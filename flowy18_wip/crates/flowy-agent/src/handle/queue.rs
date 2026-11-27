//! Durable acceptance queue, lease heartbeat, autonomous draining, and startup reconciliation.

#[cfg(test)]
use super::AcceptedSubmitHook;
use super::{
    ACCEPTED_MAX_ATTEMPTS, ACCEPTED_RELEASE_BASE_MS, ACCEPTED_RELEASE_MAX_MS,
    AcceptanceQueue, AcceptedClaim, AcceptedMessage, AcceptedRun, AgentHandle,
    AgentUpdate, AgentWorkspace, Arc, ArtifactStore, CancelToken, Engine,
    Harness, HashMap, Live, Ordering, Path, RECEIPT_SUPERSEDED_ERR,
    ReceiptState, TurnStatus, UiState, lock, now_ms,
    submission_committed_in_session,
};
use flowy_agent_core::{
    DraftTarget, SessionOrigin, SessionRole, SessionWorkspace,
};

const CLOCK_JUMP_THRESHOLD_MS: i128 = 5_000;

static MONOTONIC_ORIGIN: std::sync::OnceLock<std::time::Instant> =
    std::sync::OnceLock::new();
static CLOCK_PROBE: std::sync::OnceLock<std::sync::Mutex<(u64, u64)>> =
    std::sync::OnceLock::new();

fn monotonic_now_ms() -> u64 {
    let origin = MONOTONIC_ORIGIN.get_or_init(std::time::Instant::now);
    let elapsed_ms =
        u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX);
    elapsed_ms.max(1)
}

const fn detect_clock_jump_ms(
    prev_mono_ms: u64,
    now_mono_ms: u64,
    prev_wall_ms: u64,
    now_wall_ms: u64,
) -> Option<i64> {
    let mono_delta = (now_mono_ms as i128) - (prev_mono_ms as i128);
    let wall_delta = (now_wall_ms as i128) - (prev_wall_ms as i128);
    let skew = wall_delta - mono_delta;
    if skew.abs() >= CLOCK_JUMP_THRESHOLD_MS {
        let clamped = if skew > i64::MAX as i128 {
            i64::MAX
        } else if skew < i64::MIN as i128 {
            i64::MIN
        } else {
            skew as i64
        };
        Some(clamped)
    } else {
        None
    }
}

fn check_clock_jump_with_probe(
    probe: &mut (u64, u64),
    now_mono: u64,
    now_wall: u64,
    drain_sleep_until_ms: &std::sync::atomic::AtomicU64,
) -> Option<i64> {
    let (prev_mono, prev_wall) = *probe;
    *probe = (now_mono, now_wall);
    let jump_ms =
        detect_clock_jump_ms(prev_mono, now_mono, prev_wall, now_wall)?;
    drain_sleep_until_ms.store(0, Ordering::Release);
    tracing::warn!(
        target: "flowy.acceptance",
        event = "clock_jump_detected",
        jump_ms,
        prev_mono_ms = prev_mono,
        now_mono_ms = now_mono,
        prev_wall_ms = prev_wall,
        now_wall_ms = now_wall,
        "system clock jump detected; reset autonomous drain deadline"
    );
    Some(jump_ms)
}

fn probe_clock_jump(
    drain_sleep_until_ms: &std::sync::atomic::AtomicU64,
) -> Option<i64> {
    let now_mono = monotonic_now_ms();
    let now_wall = now_ms();
    let probe =
        CLOCK_PROBE.get_or_init(|| std::sync::Mutex::new((now_mono, now_wall)));
    let mut guard = lock(probe);
    let jump = check_clock_jump_with_probe(
        &mut guard,
        now_mono,
        now_wall,
        drain_sleep_until_ms,
    );
    drop(guard);
    jump
}

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
        let active = lock(&self.inner.state.active).clone();
        let hint =
            active.filter(|id| lock(&self.inner.state.live).contains_key(id));
        self.accept_deferred_inner(hint.as_deref(), text, false, None)
    }

    /// Durably accept a cold-start empty-draft prompt while preserving the
    /// draft's Project workspace and target role (`SessionRole::Router`), so a
    /// subsequent project switch or app restart before cloud readiness cannot
    /// misroute the prompt into `Home / Standard`.
    pub fn accept_deferred_draft(
        &self,
        text: &str,
        origin: SessionOrigin,
        workspace: Option<AgentWorkspace>,
        role: SessionRole,
    ) -> Result<AcceptedMessage, &'static str> {
        let target = DraftTarget {
            origin,
            workspace: workspace.map(|ws| SessionWorkspace { root: ws.root }),
            role,
        };
        self.accept_deferred_inner(None, text, false, Some(target))
    }

    /// Durably accept a cold-start prompt strictly targeted at an existing
    /// Router conversation (cancels cleanly instead of spawning a Home
    /// fallback if the Router is deleted before readiness).
    pub fn accept_deferred_router(
        &self,
        router_id: &str,
        text: &str,
    ) -> Result<AcceptedMessage, &'static str> {
        self.accept_deferred_inner(Some(router_id), text, true, None)
    }

    fn accept_deferred_inner(
        &self,
        session_hint: Option<&str>,
        text: &str,
        strict_target: bool,
        target: Option<DraftTarget>,
    ) -> Result<AcceptedMessage, &'static str> {
        let text = text.trim();
        if text.is_empty() {
            return Err("notice.prompt_empty");
        }
        if self.is_initializing() {
            return Err("library.loading");
        }
        let receipt = AcceptanceQueue::new(self.inner.state.root.clone())
            .accept_with_target(
                session_hint,
                text,
                strict_target,
                target,
                None,
                now_ms(),
            )
            .map_err(|_| {
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        let root_session_id = session_hint.map(|id| self.root_session_id(id));
        tracing::info!(
            target: "flowy.acceptance",
            event = "accepted",
            receipt_id = %receipt.id,
            session_id = session_hint.unwrap_or(""),
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

    /// Number of durable acceptance receipts currently claimed and executing.
    #[must_use]
    pub fn active_claim_count(&self) -> usize {
        self.inner.active_claim_count()
    }

    /// Sorted session IDs currently owned by active acceptance claims.
    #[must_use]
    pub fn active_claimed_session_ids(&self) -> Vec<String> {
        self.inner.active_claimed_session_ids()
    }

    /// Configured concurrency ceiling for simultaneous acceptance runs.
    #[must_use]
    pub fn max_concurrent_works(&self) -> usize {
        self.inner.max_concurrent_works()
    }

    #[cfg(test)]
    pub(crate) fn set_max_concurrent_works_for_test(&self, limit: usize) {
        self.inner
            .state
            .max_concurrent_works
            .store(limit.max(1), Ordering::Release);
    }

    /// Status of the primary session that owns an active receipt claim,
    /// independent of whichever session the user is currently viewing.
    #[must_use]
    pub fn claimed_session_status(&self) -> Option<TurnStatus> {
        let live = lock(&self.inner.state.live);
        let claim = {
            let claims = lock(&self.inner.state.active_claims);
            select_primary_claim_locked(&live, &claims)
        }?;
        live.get(&claim.session_id)
            .map(|session| session.status.clone())
    }

    /// The attempt number of the primary receipt currently claimed, if any.
    #[must_use]
    pub fn accepted_attempts(&self) -> Option<u32> {
        self.inner.primary_claim().map(|claim| claim.attempts)
    }
}

impl Harness {
    pub(super) fn has_active_claim(&self) -> bool {
        !lock(&self.state.active_claims).is_empty()
    }

    pub(super) fn active_claim_count(&self) -> usize {
        lock(&self.state.active_claims).len()
    }

    pub(super) fn active_claimed_session_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = lock(&self.state.active_claims)
            .values()
            .map(|claim| claim.session_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub(super) fn max_concurrent_works(&self) -> usize {
        self.state
            .max_concurrent_works
            .load(Ordering::Acquire)
            .max(1)
    }

    fn primary_claim(&self) -> Option<AcceptedClaim> {
        // Lock order: `live` -> leaf lock `active_claims`.
        let live = lock(&self.state.live);
        let claims = lock(&self.state.active_claims);
        select_primary_claim_locked(&live, &claims)
    }

    fn is_receipt_workspace_locked(&self, receipt: &AcceptedMessage) -> bool {
        let Some(sid) = receipt
            .target_session_id
            .as_deref()
            .or(receipt.session_hint.as_deref())
        else {
            // Unbound receipts create a new session without acquiring a worker write lock.
            return false;
        };
        // Routers never acquire directory write locks; spawned Workers inherit them.
        if self.session_role(sid) == SessionRole::Router {
            return false;
        }
        let Some(mode) = self.session_access_mode(sid) else {
            return false;
        };
        let Some(cwd) = self.session_workspace_path(sid) else {
            return false;
        };
        self.state.workspace_locks.is_locked_for(&cwd, mode)
    }

    fn strict_target_missing(&self, receipt: &AcceptedMessage) -> bool {
        if !receipt.strict_target {
            return false;
        }
        let Some(sid) = receipt
            .target_session_id
            .as_deref()
            .or(receipt.session_hint.as_deref())
        else {
            return true;
        };
        if !lock(&self.state.live).contains_key(sid) {
            return true;
        }
        let engine = lock(&self.engine).clone();
        engine.is_some_and(|eng| eng.session(sid).is_none())
    }

    pub(super) fn run_accepted_once(
        self: &Arc<Self>,
    ) -> Result<Option<AcceptedRun>, &'static str> {
        let _drain = lock(&self.state.accept_drain);
        probe_clock_jump(&self.state.drain_sleep_until_ms);
        if self.state.shutdown_requested.load(Ordering::Acquire)
            || !self.can_run_turns()
            || self.in_flight_work_count() >= self.max_concurrent_works()
        {
            return Ok(None);
        }
        let engine = lock(&self.engine).clone();
        if let Some(engine) = engine {
            // Skip claims actively finishing in this process so we never write
            // `Completed` to the journal before `finish_accepted_if_owned` clears
            // `active_claims` and cancels heartbeats.
            let live_attempts: Vec<(String, String)> =
                lock(&self.state.active_claims)
                    .iter()
                    .map(|(id, claim)| (id.clone(), claim.attempt_id.clone()))
                    .collect();
            push_reconciled_receipt_updates(
                &self.state,
                reconcile_committed_acceptances(
                    &self.state.root,
                    &engine,
                    self.state.artifact_store.get(),
                    &live_attempts,
                ),
            );
        }
        let state = &self.state;
        let active_claim_ids: std::collections::HashSet<String> =
            lock(&state.active_claims).keys().cloned().collect();
        let queue = AcceptanceQueue::new(state.root.clone());
        let pending = queue
            .pending()
            .map_err(|_| "notice.session_storage_failed")?;
        let mut selected: Option<AcceptedMessage> = None;
        for receipt in pending {
            if active_claim_ids.contains(&receipt.id) {
                continue;
            }
            if self.strict_target_missing(&receipt) {
                if queue.cancel(&receipt.id, now_ms()).unwrap_or(false) {
                    state.push_update(AgentUpdate::Receipt {
                        id: receipt.id,
                        state: ReceiptState::Cancelled,
                    });
                }
                continue;
            }
            if selected.is_some() {
                continue;
            }
            let target = receipt
                .target_session_id
                .as_deref()
                .or(receipt.session_hint.as_deref());
            let ready = match target {
                // A receipt belongs to the session it was bound/submitted
                // from. While that session is running, reaping after cancel,
                // parked in WaitingForUser, or its workspace directory lock
                // conflicts with its access mode, skip it cleanly without
                // inflating its attempt count or blocking free peers.
                Some(sid) => {
                    !self.is_session_busy_or_active(sid)
                        && !self.is_session_waiting_for_user(sid)
                        && !self.is_receipt_workspace_locked(&receipt)
                }
                None => true,
            };
            if ready {
                selected = Some(receipt);
            }
        }
        let Some(next) = selected else {
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
        if self.strict_target_missing(&claimed) {
            if queue.cancel(&claimed.id, now_ms()).unwrap_or(false) {
                state.push_update(AgentUpdate::Receipt {
                    id: claimed.id,
                    state: ReceiptState::Cancelled,
                });
            }
            return Ok(None);
        }
        let session = match claimed
            .target_session_id
            .as_deref()
            .or(claimed.session_hint.as_deref())
        {
            Some(target) if lock(&state.live).contains_key(target) => {
                if self.is_session_busy_or_active(target)
                    || self.is_session_waiting_for_user(target)
                    || self.is_receipt_workspace_locked(&claimed)
                {
                    // Lost the race after selection: wait with 0 ms backoff,
                    // do not fork or write an unnecessary bind.
                    if queue
                        .release(&claimed.id, &attempt_id, now_ms(), 0)
                        .unwrap_or(false)
                    {
                        state.push_update(AgentUpdate::Receipt {
                            id: claimed.id,
                            state: ReceiptState::Queued,
                        });
                    }
                    return Ok(None);
                }
                target.to_owned()
            }
            _ => {
                if claimed.strict_target {
                    if queue.cancel(&claimed.id, now_ms()).unwrap_or(false) {
                        state.push_update(AgentUpdate::Receipt {
                            id: claimed.id,
                            state: ReceiptState::Cancelled,
                        });
                    }
                    return Ok(None);
                }
                let (target_origin, target_workspace, target_role) =
                    match claimed.draft_target.as_deref() {
                        Some(draft) => {
                            let ws = draft.workspace.as_ref().map(|ws| {
                                AgentWorkspace {
                                    project_id: match &draft.origin {
                                        SessionOrigin::Project {
                                            project_id: Some(pid),
                                        } => pid.clone(),
                                        _ => String::new(),
                                    },
                                    root: ws.root.clone(),
                                }
                            });
                            (draft.origin.clone(), ws, draft.role)
                        }
                        None => {
                            (SessionOrigin::Home, None, SessionRole::Standard)
                        }
                    };
                match self.create_background_session_with_role(
                    target_origin,
                    target_workspace,
                    target_role,
                ) {
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
                }
            }
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
        // `dispatch`, acquires the directory workspace lease, starts the
        // engine, transitions `live[session].status` to `Running`, and only
        // then publishes `active_claims` + `AgentUpdate::Receipt(Running)` —
        // closing all TOCTOU windows.
        if let Err(error) = self.submit_message_targeted(
            Some(&session),
            &claimed.text,
            Some(submission),
            None,
            Some(pending_claim),
            None,
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
                // The first tick fires at once, so the lease is renewed right
                // away: it started at `take`, before the session setup and
                // submit above, which can use up most of a short lease on a
                // loaded machine.
                let mut ticker = tokio::time::interval(
                    std::time::Duration::from_millis(interval_ms),
                );
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

    pub(super) fn has_owned_claim(
        &self,
        session_id: &str,
        submission_id: Option<&str>,
    ) -> bool {
        submission_id.is_some_and(|sub| {
            lock(&self.state.active_claims).values().any(|claim| {
                claim.session_id == session_id && claim.submission_id == sub
            })
        })
    }

    pub(super) fn finish_accepted_if_owned(
        &self,
        session_id: &str,
        submission_id: Option<&str>,
        completed: bool,
    ) -> Result<bool, &'static str> {
        if !self.has_owned_claim(session_id, submission_id) {
            return Ok(false);
        }
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
        let claim = if let Some(session_id) = expected_session {
            let Some(submission_id) = expected_submission else {
                return Ok(false);
            };
            lock(&self.state.active_claims)
                .values()
                .find(|claim| {
                    claim.session_id == session_id
                        && claim.submission_id == submission_id
                })
                .cloned()
        } else {
            self.primary_claim()
        };
        let Some(claim) = claim else {
            return Ok(false);
        };
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
        let removed_claim = {
            let mut claims = lock(&self.state.active_claims);
            if claims
                .get(&claim.receipt_id)
                .is_some_and(|current| current.attempt_id == claim.attempt_id)
            {
                claims.remove(&claim.receipt_id)
            } else {
                None
            }
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
        if let Ok(applied) = result {
            claim.heartbeat_cancel.cancel();
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
        } else {
            if let Some(restored) = removed_claim {
                lock(&self.state.active_claims)
                    .insert(restored.receipt_id.clone(), restored);
            }
            Err("notice.session_storage_failed")
        }
    }

    pub(super) fn kick_autonomous_drain(self: &Arc<Self>) {
        if !self.state.autonomous_drain.load(Ordering::Acquire)
            || self.is_initializing()
        {
            return;
        }
        probe_clock_jump(&self.state.drain_sleep_until_ms);
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
            probe_clock_jump(&harness.state.drain_sleep_until_ms);
            let max_slots = harness.max_concurrent_works();
            // Fill all currently available concurrency slots in a single pass
            // while bounding iterations in case multiple receipts immediately
            // settle (e.g. disambiguation cards) without occupying a slot.
            let max_passes = max_slots.saturating_add(16);
            for _ in 0..max_passes {
                if harness.in_flight_work_count() >= max_slots {
                    break;
                }
                let h = Arc::clone(&harness);
                match tokio::task::spawn_blocking(move || h.run_accepted_once())
                    .await
                {
                    Ok(Ok(Some(_))) => {}
                    Ok(Ok(None)) | Err(_) => break,
                    Ok(Err(error)) => {
                        tracing::warn!(
                            target: "flowy.acceptance",
                            error,
                            "autonomous drain could not start next receipt"
                        );
                        break;
                    }
                }
            }
            if harness.in_flight_work_count() >= max_slots {
                return;
            }
            let delay_ms = harness.next_autonomous_wakeup_delay_ms();
            let Some(delay_ms) = delay_ms else {
                return;
            };
            probe_clock_jump(&harness.state.drain_sleep_until_ms);
            let target_ms = monotonic_now_ms().saturating_add(delay_ms).max(1);
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
                    probe_clock_jump(&sleep_flag.drain_sleep_until_ms);
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
        if self.in_flight_work_count() >= self.max_concurrent_works() {
            // Every concurrency slot is occupied; a finishing turn will call
            // `kick_autonomous_drain` upon completion.
            return None;
        }
        let engine = lock(&self.engine).clone();
        let active_claim_ids: std::collections::HashSet<String> =
            lock(&self.state.active_claims).keys().cloned().collect();
        let has_workspace_locks = !self.state.workspace_locks.is_empty();
        let now = now_ms();
        let receipts =
            AcceptanceQueue::new(self.state.root.clone()).all().ok()?;
        let mut any_reaping_target = false;
        let next_ready = receipts
            .into_iter()
            .filter(|r| {
                if r.dead
                    || r.cancelled
                    || r.quarantined
                    || r.completed_at_ms.is_some()
                    || active_claim_ids.contains(&r.id)
                {
                    return false;
                }
                let target = r
                    .target_session_id
                    .as_deref()
                    .or(r.session_hint.as_deref());
                if let Some(sid) = target {
                    if self.is_session_waiting_for_user(sid) {
                        return false;
                    }
                    let live_busy = lock(&self.state.live)
                        .get(sid)
                        .is_some_and(|session| session.status.is_busy());
                    if live_busy {
                        // That session's own turn will kick drain upon exit.
                        return false;
                    }
                    if engine.as_ref().is_some_and(|eng| eng.is_active(sid)) {
                        any_reaping_target = true;
                        return true;
                    }
                }
                if has_workspace_locks && self.is_receipt_workspace_locked(r) {
                    // Directory write lock is held by a running turn; its drop
                    // calls `kick_autonomous_drain` immediately.
                    return false;
                }
                true
            })
            .map(|r| {
                let avail = r.available_at_ms.unwrap_or(0);
                let lease_exp = r.lease.as_ref().map_or(0, |l| l.expires_at_ms);
                avail.max(lease_exp)
            })
            .min()?;
        if any_reaping_target || next_ready <= now {
            Some(30)
        } else {
            Some(next_ready.saturating_sub(now).clamp(20, 30_000))
        }
    }
}

fn select_primary_claim_locked(
    live: &HashMap<String, Live>,
    claims: &HashMap<String, AcceptedClaim>,
) -> Option<AcceptedClaim> {
    if claims.is_empty() {
        return None;
    }
    claims
        .values()
        .filter(|claim| {
            live.get(&claim.session_id)
                .is_none_or(|session| !session.status.is_busy())
        })
        .min_by(|a, b| a.receipt_id.cmp(&b.receipt_id))
        .or_else(|| {
            claims
                .values()
                .min_by(|a, b| a.receipt_id.cmp(&b.receipt_id))
        })
        .cloned()
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
    live_attempts: &[(String, String)],
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
                if live_attempts
                    .iter()
                    .any(|(id, att)| id == &receipt.id && att == attempt_id)
                {
                    return None;
                }
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use super::*;
    use crate::config::LlmConfig;

    #[test]
    fn monotonic_now_ms_is_nonzero_and_nondecreasing() {
        let first = monotonic_now_ms();
        assert!(first >= 1, "monotonic_now_ms must reserve 0 as sentinel");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = monotonic_now_ms();
        assert!(second >= first);
    }

    #[test]
    fn detect_clock_jump_ms_handles_forward_backward_and_below_threshold() {
        // Below 5_000 ms threshold -> None
        assert_eq!(detect_clock_jump_ms(1_000, 2_000, 10_000, 15_999), None);
        assert_eq!(detect_clock_jump_ms(1_000, 2_000, 10_000, 6_001), None);

        // Exact thresholds (+/- 5_000 ms)
        assert_eq!(
            detect_clock_jump_ms(1_000, 2_000, 10_000, 16_000),
            Some(5_000)
        );
        assert_eq!(
            detect_clock_jump_ms(1_000, 2_000, 10_000, 6_000),
            Some(-5_000)
        );

        // Large forward jump (e.g. OS sleep/wake) and backward NTP step
        assert_eq!(
            detect_clock_jump_ms(10_000, 10_500, 100_000, 160_500),
            Some(60_000)
        );
        assert_eq!(
            detect_clock_jump_ms(10_000, 11_000, 100_000, 70_000),
            Some(-31_000)
        );

        // Probe resets drain_sleep_until_ms only when threshold is crossed.
        let deadline = AtomicU64::new(987_654);
        let mut probe = (1_000, 10_000);
        assert_eq!(
            check_clock_jump_with_probe(&mut probe, 2_000, 11_100, &deadline),
            None
        );
        assert_eq!(deadline.load(Ordering::Acquire), 987_654);

        assert_eq!(
            check_clock_jump_with_probe(&mut probe, 2_100, 20_100, &deadline),
            Some(8_900)
        );
        assert_eq!(deadline.load(Ordering::Acquire), 0);

        deadline.store(123_456, Ordering::Release);
        assert_eq!(
            check_clock_jump_with_probe(&mut probe, 3_100, 10_100, &deadline),
            Some(-11_000)
        );
        assert_eq!(deadline.load(Ordering::Acquire), 0);
    }

    #[test]
    fn run_accepted_once_skips_in_process_active_claim_even_when_lease_expired()
    {
        let root = std::env::temp_dir().join(format!(
            "flowy-queue-active-claim-skip-{}",
            super::super::next_stamp()
        ));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let session_id = handle.create_session().expect("session");

        // Create a receipt whose journal lease already expired in wall-clock
        // time (e.g. OS slept past `expires_at_ms`), while remaining actively
        // claimed in `self.state.active_claims`.
        let queue = AcceptanceQueue::new(root.clone());
        let r1 = queue
            .accept(Some(&session_id), "claimed before sleep", 1_000)
            .expect("accept r1");
        let claimed = queue
            .take(&r1.id, "attempt-live", 1_000, 10)
            .expect("take r1")
            .expect("claimed r1");
        assert_eq!(claimed.attempts, 1);
        assert_eq!(queue.pending().expect("pending").len(), 1);

        // Keep `live[&session_id].status == TurnStatus::Idle` intentionally so
        // only the `active_claims` guard in `run_accepted_once` prevents
        // double-claiming.
        lock(&handle.inner.state.active_claims).insert(
            r1.id.clone(),
            AcceptedClaim {
                receipt_id: r1.id.clone(),
                attempt_id: "attempt-live".into(),
                session_id,
                submission_id: "submission-live".into(),
                attempts: 1,
                heartbeat_cancel: CancelToken::new(),
            },
        );

        assert!(
            handle.run_accepted_once().expect("pump 1").is_none(),
            "in-process active claim must be skipped even when wall-clock lease expired"
        );
        let r1_after = queue.get(&r1.id).expect("get").expect("present");
        assert_eq!(r1_after.attempts, 1, "skip must not bump attempts");
        assert_eq!(r1_after.last_attempt_id.as_deref(), Some("attempt-live"));

        // A peer queued receipt not in `active_claims` must still be claimed.
        let _s2 = handle.create_session().expect("session 2");
        let r2 = handle
            .accept_deferred("free peer prompt")
            .expect("accept r2");
        let run = handle
            .run_accepted_once()
            .expect("pump 2")
            .expect("peer claimed");
        assert_eq!(run.receipt_id, r2.id);

        let r1_final = queue.get(&r1.id).expect("get").expect("present");
        assert_eq!(r1_final.attempts, 1);
        assert_eq!(r1_final.last_attempt_id.as_deref(), Some("attempt-live"));

        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }
}

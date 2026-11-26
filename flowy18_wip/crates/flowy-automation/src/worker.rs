//! A caller-driven durable queue. Wakeups are hints; SQLite owns every run.
use crate::store::{AutomationStore, RevisionRecord, RunRecord, StoreError};
use crate::{
    AccessOperation, ActionSpec, AdmissionDecision, AdmissionPoint,
    AdmissionPolicy, AdmissionRequest, ArtifactState, BlockedReason,
    CallerContext, CancellationSignal, Clock, ExecutionOutcome,
    HistoryCommitState, HostAccess, HostAccessDecision, HostAccessRequest,
    PortError, PortFuture, PrincipalId, ProgressPhase, ResultRef,
    RunCompletion, RunId, RunPhase, RunProgress, RunProgressSink, RunSpec,
    StopReason,
};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};

/// Preparing may wait for workspace and execution permits, but must not submit
/// model work or create a durable Attempt. The returned object owns all permits.
pub trait RunPreparer: Send + Sync {
    fn blocked_reason(
        &self,
        _run: &RunRecord,
        _revision: &RevisionRecord,
    ) -> Result<Option<BlockedReason>, PortError> {
        Ok(None)
    }

    fn prepare<'a>(
        &'a self,
        run: &'a RunRecord,
        revision: &'a RevisionRecord,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> PortFuture<'a, Result<Box<dyn PreparedRun>, PortError>>;
}

/// Consumed only after the exact session/submission mapping commits. Dropping
/// an unused prepared run releases its permits without running the agent.
pub trait PreparedRun: Send {
    fn spec(&self) -> &RunSpec;
    fn execute(
        self: Box<Self>,
        progress: Arc<dyn RunProgressSink>,
    ) -> PortFuture<'static, RunCompletion>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerCycle {
    Idle,
    Deferred,
    Expired { run_id: RunId },
    Settled { run_id: RunId },
    Finalizing { run_id: RunId },
}

/// Uses the host runtime; no thread, runtime or periodic loop is created here.
#[derive(Clone)]
pub struct AutomationWorker {
    store: AutomationStore,
    preparer: Arc<dyn RunPreparer>,
    access: Arc<dyn HostAccess>,
    admission: Arc<dyn AdmissionPolicy>,
    clock: Arc<dyn Clock>,
    gate: Arc<Mutex<()>>,
    stopping: watch::Sender<Option<StopReason>>,
}

impl std::fmt::Debug for AutomationWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationWorker").finish_non_exhaustive()
    }
}

impl AutomationWorker {
    pub fn new(
        store: AutomationStore,
        preparer: Arc<dyn RunPreparer>,
        access: Arc<dyn HostAccess>,
        admission: Arc<dyn AdmissionPolicy>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            store,
            preparer,
            access,
            admission,
            clock,
            gate: Arc::new(Mutex::new(())),
            stopping: watch::channel(None).0,
        }
    }

    /// Stops admission and requests cancellation. The host must still await its
    /// active `run_once` future before releasing the shared execution resources.
    pub fn request_stop(&self, reason: StopReason) {
        self.stopping.send_if_modified(|current| {
            if current.is_some() {
                false
            } else {
                *current = Some(reason);
                true
            }
        });
    }

    pub async fn run_once(
        &self,
        caller: &CallerContext,
    ) -> Result<WorkerCycle, PortError> {
        self.run_once_with_wakeup(caller, true).await
    }

    pub async fn run_once_with_wakeup(
        &self,
        caller: &CallerContext,
        recheck_now: bool,
    ) -> Result<WorkerCycle, PortError> {
        let Ok(_guard) = self.gate.try_lock() else {
            return Ok(WorkerCycle::Idle);
        };
        if self.stopping.borrow().is_some() {
            return Ok(WorkerCycle::Idle);
        }
        let candidates = self
            .store
            .worker_candidates(caller.principal.clone())
            .await
            .map_err(storage)?;
        let mut deferred = false;
        for mut run in candidates {
            if self.stopping.borrow().is_some() {
                break;
            }
            let now = self.clock.now().utc_ms;
            if now > run.latest_start_at_ms {
                if self
                    .store
                    .worker_expire(
                        caller.principal.clone(),
                        run.run_id.clone(),
                        run.phase,
                        now,
                    )
                    .await
                    .map_err(storage)?
                {
                    return Ok(WorkerCycle::Expired { run_id: run.run_id });
                }
                continue;
            }
            if run.phase == RunPhase::Blocked
                && !recheck_now
                && !self
                    .store
                    .worker_blocked_due(
                        run.principal.clone(),
                        run.run_id.clone(),
                        now,
                    )
                    .await
                    .map_err(storage)?
            {
                continue;
            }
            let revision = self
                .store
                .revisions(caller.principal.clone(), run.automation_id.clone())
                .await
                .map_err(storage)?
                .into_iter()
                .find(|revision| revision.revision == run.revision)
                .ok_or(PortError::Storage)?;
            if let Some(reason) = self.blocked(caller, &run, &revision).await? {
                self.defer(&run, reason).await?;
                deferred = true;
                continue;
            }
            if run.phase == RunPhase::Blocked {
                if !self
                    .store
                    .worker_requeue(
                        run.principal.clone(),
                        run.run_id.clone(),
                        self.clock.now().utc_ms,
                    )
                    .await
                    .map_err(storage)?
                {
                    continue;
                }
                run.phase = RunPhase::Queued;
            }
            if matches!(revision.definition.action, ActionSpec::Reminder { .. })
            {
                // No prepare, workspace permit, model, session or Attempt.
                let completion = RunCompletion {
                    execution_outcome: ExecutionOutcome::Completed,
                    history_commit: HistoryCommitState::NotStarted,
                    artifact_state: ArtifactState::NotRequired,
                    commit_ref: None,
                    result_ref: Some(ResultRef::Reminder {
                        run_id: run.run_id.clone(),
                    }),
                    error_code: None,
                    stop_reason: None,
                };
                let now = self.clock.now().utc_ms;
                if now > run.latest_start_at_ms {
                    if self
                        .store
                        .worker_expire(
                            caller.principal.clone(),
                            run.run_id.clone(),
                            run.phase,
                            now,
                        )
                        .await
                        .map_err(storage)?
                    {
                        return Ok(WorkerCycle::Expired { run_id: run.run_id });
                    }
                    continue;
                }
                if let Some(reason) =
                    self.blocked(caller, &run, &revision).await?
                {
                    self.defer(&run, reason).await?;
                    deferred = true;
                    continue;
                }
                if self
                    .store
                    .worker_complete(
                        caller.principal.clone(),
                        run.run_id.clone(),
                        None,
                        completion,
                        self.clock.now().utc_ms,
                    )
                    .await
                    .map_err(storage)?
                {
                    return Ok(WorkerCycle::Settled { run_id: run.run_id });
                }
                continue;
            }
            if !self
                .store
                .worker_waiting(
                    caller.principal.clone(),
                    run.run_id.clone(),
                    run.phase,
                    now,
                )
                .await
                .map_err(storage)?
            {
                continue;
            }
            let cancellation = Arc::new(WorkerCancellation::new());
            let prepared = self
                .observe(
                    &run,
                    &cancellation,
                    Some((caller, &revision)),
                    self.preparer.prepare(
                        &run,
                        &revision,
                        cancellation.clone(),
                    ),
                )
                .await;
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(PortError::Unavailable | PortError::Forbidden) => {
                    let reason = self
                        .blocked(caller, &run, &revision)
                        .await?
                        .unwrap_or(BlockedReason::WorkspaceUnavailable);
                    self.defer_current(&run, reason).await?;
                    deferred = true;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let current = self
                .store
                .run(run.principal.clone(), run.run_id.clone())
                .await
                .map_err(storage)?
                .ok_or(PortError::NotFound)?;
            let now = self.clock.now().utc_ms;
            if now > current.latest_start_at_ms {
                if self
                    .store
                    .worker_expire(
                        current.principal.clone(),
                        current.run_id.clone(),
                        current.phase,
                        now,
                    )
                    .await
                    .map_err(storage)?
                {
                    return Ok(WorkerCycle::Expired {
                        run_id: current.run_id,
                    });
                }
                continue;
            }
            if cancellation.requested().is_some()
                || self.stopping.borrow().is_some()
                || !matches!(
                    current.phase,
                    RunPhase::Queued
                        | RunPhase::Blocked
                        | RunPhase::WaitingWorkspace
                )
            {
                continue;
            }
            let spec = prepared.spec().clone();
            if !snapshot_matches(&spec, &run, &revision, caller) {
                return Err(PortError::InvalidRequest);
            }
            if let Some(reason) = self.blocked(caller, &run, &revision).await? {
                self.defer_current(&run, reason).await?;
                deferred = true;
                continue;
            }
            if !self
                .store
                .worker_start(
                    run.principal.clone(),
                    run.run_id.clone(),
                    current.phase,
                    spec.clone(),
                    self.clock.now().utc_ms,
                )
                .await
                .map_err(storage)?
            {
                continue;
            }
            let progress: Arc<dyn RunProgressSink> =
                Arc::new(DurableProgress {
                    store: self.store.clone(),
                    principal: run.principal.clone(),
                    clock: self.clock.clone(),
                });
            // observe never drops an executing future on cancellation or a DB
            // error: the real executor owns quiescence and the permit lifetime.
            let completion = self
                .observe(&run, &cancellation, None, prepared.execute(progress))
                .await;
            let finalizing = matches!(
                completion.artifact_state,
                ArtifactState::Pending | ArtifactState::RetryableFailed
            ) || completion.execution_outcome
                == ExecutionOutcome::Pending;
            if self
                .store
                .worker_complete(
                    run.principal.clone(),
                    run.run_id.clone(),
                    Some(spec.attempt_id),
                    completion,
                    self.clock.now().utc_ms,
                )
                .await
                .map_err(storage)?
            {
                return Ok(if finalizing {
                    WorkerCycle::Finalizing { run_id: run.run_id }
                } else {
                    WorkerCycle::Settled { run_id: run.run_id }
                });
            }
        }
        Ok(if deferred {
            WorkerCycle::Deferred
        } else {
            WorkerCycle::Idle
        })
    }

    async fn blocked(
        &self,
        caller: &CallerContext,
        run: &RunRecord,
        revision: &RevisionRecord,
    ) -> Result<Option<BlockedReason>, PortError> {
        let request = HostAccessRequest {
            principal: caller.principal.clone(),
            account_generation: caller.account_generation.clone(),
            run_id: Some(run.run_id.clone()),
            grant_ref: revision.grant_ref.clone(),
            operation: AccessOperation::Start,
            requested: revision.definition.capabilities.clone(),
        };
        let blocked = |decision| match decision {
            HostAccessDecision::Allowed => None,
            HostAccessDecision::Blocked(reason) => Some(reason),
            HostAccessDecision::Denied => {
                Some(BlockedReason::AccountUnavailable)
            }
        };
        if let Some(reason) = blocked(self.access.check(&request).await?) {
            return Ok(Some(reason));
        }
        let admission = self
            .admission
            .check(&AdmissionRequest {
                principal: caller.principal.clone(),
                run_id: Some(run.run_id.clone()),
                point: AdmissionPoint::StartAttempt,
            })
            .await?;
        if admission != AdmissionDecision::Allowed {
            return Ok(Some(BlockedReason::StorageUnavailable));
        }
        // Quota lookup can await; close the account-switch window afterwards.
        if let Some(reason) = blocked(self.access.check(&request).await?) {
            return Ok(Some(reason));
        }
        if matches!(revision.definition.action, ActionSpec::Reminder { .. }) {
            return Ok(None);
        }
        self.preparer.blocked_reason(run, revision)
    }

    async fn defer(
        &self,
        run: &RunRecord,
        reason: BlockedReason,
    ) -> Result<(), PortError> {
        self.store
            .worker_blocked(
                run.principal.clone(),
                run.run_id.clone(),
                run.phase,
                reason,
                self.clock.now().utc_ms,
            )
            .await
            .map_err(storage)?;
        Ok(())
    }

    async fn defer_current(
        &self,
        run: &RunRecord,
        reason: BlockedReason,
    ) -> Result<(), PortError> {
        if let Some(current) = self
            .store
            .run(run.principal.clone(), run.run_id.clone())
            .await
            .map_err(storage)?
        {
            self.defer(&current, reason).await?;
        }
        Ok(())
    }

    async fn observe<F: Future>(
        &self,
        run: &RunRecord,
        cancellation: &WorkerCancellation,
        preparation: Option<(&CallerContext, &RevisionRecord)>,
        future: F,
    ) -> F::Output {
        let mut future = std::pin::pin!(future);
        loop {
            let stopping = *self.stopping.borrow();
            if let Some(reason) = stopping {
                cancellation.request(reason);
            }
            if preparation.is_some()
                && self.clock.now().utc_ms > run.latest_start_at_ms
            {
                cancellation.request(StopReason::Deadline);
            }
            if let Some((caller, revision)) = preparation {
                let request = HostAccessRequest {
                    principal: caller.principal.clone(),
                    account_generation: caller.account_generation.clone(),
                    run_id: Some(run.run_id.clone()),
                    grant_ref: revision.grant_ref.clone(),
                    operation: AccessOperation::Start,
                    requested: revision.definition.capabilities.clone(),
                };
                match self.access.check(&request).await {
                    Ok(HostAccessDecision::Allowed) => {}
                    Ok(HostAccessDecision::Denied) => {
                        cancellation.request(StopReason::AccountInvalidated);
                    }
                    Ok(HostAccessDecision::Blocked(_)) => {
                        cancellation.request(StopReason::GrantRevoked);
                    }
                    Err(_) => cancellation.request(StopReason::Shutdown),
                }
            }
            match tokio::time::timeout(
                Duration::from_millis(25),
                future.as_mut(),
            )
            .await
            {
                Ok(value) => return value,
                Err(_) => match self
                    .store
                    .run(run.principal.clone(), run.run_id.clone())
                    .await
                {
                    Ok(Some(current))
                        if matches!(
                            current.phase,
                            RunPhase::Cancelling
                                | RunPhase::Settled
                                | RunPhase::Interrupted
                        ) =>
                    {
                        cancellation.request(StopReason::UserCancelled);
                    }
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => {
                        cancellation.request(StopReason::Shutdown);
                    }
                },
            }
        }
    }
}

fn snapshot_matches(
    spec: &RunSpec,
    run: &RunRecord,
    revision: &RevisionRecord,
    caller: &CallerContext,
) -> bool {
    spec.validate().is_ok()
        && spec.run_id == run.run_id
        && spec.automation_id == run.automation_id
        && spec.revision == run.revision
        && spec.principal == run.principal
        && spec.principal == caller.principal
        && spec.account_generation == caller.account_generation
        && spec.latest_start_at_ms == run.latest_start_at_ms
        && spec.action == revision.definition.action
        && spec.workspace == revision.definition.workspace
        && spec.capabilities == revision.definition.capabilities
        && spec.model == revision.definition.model
        && spec.budget == revision.definition.budget
        && spec.source_session_id == revision.context.source_session_id
        && spec.context_summary == revision.context.summary
}

fn storage(_: StoreError) -> PortError {
    PortError::Storage
}

struct WorkerCancellation {
    reason: watch::Sender<Option<StopReason>>,
}
impl WorkerCancellation {
    fn new() -> Self {
        Self {
            reason: watch::channel(None).0,
        }
    }
    fn request(&self, reason: StopReason) {
        self.reason.send_if_modified(|current| {
            if current.is_some() {
                false
            } else {
                *current = Some(reason);
                true
            }
        });
    }
}
impl CancellationSignal for WorkerCancellation {
    fn requested(&self) -> Option<StopReason> {
        *self.reason.borrow()
    }
    fn cancelled(&self) -> PortFuture<'_, StopReason> {
        Box::pin(async {
            let mut receiver = self.reason.subscribe();
            loop {
                let stopping = *receiver.borrow_and_update();
                if let Some(reason) = stopping {
                    return reason;
                }
                if receiver.changed().await.is_err() {
                    return StopReason::Shutdown;
                }
            }
        })
    }
}

struct DurableProgress {
    store: AutomationStore,
    principal: PrincipalId,
    clock: Arc<dyn Clock>,
}
impl RunProgressSink for DurableProgress {
    fn report(
        &self,
        progress: RunProgress,
    ) -> PortFuture<'_, Result<(), PortError>> {
        Box::pin(async move {
            if self
                .store
                .worker_progress(
                    self.principal.clone(),
                    progress.clone(),
                    self.clock.now().utc_ms,
                )
                .await
                .map_err(storage)?
            {
                return Ok(());
            }
            // Duplicate phase notifications are harmless, but a foreign mapping
            // or a stopped run cannot be acknowledged as a successful write.
            let run = self
                .store
                .run(self.principal.clone(), progress.run_id)
                .await
                .map_err(storage)?
                .ok_or(PortError::NotFound)?;
            if run.current_attempt_id.as_ref() != Some(&progress.attempt_id) {
                return Err(PortError::InvalidRequest);
            }
            if matches!(
                (run.phase, progress.phase),
                (RunPhase::Running, ProgressPhase::Running)
                    | (
                        RunPhase::Cancelling,
                        ProgressPhase::CancellationRequested
                    )
            ) {
                return Ok(());
            }
            if run.phase == RunPhase::Finalizing
                && progress.phase == ProgressPhase::Finalizing
            {
                return Ok(());
            }
            if run.phase == RunPhase::Cancelling
                && progress.phase == ProgressPhase::Running
            {
                return Ok(());
            }
            Err(PortError::RevisionConflict)
        })
    }
}

#[cfg(test)]
mod tests;

//! Reconcile saved identities after the agent has completed its own boot recovery.
//! Unknown or incomplete history never authorizes replaying a model or tool.
use crate::{
    ArtifactState, AttemptRef, Clock, ExecutionOutcome, HistoryCommitState,
    PortError, PrincipalId, ResultRef, RunCompletion, RunExecutor,
    SubmissionEvidence,
    store::{AutomationStore, StoreError},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct RecoveryService {
    store: AutomationStore,
    executor: Arc<dyn RunExecutor>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for RecoveryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryService").finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub settled: usize,
    pub interrupted: usize,
    pub pending_commit: usize,
}

impl RecoveryService {
    pub fn new(
        store: AutomationStore,
        executor: Arc<dyn RunExecutor>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            store,
            executor,
            clock,
        }
    }
    /// The host calls this after agent initialization, before enabling intake.
    pub async fn reconcile(
        &self,
        principal: PrincipalId,
    ) -> Result<RecoveryReport, PortError> {
        let records = self
            .store
            .worker_recoverable(principal.clone())
            .await
            .map_err(storage)?;
        self.reconcile_records(&principal, records).await
    }

    /// Invoked by the same loop that owns execution, after the active
    /// execution future has returned. This path only handles finalization.
    pub async fn retry_requested(
        &self,
        principal: &PrincipalId,
    ) -> Result<(), PortError> {
        let now = self.clock.now().utc_ms;
        let records = self
            .store
            .commit_retry_candidates(principal.clone(), now)
            .await
            .map_err(storage)?;
        for run in records {
            let run_id = run.run_id.clone();
            let result = self.reconcile_records(principal, vec![run]).await;
            self.store
                .defer_commit_retry(principal.clone(), run_id, now)
                .await
                .map_err(storage)?;
            result?;
        }
        Ok(())
    }

    async fn reconcile_records(
        &self,
        principal: &PrincipalId,
        records: Vec<crate::store::RunRecord>,
    ) -> Result<RecoveryReport, PortError> {
        let mut report = RecoveryReport::default();
        for run in records {
            let attempts = self
                .store
                .attempts(principal.clone(), run.run_id.clone())
                .await
                .map_err(storage)?;
            let attempt = attempts.into_iter().find(|a| {
                Some(&a.spec.attempt_id) == run.current_attempt_id.as_ref()
            });
            let Some(attempt) = attempt else {
                if self
                    .store
                    .worker_interrupt(
                        principal.clone(),
                        run.run_id,
                        run.current_attempt_id,
                        self.clock.now().utc_ms,
                    )
                    .await
                    .map_err(storage)?
                {
                    report.interrupted += 1;
                }
                continue;
            };
            let (Some(session_id), Some(submission_id)) =
                (&attempt.spec.session_id, &attempt.spec.submission_id)
            else {
                return Err(PortError::Storage);
            };
            let reference = AttemptRef {
                principal: principal.clone(),
                run_id: run.run_id.clone(),
                attempt_id: attempt.spec.attempt_id.clone(),
                session_id: session_id.clone(),
                submission_id: submission_id.clone(),
            };
            let mut evidence =
                match self.executor.reconcile_attempt(&reference).await {
                    Ok(evidence) => evidence,
                    Err(PortError::Unavailable | PortError::Storage) => {
                        report.pending_commit += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
            if evidence.attempt != reference {
                return Err(PortError::Storage);
            }
            if evidence.submission != SubmissionEvidence::Present
                || evidence.history_commit != HistoryCommitState::Committed
            {
                if self
                    .store
                    .worker_interrupt(
                        principal.clone(),
                        run.run_id,
                        run.current_attempt_id,
                        self.clock.now().utc_ms,
                    )
                    .await
                    .map_err(storage)?
                {
                    report.interrupted += 1;
                }
                continue;
            }
            if matches!(
                evidence.artifact_state,
                ArtifactState::Pending | ArtifactState::RetryableFailed
            ) {
                let Some(commit) = evidence.commit_ref.as_ref() else {
                    return Err(PortError::Storage);
                };
                if commit.session_id != reference.session_id
                    || commit.submission_id != reference.submission_id
                {
                    return Err(PortError::Storage);
                }
                let abandon = self
                    .store
                    .abandon_requested(principal.clone(), run.run_id.clone())
                    .await
                    .map_err(storage)?;
                let operation = if abandon {
                    self.executor.abandon_commit(&reference, commit)
                } else {
                    self.executor.retry_commit(&reference, commit)
                };
                evidence = match operation.await {
                    Ok(evidence) => evidence,
                    Err(PortError::Storage | PortError::Unavailable) => {
                        report.pending_commit += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if evidence.attempt != reference {
                    return Err(PortError::Storage);
                }
            }
            if evidence.submission != SubmissionEvidence::Present
                || evidence.history_commit != HistoryCommitState::Committed
            {
                return Err(PortError::Storage);
            }
            if matches!(
                evidence.artifact_state,
                ArtifactState::Pending | ArtifactState::RetryableFailed
            ) || evidence.execution_outcome == ExecutionOutcome::Pending
            {
                report.pending_commit += 1;
                continue;
            }
            let completion = RunCompletion {
                execution_outcome: evidence.execution_outcome,
                history_commit: evidence.history_commit,
                artifact_state: evidence.artifact_state,
                commit_ref: evidence.commit_ref.clone(),
                result_ref: evidence
                    .commit_ref
                    .map(|commit| ResultRef::SessionTurn { commit }),
                error_code: None,
                stop_reason: evidence.stop_reason,
            };
            if self
                .store
                .worker_complete(
                    principal.clone(),
                    run.run_id,
                    run.current_attempt_id,
                    completion,
                    self.clock.now().utc_ms,
                )
                .await
                .map_err(storage)?
            {
                report.settled += 1;
            }
        }
        Ok(report)
    }
}
fn storage(_: StoreError) -> PortError {
    PortError::Storage
}

#[cfg(test)]
mod tests;

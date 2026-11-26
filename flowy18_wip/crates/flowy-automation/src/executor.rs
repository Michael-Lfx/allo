use crate::{
    ArtifactState, AttemptId, CommitRef, ExecutionOutcome, HistoryCommitState,
    PrincipalId, RunCompletion, RunId, RunPhase, RunSpec, SessionRef,
    StopReason, SubmissionRef,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type PortFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    #[error("automation dependency unavailable")]
    Unavailable,
    #[error("automation storage unavailable")]
    Storage,
    #[error("automation access denied")]
    Forbidden,
    #[error("automation record not found")]
    NotFound,
    #[error("invalid automation request")]
    InvalidRequest,
    #[error("automation request expired")]
    RequestExpired,
    #[error("automation revision changed")]
    RevisionConflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelDisposition {
    Accepted,
    TooLate,
    AlreadyTerminal,
    StorageError,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptRef {
    pub principal: PrincipalId,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub session_id: SessionRef,
    pub submission_id: SubmissionRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelReceipt {
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub disposition: CancelDisposition,
    pub phase: RunPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressPhase {
    WaitingWorkspace,
    Running,
    CancellationRequested,
    Finalizing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunProgress {
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub phase: ProgressPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionEvidence {
    Missing,
    Present,
    Unknown,
}

/// Evidence is read without starting a model or task tool. Unknown history
/// never proves that an attempt did not start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptEvidence {
    pub attempt: AttemptRef,
    pub submission: SubmissionEvidence,
    pub execution_outcome: ExecutionOutcome,
    pub history_commit: HistoryCommitState,
    pub artifact_state: ArtifactState,
    pub commit_ref: Option<CommitRef>,
    #[serde(default)]
    pub stop_reason: Option<StopReason>,
}

pub trait CancellationSignal: Send + Sync {
    fn requested(&self) -> Option<StopReason>;
    fn cancelled(&self) -> PortFuture<'_, StopReason>;
}

pub trait RunProgressSink: Send + Sync {
    fn report(
        &self,
        progress: RunProgress,
    ) -> PortFuture<'_, Result<(), PortError>>;
}

/// Implemented by the shared agent host. Cancellation is a request: neither
/// dropping this future nor receiving Accepted proves tools are quiescent.
pub trait RunExecutor: Send + Sync {
    fn execute(
        &self,
        spec: RunSpec,
        cancellation: Arc<dyn CancellationSignal>,
        progress: Arc<dyn RunProgressSink>,
    ) -> PortFuture<'_, RunCompletion>;

    fn request_cancel<'a>(
        &'a self,
        attempt: &'a AttemptRef,
        reason: StopReason,
    ) -> PortFuture<'a, Result<CancelReceipt, PortError>>;

    fn reconcile_attempt<'a>(
        &'a self,
        attempt: &'a AttemptRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>>;

    /// Retries only the referenced local commit group; it receives no prompt
    /// or tool registry and must not execute the agent again.
    fn retry_commit<'a>(
        &'a self,
        attempt: &'a AttemptRef,
        commit: &'a CommitRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>>;

    fn abandon_commit<'a>(
        &'a self,
        _attempt: &'a AttemptRef,
        _commit: &'a CommitRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>> {
        Box::pin(async { Err(PortError::Unavailable) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AdmissionPolicy, Clock, HostAccess};

    #[test]
    fn host_ports_are_object_safe_without_a_runtime() {
        fn check(
            _: Option<&dyn RunExecutor>,
            _: Option<&dyn CancellationSignal>,
            _: Option<&dyn RunProgressSink>,
            _: Option<&dyn Clock>,
            _: Option<&dyn HostAccess>,
            _: Option<&dyn AdmissionPolicy>,
        ) {
        }
        check(None, None, None, None, None, None);
    }
}

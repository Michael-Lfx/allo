//! Trusted automation adapter. It borrows the existing Agent and never owns
//! an application runtime, account token, scheduler, or foreground session.

use std::collections::BTreeSet;
use std::sync::Arc;

use flowy_automation::store::{RevisionRecord, RunRecord};
use flowy_automation::{
    ActionSpec, ArtifactState, AttemptEvidence, AttemptRef, CancelDisposition,
    CancelReceipt, CancellationSignal, CommitRef, ExecutionOutcome,
    HistoryCommitState, PortError, PortFuture, PreparedRun, ProgressPhase,
    ResultRef, RunCompletion, RunExecutor, RunPhase, RunPreparer, RunProgress,
    RunProgressSink, RunSpec, StopReason, SubmissionEvidence, WorkspaceAccess,
};

use crate::{
    AgentHandle, BudgetStopReason, CancelToken, DetachedArtifacts,
    DetachedAttempt, DetachedAttemptState, DetachedError, DetachedPhase,
    DetachedRun, DetachedRunSpec, DetachedSessionTicket, ExecutionGate,
    ExecutionMetadata, ExecutionOwner, ExecutionSource, InstalledSkill,
    LlmConfig, ModelRoute, SessionOrigin, SessionRole, SessionSpec,
    SessionWorkspace, TurnExecutionOutcome, WorkAccessMode,
};

/// Host implementations resolve current account, route and immutable skill
/// references. Neither method may fall back to the foreground configuration.
pub trait AutomationRunResolver: Send + Sync {
    fn blocked_reason(
        &self,
        _run: &RunRecord,
        _revision: &RevisionRecord,
    ) -> Result<Option<flowy_automation::BlockedReason>, PortError> {
        Ok(None)
    }

    fn bind(
        &self,
        run: &RunRecord,
        revision: &RevisionRecord,
    ) -> Result<RunSpec, PortError>;

    fn resolve(
        &self,
        spec: &RunSpec,
    ) -> Result<ResolvedAutomationRun, PortError>;
}

pub struct ResolvedAutomationRun {
    pub config: LlmConfig,
    pub skills: Vec<InstalledSkill>,
    pub gate: Arc<dyn ExecutionGate>,
}

impl std::fmt::Debug for ResolvedAutomationRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedAutomationRun")
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct AgentRunExecutor {
    agent: AgentHandle,
    resolver: Arc<dyn AutomationRunResolver>,
}

impl std::fmt::Debug for AgentRunExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRunExecutor").finish_non_exhaustive()
    }
}

impl AgentRunExecutor {
    pub fn new(
        agent: AgentHandle,
        resolver: Arc<dyn AutomationRunResolver>,
    ) -> Self {
        Self { agent, resolver }
    }

    async fn prepare_spec(
        &self,
        spec: RunSpec,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> Result<AgentPreparedRun, PortError> {
        spec.validate().map_err(|_| PortError::InvalidRequest)?;
        let ActionSpec::AgentTask { prompt } = &spec.action else {
            return Err(PortError::InvalidRequest);
        };
        if cancellation.requested().is_some() {
            return Err(PortError::Unavailable);
        }
        let resolved = self.resolver.resolve(&spec)?;
        let mut expected_skills = spec.skill_refs.clone();
        let mut actual_skills = resolved
            .skills
            .iter()
            .map(automation_skill_ref)
            .collect::<Vec<_>>();
        expected_skills.sort_unstable();
        actual_skills.sort_unstable();
        if expected_skills != actual_skills {
            return Err(PortError::Forbidden);
        }
        let binding = spec.model.as_ref().ok_or(PortError::InvalidRequest)?;
        let route = ModelRoute {
            provider: resolved.config.provider.clone(),
            model: resolved.config.model.clone(),
            base_url: resolved.config.base_url.clone(),
            context_window: resolved.config.context_window,
            max_tokens: resolved.config.max_tokens,
        };
        let requested = &binding.selected;
        let selected = requested.provider == route.provider
            && requested.model == route.model;
        let fallback = binding.allowed_fallbacks.iter().any(|allowed| {
            allowed.provider == route.provider && allowed.model == route.model
        });
        if (!selected && !fallback)
            || route.base_url.trim().is_empty()
            || spec.credential_ref.as_ref().is_none_or(|reference| {
                reference.principal != spec.principal
                    || reference.provider != route.provider
            })
        {
            return Err(PortError::Forbidden);
        }
        let execution = execution_metadata(&spec);
        // Resolve only as a preflight; the Engine gate obtains the current
        // credential again before each real network request.
        resolved
            .gate
            .model_credential(&execution, &route)
            .map_err(|_| PortError::Unavailable)?;
        let attempt = detached_attempt(&spec)?;
        let access_mode = spec.workspace.as_ref().map_or(
            WorkAccessMode::ReadWrite,
            |workspace| match workspace.access {
                WorkspaceAccess::Read => WorkAccessMode::ReadOnly,
                WorkspaceAccess::Write | WorkspaceAccess::DeviceWrite => {
                    WorkAccessMode::ReadWrite
                }
            },
        );
        let workspace = match &spec.workspace {
            None => None,
            Some(workspace) if workspace.roots.len() == 1 => {
                let has_writes = spec.capabilities.tools.iter().any(|tool| {
                    !matches!(tool.as_str(), "read_file" | "grep" | "glob")
                });
                if workspace.access == WorkspaceAccess::Read && has_writes {
                    return Err(PortError::Forbidden);
                }
                Some(SessionWorkspace {
                    root: workspace.roots[0].clone(),
                })
            }
            Some(_) => return Err(PortError::InvalidRequest),
        };
        let origin = if workspace.is_some() {
            SessionOrigin::Project {
                project_id: Some(format!(
                    "automation-{}",
                    spec.automation_id.as_str()
                )),
            }
        } else {
            SessionOrigin::Home
        };
        let session = SessionSpec {
            id: attempt.session_id.clone(),
            created_at_ms: 0,
            origin,
            workspace,
            execution,
            role: SessionRole::Standard,
            root_session_id: None,
            access_mode,
        };
        let mut text = String::new();
        if let Some(summary) = &spec.context_summary {
            text.push_str(summary.expose());
            text.push_str("\n\n");
        }
        text.push_str(prompt.expose());
        let budget = spec.budget.as_ref().ok_or(PortError::InvalidRequest)?;
        let detached = DetachedRunSpec {
            attempt,
            text,
            config: LlmConfig {
                api_key: None,
                ..resolved.config
            },
            allowed_tools: spec
                .capabilities
                .tools
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            skills: resolved.skills,
            execution_gate: Some(resolved.gate),
            budget: crate::RunBudget {
                max_execution_ms: budget.max_execution_ms,
                max_model_calls: budget.max_model_calls,
                max_tool_calls: budget.max_tool_calls,
            },
        };
        let cancel = CancelToken::new();
        let waiting = self
            .agent
            .reserve_detached_session(session, detached, &cancel);
        tokio::pin!(waiting);
        let ticket = tokio::select! {
            result = &mut waiting => result.map_err(port_error)?,
            _ = cancellation.cancelled() => {
                cancel.cancel();
                let _ = waiting.await;
                return Err(PortError::Unavailable);
            }
        };
        Ok(AgentPreparedRun {
            executor: self.clone(),
            spec,
            ticket,
            cancellation,
        })
    }

    async fn finish_run(
        &self,
        spec: &RunSpec,
        run: DetachedRun,
        cancellation: Arc<dyn CancellationSignal>,
        progress: Arc<dyn RunProgressSink>,
    ) -> RunCompletion {
        let mut phases = run.subscribe();
        let Ok(attempt) = detached_attempt(spec) else {
            return failed("INVALID_ATTEMPT_BINDING");
        };
        let mut progress_failed = progress
            .report(RunProgress {
                run_id: spec.run_id.clone(),
                attempt_id: spec.attempt_id.clone(),
                phase: ProgressPhase::Running,
            })
            .await
            .is_err();
        let mut requested = None;
        let finish = run.wait();
        tokio::pin!(finish);
        let result = loop {
            tokio::select! {
                result = &mut finish => break result,
                reason = cancellation.cancelled(), if requested.is_none() => {
                    requested = Some(reason);
                    let _ = self.agent.cancel_session(&attempt);
                    progress_failed |= progress.report(RunProgress {
                        run_id: spec.run_id.clone(), attempt_id: spec.attempt_id.clone(),
                        phase: ProgressPhase::CancellationRequested,
                    }).await.is_err();
                }
                changed = phases.changed() => {
                    if changed.is_ok() && *phases.borrow_and_update() == DetachedPhase::Finalizing {
                        progress_failed |= progress.report(RunProgress {
                            run_id: spec.run_id.clone(), attempt_id: spec.attempt_id.clone(),
                            phase: ProgressPhase::Finalizing,
                        }).await.is_err();
                    }
                }
            }
        };
        let state = result.ok().and_then(|result| result.evidence.ok());
        let mut completion = completion_from_state(spec, state);
        completion.stop_reason = self
            .agent
            .automation_budget_stop(&attempt)
            .map(budget_reason)
            .or(requested);
        if completion.execution_outcome == ExecutionOutcome::Cancelled {
            completion.execution_outcome = match completion.stop_reason {
                Some(StopReason::Deadline) => ExecutionOutcome::TimedOut,
                Some(
                    StopReason::ModelCallBudget | StopReason::ToolCallBudget,
                ) => ExecutionOutcome::BudgetExceeded,
                _ => ExecutionOutcome::Cancelled,
            };
        }
        if progress_failed {
            completion.error_code = Some("PROGRESS_STORAGE_FAILED".into());
        }
        completion
    }
}

struct AgentPreparedRun {
    executor: AgentRunExecutor,
    spec: RunSpec,
    ticket: DetachedSessionTicket,
    cancellation: Arc<dyn CancellationSignal>,
}

impl PreparedRun for AgentPreparedRun {
    fn spec(&self) -> &RunSpec {
        &self.spec
    }

    fn execute(
        self: Box<Self>,
        progress: Arc<dyn RunProgressSink>,
    ) -> PortFuture<'static, RunCompletion> {
        Box::pin(async move {
            let Self {
                executor,
                spec,
                ticket,
                cancellation,
            } = *self;
            if cancellation.requested().is_some() {
                return RunCompletion {
                    execution_outcome: ExecutionOutcome::Cancelled,
                    stop_reason: cancellation.requested(),
                    ..failed("CANCELLED_BEFORE_START")
                };
            }
            match ticket.start() {
                Ok(run) => {
                    executor
                        .finish_run(&spec, run, cancellation, progress)
                        .await
                }
                Err(_) => failed("AGENT_START_FAILED"),
            }
        })
    }
}

impl RunPreparer for AgentRunExecutor {
    fn blocked_reason(
        &self,
        run: &RunRecord,
        revision: &RevisionRecord,
    ) -> Result<Option<flowy_automation::BlockedReason>, PortError> {
        self.resolver.blocked_reason(run, revision)
    }

    fn prepare<'a>(
        &'a self,
        run: &'a RunRecord,
        revision: &'a RevisionRecord,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> PortFuture<'a, Result<Box<dyn PreparedRun>, PortError>> {
        Box::pin(async move {
            let spec = self.resolver.bind(run, revision)?;
            if spec.principal != run.principal
                || spec.automation_id != run.automation_id
                || spec.run_id != run.run_id
                || spec.revision != run.revision
                || revision.principal != run.principal
                || revision.revision != run.revision
                || revision.automation_id != run.automation_id
                || spec.action != revision.definition.action
                || spec.capabilities != revision.definition.capabilities
                || spec.model != revision.definition.model
                || spec.budget != revision.definition.budget
                || spec.workspace != revision.definition.workspace
                || spec.source_session_id != revision.context.source_session_id
                || spec.context_summary != revision.context.summary
                || spec.latest_start_at_ms != run.latest_start_at_ms
            {
                return Err(PortError::InvalidRequest);
            }
            let prepared: Box<dyn PreparedRun> =
                Box::new(self.prepare_spec(spec, cancellation).await?);
            Ok(prepared)
        })
    }
}

impl RunExecutor for AgentRunExecutor {
    fn execute(
        &self,
        spec: RunSpec,
        cancellation: Arc<dyn CancellationSignal>,
        progress: Arc<dyn RunProgressSink>,
    ) -> PortFuture<'_, RunCompletion> {
        Box::pin(async move {
            match self.prepare_spec(spec, cancellation).await {
                Ok(prepared) => Box::new(prepared).execute(progress).await,
                Err(_) => failed("AGENT_PREPARE_FAILED"),
            }
        })
    }

    fn request_cancel<'a>(
        &'a self,
        attempt: &'a AttemptRef,
        _reason: StopReason,
    ) -> PortFuture<'a, Result<CancelReceipt, PortError>> {
        Box::pin(async move {
            let detached = self
                .agent
                .automation_attempt_binding(attempt)
                .map_err(port_error)?
                .ok_or(PortError::NotFound)?;
            let disposition = match self
                .agent
                .cancel_session(&detached)
                .map_err(port_error)?
            {
                crate::CancelDisposition::Accepted => {
                    CancelDisposition::Accepted
                }
                crate::CancelDisposition::TooLate => CancelDisposition::TooLate,
                crate::CancelDisposition::NotRunning
                | crate::CancelDisposition::NotFound => {
                    CancelDisposition::AlreadyTerminal
                }
            };
            Ok(CancelReceipt {
                run_id: attempt.run_id.clone(),
                attempt_id: attempt.attempt_id.clone(),
                disposition,
                phase: if disposition == CancelDisposition::Accepted {
                    RunPhase::Cancelling
                } else {
                    RunPhase::Finalizing
                },
            })
        })
    }

    fn reconcile_attempt<'a>(
        &'a self,
        attempt: &'a AttemptRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>> {
        Box::pin(async move {
            let state = match self
                .agent
                .automation_attempt_binding(attempt)
                .map_err(port_error)?
            {
                Some(detached) => self
                    .agent
                    .reconcile_attempt(&detached)
                    .map_err(port_error)?,
                None => DetachedAttemptState::MissingSession,
            };
            let mut evidence = attempt_evidence(attempt, state);
            if let Some(reason) = self
                .agent
                .automation_attempt_binding(attempt)
                .ok()
                .flatten()
                .and_then(|detached| {
                    self.agent.automation_budget_stop(&detached)
                })
            {
                evidence.stop_reason = Some(budget_reason(reason));
                if evidence.execution_outcome == ExecutionOutcome::Cancelled {
                    evidence.execution_outcome = match reason {
                        BudgetStopReason::Deadline => {
                            ExecutionOutcome::TimedOut
                        }
                        BudgetStopReason::ModelCallBudget
                        | BudgetStopReason::ToolCallBudget => {
                            ExecutionOutcome::BudgetExceeded
                        }
                    };
                }
            }
            Ok(evidence)
        })
    }

    fn retry_commit<'a>(
        &'a self,
        attempt: &'a AttemptRef,
        commit: &'a CommitRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>> {
        Box::pin(async move {
            if commit.session_id != attempt.session_id
                || commit.submission_id != attempt.submission_id
            {
                return Err(PortError::InvalidRequest);
            }
            let detached = self
                .agent
                .automation_attempt_binding(attempt)
                .map_err(port_error)?
                .ok_or(PortError::NotFound)?;
            let state = self
                .agent
                .reconcile_attempt(&detached)
                .map_err(port_error)?;
            match state {
                DetachedAttemptState::Terminal(ref completion)
                    if matches!(
                        completion.artifacts,
                        DetachedArtifacts::Pending(_)
                    ) =>
                {
                    self.agent
                        .retry_delivery_commit(
                            attempt.session_id.as_str(),
                            attempt.submission_id.as_str(),
                        )
                        .map_err(|_| PortError::Storage)?;
                }
                DetachedAttemptState::Terminal(_) => {}
                _ => return Err(PortError::InvalidRequest),
            }
            self.reconcile_attempt(attempt).await
        })
    }

    fn abandon_commit<'a>(
        &'a self,
        attempt: &'a AttemptRef,
        commit: &'a CommitRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>> {
        Box::pin(async move {
            if commit.session_id != attempt.session_id
                || commit.submission_id != attempt.submission_id
            {
                return Err(PortError::InvalidRequest);
            }
            let evidence = self.reconcile_attempt(attempt).await?;
            if matches!(
                evidence.artifact_state,
                ArtifactState::Pending | ArtifactState::RetryableFailed
            ) && evidence.history_commit == HistoryCommitState::Committed
            {
                self.agent
                    .abandon_automation_delivery(
                        attempt.session_id.as_str(),
                        attempt.submission_id.as_str(),
                    )
                    .map_err(|_| PortError::Storage)?;
            }
            self.reconcile_attempt(attempt).await
        })
    }
}

pub fn execution_metadata(spec: &RunSpec) -> ExecutionMetadata {
    ExecutionMetadata {
        owner: Some(ExecutionOwner {
            authority: spec.principal.authority().to_owned(),
            subject: spec.principal.external_user_id().to_string(),
        }),
        source: ExecutionSource::Automation {
            automation_id: spec.automation_id.as_str().to_owned(),
            run_id: spec.run_id.as_str().to_owned(),
            attempt_id: spec.attempt_id.as_str().to_owned(),
        },
    }
}

/// Stable skill identity, exact installed version and content fingerprint.
pub fn automation_skill_ref(skill: &InstalledSkill) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{}/{}/{}#sha256:{:x}",
        skill.id.owner_handle,
        skill.id.slug,
        skill.id.version,
        Sha256::digest(skill.content.as_bytes())
    )
}

fn detached_attempt(spec: &RunSpec) -> Result<DetachedAttempt, PortError> {
    let session = spec.session_id.as_ref().ok_or(PortError::InvalidRequest)?;
    let submission = spec
        .submission_id
        .as_ref()
        .ok_or(PortError::InvalidRequest)?;
    let id = submission
        .as_str()
        .strip_prefix("submission-")
        .filter(|id| id.len() == 32)
        .ok_or(PortError::InvalidRequest)?;
    let attempt = DetachedAttempt {
        session_id: session.as_str().to_owned(),
        submission: u128::from_str_radix(id, 16)
            .map_err(|_| PortError::InvalidRequest)?,
        execution: execution_metadata(spec),
    };
    if attempt.submission_id() != submission.as_str() {
        return Err(PortError::InvalidRequest);
    }
    Ok(attempt)
}

fn failed(code: &str) -> RunCompletion {
    RunCompletion {
        execution_outcome: ExecutionOutcome::Interrupted,
        history_commit: HistoryCommitState::Unknown,
        artifact_state: ArtifactState::NotRequired,
        commit_ref: None,
        result_ref: None,
        error_code: Some(code.into()),
        stop_reason: None,
    }
}

fn completion_from_state(
    spec: &RunSpec,
    state: Option<DetachedAttemptState>,
) -> RunCompletion {
    let Some(DetachedAttemptState::Terminal(completion)) = state else {
        return failed("ATTEMPT_EVIDENCE_UNAVAILABLE");
    };
    let commit = CommitRef {
        session_id: spec.session_id.clone().expect("validated Agent session"),
        submission_id: spec
            .submission_id
            .clone()
            .expect("validated Agent submission"),
    };
    RunCompletion {
        execution_outcome: outcome(completion.evidence.outcome),
        history_commit: HistoryCommitState::Committed,
        artifact_state: artifact_state(&completion.artifacts),
        commit_ref: Some(commit.clone()),
        result_ref: Some(ResultRef::SessionTurn { commit }),
        error_code: matches!(
            completion.artifacts,
            DetachedArtifacts::Pending(_)
        )
        .then(|| "ARTIFACT_COMMIT_FAILED".into()),
        stop_reason: None,
    }
}

fn attempt_evidence(
    attempt: &AttemptRef,
    state: DetachedAttemptState,
) -> AttemptEvidence {
    let (submission, execution_outcome, history_commit, artifacts) = match state
    {
        DetachedAttemptState::MissingSession
        | DetachedAttemptState::Interrupted => (
            SubmissionEvidence::Unknown,
            ExecutionOutcome::Interrupted,
            HistoryCommitState::Unknown,
            ArtifactState::NotRequired,
        ),
        DetachedAttemptState::NotStarted => (
            SubmissionEvidence::Missing,
            ExecutionOutcome::Pending,
            HistoryCommitState::NotStarted,
            ArtifactState::NotRequired,
        ),
        DetachedAttemptState::Running | DetachedAttemptState::Cancelling => (
            SubmissionEvidence::Present,
            ExecutionOutcome::Pending,
            HistoryCommitState::Unknown,
            ArtifactState::Pending,
        ),
        DetachedAttemptState::Terminal(completion) => (
            SubmissionEvidence::Present,
            outcome(completion.evidence.outcome),
            HistoryCommitState::Committed,
            artifact_state(&completion.artifacts),
        ),
    };
    AttemptEvidence {
        attempt: attempt.clone(),
        submission,
        execution_outcome,
        history_commit,
        artifact_state: artifacts,
        stop_reason: None,
        commit_ref: (history_commit == HistoryCommitState::Committed).then(
            || CommitRef {
                session_id: attempt.session_id.clone(),
                submission_id: attempt.submission_id.clone(),
            },
        ),
    }
}

const fn outcome(value: TurnExecutionOutcome) -> ExecutionOutcome {
    match value {
        TurnExecutionOutcome::Completed => ExecutionOutcome::Completed,
        TurnExecutionOutcome::Failed => ExecutionOutcome::Failed,
        TurnExecutionOutcome::Stopped => ExecutionOutcome::Cancelled,
    }
}

const fn artifact_state(value: &DetachedArtifacts) -> ArtifactState {
    match value {
        DetachedArtifacts::NotRequired => ArtifactState::NotRequired,
        DetachedArtifacts::Pending(_) => ArtifactState::RetryableFailed,
        DetachedArtifacts::Committed(_) => ArtifactState::Committed,
        DetachedArtifacts::Abandoned => ArtifactState::Abandoned,
    }
}

const fn budget_reason(value: BudgetStopReason) -> StopReason {
    match value {
        BudgetStopReason::Deadline => StopReason::Deadline,
        BudgetStopReason::ModelCallBudget => StopReason::ModelCallBudget,
        BudgetStopReason::ToolCallBudget => StopReason::ToolCallBudget,
    }
}

const fn port_error(error: DetachedError) -> PortError {
    match error {
        DetachedError::InvalidBinding
        | DetachedError::BindingConflict
        | DetachedError::EvidenceConflict => PortError::InvalidRequest,
        DetachedError::Storage | DetachedError::StorageUnreliable => {
            PortError::Storage
        }
        _ => PortError::Unavailable,
    }
}

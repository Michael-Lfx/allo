//! Host-only execution of one immutable automation attempt.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flowy_agent_core::{
    CancelDisposition, CancelToken, DeliveryCommit, DeliveryCommitter, Engine,
    EngineConfig, ExecutionGate, ExecutionMetadata, ExecutionSource,
    PromptTurn, Role, RunBudget, RunError, SessionCreateDisposition,
    SessionOrigin, SessionRecord, SessionSpec, SessionWorkspace,
    TurnCompletionEvidence,
    tools::{has_reserved_device_component, is_windows_reparse_point},
};
use flowy_skills::InstalledSkill;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::{
    AgentHandle, AgentInitialization, ArtifactStore, LlmConfig, lock,
    log_storage_io_error,
};
use crate::execution_host::{
    append_skill_listing, prepare_skill_prompt,
    track_unproven_process_dispatches,
};
use crate::workspace_arbiter::{
    AdmissionError, ExecutionClass, ExecutionPermit, WorkspaceRequest,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetachedAttempt {
    pub session_id: String,
    pub submission: u128,
    pub execution: ExecutionMetadata,
}

impl DetachedAttempt {
    #[must_use]
    pub fn submission_id(&self) -> String {
        format!("submission-{:032x}", self.submission)
    }

    fn validate(&self) -> Result<(), DetachedError> {
        validate_identity(&self.session_id, &self.execution)
    }
}

/// Resolved, request-scoped inputs supplied by a trusted execution host.
/// The allowlist and skills are independent of the current foreground context.
pub struct DetachedRunSpec {
    pub attempt: DetachedAttempt,
    pub text: String,
    pub config: LlmConfig,
    pub allowed_tools: BTreeSet<String>,
    pub skills: Vec<InstalledSkill>,
    pub execution_gate: Option<Arc<dyn ExecutionGate>>,
    pub budget: RunBudget,
}

impl DetachedRunSpec {
    #[must_use]
    pub fn new(
        attempt: DetachedAttempt,
        text: String,
        config: LlmConfig,
    ) -> Self {
        Self {
            attempt,
            text,
            config,
            allowed_tools: BTreeSet::new(),
            skills: Vec::new(),
            execution_gate: None,
            budget: RunBudget::new(600_000, 20, 50)
                .expect("positive detached budget"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachedError {
    Initializing,
    StorageUnreliable,
    ShuttingDown,
    InvalidBinding,
    BindingConflict,
    WorkspaceUnavailable,
    EmptyPrompt,
    MissingConfiguration,
    ToolUnavailable,
    InvalidSkill,
    Busy,
    Cancelled,
    WorkspaceQuarantined,
    AlreadySubmitted,
    Storage,
    EvidenceConflict,
    Execution,
}

impl fmt::Display for DetachedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Initializing => "agent storage is initializing",
            Self::StorageUnreliable => "agent storage is unreliable",
            Self::ShuttingDown => "agent is shutting down",
            Self::InvalidBinding => "detached execution binding is invalid",
            Self::BindingConflict => {
                "detached execution binding conflicts with history"
            }
            Self::WorkspaceUnavailable => "detached workspace is unavailable",
            Self::EmptyPrompt => "detached prompt is empty",
            Self::MissingConfiguration => {
                "detached model configuration is unavailable"
            }
            Self::ToolUnavailable => "requested detached tool is unavailable",
            Self::InvalidSkill => "requested detached skill is unavailable",
            Self::Busy => "detached session is busy",
            Self::Cancelled => "detached start was cancelled while waiting",
            Self::WorkspaceQuarantined => {
                "workspace writers require a quiescence confirmation"
            }
            Self::AlreadySubmitted => "detached submission is already recorded",
            Self::Storage => {
                "detached execution could not access durable storage"
            }
            Self::EvidenceConflict => {
                "detached completion evidence is inconsistent"
            }
            Self::Execution => {
                "detached execution stopped without a normal result"
            }
        })
    }
}

impl std::error::Error for DetachedError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachedPhase {
    Running,
    Finalizing,
    ExecutionFinished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetachedArtifacts {
    NotRequired,
    Pending(DeliveryCommit),
    Committed(DeliveryCommit),
    Abandoned,
}

/// Read-only adjudication of one exact attempt. Only Terminal contains a
/// durable outcome; Interrupted never authorizes replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetachedAttemptState {
    MissingSession,
    NotStarted,
    Running,
    Cancelling,
    Interrupted,
    Terminal(Box<DetachedCompletion>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetachedCompletion {
    pub evidence: TurnCompletionEvidence,
    pub artifacts: DetachedArtifacts,
}

impl fmt::Debug for DetachedRunSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DetachedRunSpec")
            .field("attempt", &self.attempt)
            .field("allowed_tools", &self.allowed_tools)
            .finish_non_exhaustive()
    }
}

/// Separate the future's error from the history and delivery evidence.
#[derive(Debug)]
pub struct DetachedRunResult {
    pub evidence: Result<DetachedAttemptState, DetachedError>,
    pub execution_error: Option<DetachedError>,
}

/// Dropping the observation handle detaches it from the already-owned worker.
/// Cancellation uses the target session and the durable attempt binding.
#[derive(Debug)]
pub struct DetachedRun {
    phases: watch::Receiver<DetachedPhase>,
    task: JoinHandle<DetachedRunResult>,
}

impl DetachedRun {
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<DetachedPhase> {
        self.phases.clone()
    }

    pub async fn wait(self) -> Result<DetachedRunResult, DetachedError> {
        self.task.await.map_err(|_| DetachedError::Execution)
    }
}

struct PreparedDetached {
    engine: Arc<Engine>,
    cwd: PathBuf,
    prompt: PromptTurn,
    config: EngineConfig,
    store: ArtifactStore,
    phase_sender: watch::Sender<DetachedPhase>,
    phases: watch::Receiver<DetachedPhase>,
    unproven_process_started: Arc<AtomicBool>,
}

/// Reserved before a new session exists. Its start is called only after the
/// Automation database has durably recorded the exact attempt mapping.
pub struct DetachedSessionTicket {
    handle: AgentHandle,
    session: SessionSpec,
    spec: DetachedRunSpec,
    permit: ExecutionPermit,
}

impl fmt::Debug for DetachedSessionTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DetachedSessionTicket")
            .field("attempt", &self.spec.attempt)
            .finish_non_exhaustive()
    }
}

impl DetachedSessionTicket {
    pub fn start(self) -> Result<DetachedRun, DetachedError> {
        let Self {
            handle,
            session,
            spec,
            permit,
        } = self;
        handle.create_detached(session)?;
        let _dispatch = lock(&handle.inner.state.dispatch);
        let prepared = handle.prepare_detached(&spec)?;
        if !permit
            .covers(ExecutionClass::Background, &prepared.resources(&spec)?)
        {
            return Err(DetachedError::WorkspaceUnavailable);
        }
        handle.launch_detached(spec, prepared, permit)
    }
}

impl PreparedDetached {
    fn resources(
        &self,
        spec: &DetachedRunSpec,
    ) -> Result<WorkspaceRequest, DetachedError> {
        WorkspaceRequest::for_tools(&self.cwd, &self.config.registry.list())
            .and_then(|request| {
                request.with_skill_reads(
                    spec.skills.iter().map(|skill| skill.root.clone()),
                )
            })
            .map_err(|_| DetachedError::WorkspaceUnavailable)
    }
}

const fn admission_error(error: AdmissionError) -> DetachedError {
    match error {
        AdmissionError::Closed => DetachedError::ShuttingDown,
        AdmissionError::Cancelled => DetachedError::Cancelled,
        AdmissionError::Quarantined => DetachedError::WorkspaceQuarantined,
        AdmissionError::Storage => DetachedError::Storage,
    }
}

impl AgentHandle {
    /// Reserve launch resources before creating either the session or its
    /// submission. Home uses a conservative device ticket until its private
    /// workspace exists; Project uses the canonical requested directory.
    pub async fn reserve_detached_session(
        &self,
        session: SessionSpec,
        spec: DetachedRunSpec,
        cancel: &CancelToken,
    ) -> Result<DetachedSessionTicket, DetachedError> {
        spec.attempt.validate()?;
        if session.id != spec.attempt.session_id
            || session.execution != spec.attempt.execution
        {
            return Err(DetachedError::BindingConflict);
        }
        self.detached_engine()?;
        self.check_detached_intake()?;
        let tools = spec.allowed_tools.iter().cloned().collect::<Vec<_>>();
        let resources = match &session.origin {
            SessionOrigin::Home if session.workspace.is_none() => {
                WorkspaceRequest::DeviceWrite
            }
            SessionOrigin::Project {
                project_id: Some(id),
            } if !id.trim().is_empty() => {
                let root = session
                    .workspace
                    .as_ref()
                    .ok_or(DetachedError::WorkspaceUnavailable)?;
                WorkspaceRequest::for_tools(&root.root, &tools)
                    .and_then(|resources| {
                        resources.with_skill_reads(
                            spec.skills.iter().map(|skill| skill.root.clone()),
                        )
                    })
                    .map_err(|_| DetachedError::WorkspaceUnavailable)?
            }
            _ => return Err(DetachedError::InvalidBinding),
        };
        let permit = self
            .inner
            .state
            .coordinator
            .acquire(ExecutionClass::Background, resources, cancel)
            .await
            .map_err(admission_error)?;
        if cancel.is_cancelled() {
            return Err(DetachedError::Cancelled);
        }
        Ok(DetachedSessionTicket {
            handle: self.clone(),
            session,
            spec,
            permit,
        })
    }
    pub(crate) fn automation_attempt_binding(
        &self,
        reference: &flowy_automation::AttemptRef,
    ) -> Result<Option<DetachedAttempt>, DetachedError> {
        let engine = self.detached_engine()?;
        let Some(record) = engine.session(reference.session_id.as_str()) else {
            return Ok(None);
        };
        let owner = flowy_agent_core::ExecutionOwner {
            authority: reference.principal.authority().to_owned(),
            subject: reference.principal.external_user_id().to_string(),
        };
        if record.execution.owner.as_ref() != Some(&owner)
            || !matches!(&record.execution.source, ExecutionSource::Automation { run_id, attempt_id, .. }
                if run_id == reference.run_id.as_str() && attempt_id == reference.attempt_id.as_str())
        {
            return Err(DetachedError::BindingConflict);
        }
        let submission = reference
            .submission_id
            .as_str()
            .strip_prefix("submission-")
            .filter(|id| id.len() == 32)
            .and_then(|id| u128::from_str_radix(id, 16).ok())
            .ok_or(DetachedError::InvalidBinding)?;
        let attempt = DetachedAttempt {
            session_id: reference.session_id.as_str().to_owned(),
            submission,
            execution: record.execution,
        };
        if attempt.submission_id() != reference.submission_id.as_str() {
            return Err(DetachedError::InvalidBinding);
        }
        Ok(Some(attempt))
    }

    pub(crate) fn automation_budget_stop(
        &self,
        attempt: &DetachedAttempt,
    ) -> Option<flowy_agent_core::BudgetStopReason> {
        self.detached_engine()
            .ok()?
            .run_budget_checkpoint(
                &attempt.session_id,
                &attempt.submission_id(),
            )
            .ok()
            .flatten()
            .and_then(|checkpoint| checkpoint.stop_reason)
    }

    /// Persist a Home or Project session without selecting or projecting it.
    /// A Home workspace is allocated below the host's own sessions directory.
    pub fn create_detached(
        &self,
        mut spec: SessionSpec,
    ) -> Result<SessionCreateDisposition, DetachedError> {
        validate_identity(&spec.id, &spec.execution)?;
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = self.detached_engine()?;
        self.check_detached_intake()?;
        let stored_workspace = engine
            .with_session(&spec.id, |record| {
                if spec.origin != record.origin
                    || spec.execution != record.execution
                {
                    return Err(DetachedError::BindingConflict);
                }
                Ok(record.workspace.clone())
            })
            .transpose()?;
        let workspace = match &spec.origin {
            SessionOrigin::Home => {
                let root = self
                    .inner
                    .state
                    .root
                    .canonicalize()
                    .map_err(|_| DetachedError::WorkspaceUnavailable)?;
                let expected =
                    root.join("sessions").join("workspaces").join(&spec.id);
                if spec
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.root != expected)
                {
                    return Err(DetachedError::BindingConflict);
                }
                if stored_workspace.is_none()
                    && let ExecutionSource::Automation { attempt_id, .. } =
                        &spec.execution.source
                    && spec.id.strip_prefix("automation-") == Some(attempt_id)
                {
                    let sessions = private_child(&root, "sessions", true)?;
                    let workspaces =
                        private_child(&sessions, "workspaces", true)?;
                    super::automation_retention::initialize_owned_workspace(
                        &workspaces,
                        &spec.id,
                    )
                    .map_err(|_| DetachedError::WorkspaceUnavailable)?
                } else {
                    session_home_workspace(
                        &root,
                        &spec.id,
                        stored_workspace.is_none(),
                    )?
                }
            }
            SessionOrigin::Project {
                project_id: Some(project_id),
            } if !project_id.trim().is_empty() => canonical_directory(
                &spec
                    .workspace
                    .as_ref()
                    .ok_or(DetachedError::WorkspaceUnavailable)?
                    .root,
            )?,
            _ => return Err(DetachedError::InvalidBinding),
        };
        spec.workspace = Some(SessionWorkspace { root: workspace });
        engine
            .create_if_absent(spec)
            .map_err(|error| match error.kind() {
                io::ErrorKind::AlreadyExists => DetachedError::BindingConflict,
                io::ErrorKind::InvalidInput => DetachedError::InvalidBinding,
                _ => {
                    log_storage_io_error(
                        "create detached session",
                        engine.root(),
                        &error,
                    );
                    DetachedError::Storage
                }
            })
    }

    /// Start immediately with a complete workspace and capacity ticket.
    /// A busy result has not recorded or started the submission.
    pub fn start_detached(
        &self,
        spec: DetachedRunSpec,
    ) -> Result<DetachedRun, DetachedError> {
        let _dispatch = lock(&self.inner.state.dispatch);
        let prepared = self.prepare_detached(&spec)?;
        let resources = prepared.resources(&spec)?;
        let permit = self
            .inner
            .state
            .coordinator
            .try_acquire(ExecutionClass::Background, resources)
            .map_err(admission_error)?
            .ok_or(DetachedError::Busy)?;
        self.launch_detached(spec, prepared, permit)
    }

    /// Wait for all resources without holding any execution slot or writing a
    /// submission. Cancellation drops the queued candidate, never the real
    /// engine future after it has started. Host shutdown wakes every waiter.
    pub async fn start_detached_waiting(
        &self,
        spec: DetachedRunSpec,
        cancel: &CancelToken,
    ) -> Result<DetachedRun, DetachedError> {
        let (prepared, resources) = {
            let _dispatch = lock(&self.inner.state.dispatch);
            let prepared = self.prepare_detached(&spec)?;
            let resources = prepared.resources(&spec)?;
            (prepared, resources)
        };
        let permit = self
            .inner
            .state
            .coordinator
            .acquire(ExecutionClass::Background, resources, cancel)
            .await
            .map_err(admission_error)?;
        let _dispatch = lock(&self.inner.state.dispatch);
        if cancel.is_cancelled() {
            permit.complete();
            return Err(DetachedError::Cancelled);
        }
        // Re-resolve after waiting; a ticket for a renamed/replaced root must
        // not authorize a different workspace. The gate independently checks
        // the current account and credentials at each actual request.
        let validation = (|| {
            self.check_detached_intake()?;
            let record =
                Self::validate_detached_start(&prepared.engine, &spec)?;
            let cwd = checked_workspace(&self.inner.state.root, &record)?;
            let resources = WorkspaceRequest::for_tools(
                &cwd,
                &prepared.config.registry.list(),
            )
            .and_then(|request| {
                request.with_skill_reads(
                    spec.skills.iter().map(|skill| skill.root.clone()),
                )
            })
            .map_err(|_| DetachedError::WorkspaceUnavailable)?;
            if cwd != prepared.cwd
                || !permit.covers(ExecutionClass::Background, &resources)
            {
                return Err(DetachedError::WorkspaceUnavailable);
            }
            Ok(())
        })();
        if let Err(error) = validation {
            permit.complete();
            return Err(error);
        }
        self.launch_detached(spec, prepared, permit)
    }

    fn validate_detached_start(
        engine: &Engine,
        spec: &DetachedRunSpec,
    ) -> Result<SessionRecord, DetachedError> {
        let record = attempt_record(engine, &spec.attempt)?
            .ok_or(DetachedError::InvalidBinding)?;
        if has_attempt_history(&record, &spec.attempt.submission_id()) {
            return Err(DetachedError::AlreadySubmitted);
        }
        if engine.submitted_session(spec.attempt.submission).is_some() {
            return Err(DetachedError::BindingConflict);
        }
        Ok(record)
    }

    fn prepare_detached(
        &self,
        spec: &DetachedRunSpec,
    ) -> Result<PreparedDetached, DetachedError> {
        spec.attempt.validate()?;
        let text = spec.text.trim();
        if text.is_empty() {
            return Err(DetachedError::EmptyPrompt);
        }
        if spec.config.model.trim().is_empty()
            || spec.config.provider.trim().is_empty()
            || spec.config.base_url.trim().is_empty()
            || spec.execution_gate.is_none()
        {
            return Err(DetachedError::MissingConfiguration);
        }
        let engine = self.detached_engine()?;
        self.check_detached_intake()?;
        let record = Self::validate_detached_start(&engine, spec)?;
        let submission_id = spec.attempt.submission_id();
        let cwd = checked_workspace(&self.inner.state.root, &record)?;
        let (prompt, active_skills) = prepare_skill_prompt(text, &spec.skills)
            .map_err(|_| DetachedError::InvalidSkill)?;
        let (phase_sender, phases) = watch::channel(DetachedPhase::Running);
        let store = self
            .inner
            .state
            .artifact_store
            .get()
            .cloned()
            .ok_or(DetachedError::StorageUnreliable)?;
        let mut config = super::engine_config(&spec.config, &cwd);
        config.registry =
            super::tool_registry(&cwd, &spec.skills, active_skills);
        config.api_key = None;
        config.execution_gate.clone_from(&spec.execution_gate);
        config.run_budget = Some(spec.budget);
        if record.origin == SessionOrigin::Home
            && spec.allowed_tools.contains("publish_file")
        {
            configure_delivery(
                &mut config,
                &cwd,
                &submission_id,
                &self.inner.state.root,
                &store,
                phase_sender.clone(),
            );
        }
        scope_tools(&config, &spec.allowed_tools)?;
        config.system = append_skill_listing(&config.system, &spec.skills);
        let unproven_process_started =
            track_unproven_process_dispatches(&config.registry);
        Ok(PreparedDetached {
            engine,
            cwd,
            prompt,
            config,
            store,
            phase_sender,
            phases,
            unproven_process_started,
        })
    }

    fn launch_detached(
        &self,
        spec: DetachedRunSpec,
        prepared: PreparedDetached,
        permit: ExecutionPermit,
    ) -> Result<DetachedRun, DetachedError> {
        let PreparedDetached {
            engine,
            cwd,
            prompt,
            config,
            store,
            phase_sender,
            phases,
            unproven_process_started,
        } = prepared;
        if config.delivery_committer.is_some() {
            let parent = private_child(&cwd, "outputs", true)?;
            private_child(&parent, &spec.attempt.submission_id(), true)?;
        }
        let turn = engine.start_with_config(
            spec.attempt.session_id.clone(),
            prompt,
            config,
            None,
            Some(spec.attempt.submission),
        );
        let turn = match turn {
            Ok(turn) => turn,
            Err(error) => {
                // No engine future was launched, so no tools can be alive.
                permit.complete();
                return Err(detached_run_error(&engine, error));
            }
        };
        let attempt = spec.attempt;
        let permit = permit.started();
        let task = self.inner.runtime.spawn(async move {
            let execution_error = turn
                .await
                .err()
                .map(|error| detached_run_error(&engine, error));
            let evidence = observe_attempt(&engine, &attempt)
                .and_then(|state| check_delivery_marker(state, &store));
            permit.settle(unproven_process_started.load(Ordering::Acquire));
            phase_sender.send_replace(DetachedPhase::ExecutionFinished);
            DetachedRunResult {
                evidence,
                execution_error,
            }
        });
        Ok(DetachedRun { phases, task })
    }

    /// Fail closed when recovery has evidence of an unaccounted writer.
    /// The marker contains no user data and survives a crash or restart.
    pub fn quarantine_workspace_writers(&self) -> io::Result<()> {
        self.inner.state.coordinator.quarantine_device()
    }

    #[must_use]
    pub fn workspace_writers_quarantined(&self) -> bool {
        self.inner.state.coordinator.is_quarantined()
    }

    /// A trusted host may clear a crash quarantine only after explicitly
    /// confirming that no process or tool from the former host can write.
    /// Active and waiting writers make confirmation fail with `WouldBlock`.
    pub fn confirm_workspace_quiescence(
        &self,
        confirmed_no_surviving_writers: bool,
    ) -> io::Result<()> {
        self.inner
            .state
            .coordinator
            .clear_device_quarantine(confirmed_no_surviving_writers)
    }

    /// Cancel only the supplied attempt, without altering foreground state.
    pub fn cancel_session(
        &self,
        attempt: &DetachedAttempt,
    ) -> Result<CancelDisposition, DetachedError> {
        attempt.validate()?;
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = self.detached_engine()?;
        let Some(record) = attempt_record(&engine, attempt)? else {
            return Ok(CancelDisposition::NotFound);
        };
        if !has_attempt_history(&record, &attempt.submission_id()) {
            return Ok(CancelDisposition::NotRunning);
        }
        engine
            .request_cancel_typed(&attempt.session_id)
            .map_err(|error| {
                log_storage_io_error(
                    "cancel detached attempt",
                    engine.root(),
                    &error,
                );
                DetachedError::Storage
            })
    }

    /// Query durable evidence without replaying execution or changing active.
    pub fn reconcile_attempt(
        &self,
        attempt: &DetachedAttempt,
    ) -> Result<DetachedAttemptState, DetachedError> {
        attempt.validate()?;
        let observation = {
            let _dispatch = lock(&self.inner.state.dispatch);
            let engine = self.detached_engine()?;
            observe_attempt(&engine, attempt)?
        };
        let store = self
            .inner
            .state
            .artifact_store
            .get()
            .ok_or(DetachedError::StorageUnreliable)?;
        check_delivery_marker(observation, store)
    }

    fn detached_engine(&self) -> Result<Arc<Engine>, DetachedError> {
        match self.initialization_state() {
            AgentInitialization::Ready => self
                .completion_engine()
                .map_err(|_| DetachedError::StorageUnreliable),
            AgentInitialization::Initializing => {
                Err(DetachedError::Initializing)
            }
            AgentInitialization::Quarantined
            | AgentInitialization::Unavailable => {
                Err(DetachedError::StorageUnreliable)
            }
        }
    }

    fn check_detached_intake(&self) -> Result<(), DetachedError> {
        if self.inner.state.accepting_runs.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(DetachedError::ShuttingDown)
        }
    }
}

fn validate_identity(
    id: &str,
    execution: &ExecutionMetadata,
) -> Result<(), DetachedError> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_')
        })
        || has_reserved_device_component(Path::new(id))
        || !execution.is_valid()
        || !matches!(execution.source, ExecutionSource::Automation { .. })
    {
        return Err(DetachedError::InvalidBinding);
    }
    Ok(())
}

fn attempt_record(
    engine: &Engine,
    attempt: &DetachedAttempt,
) -> Result<Option<SessionRecord>, DetachedError> {
    let Some(record) = engine.session(&attempt.session_id) else {
        return Ok(None);
    };
    let submission_id = attempt.submission_id();
    let other_submission =
        record.messages.iter().any(|msg| {
            msg.role == Role::User
                && msg.source == flowy_agent_core::MessageSource::User
                && msg.id != submission_id
        }) || record
            .turn_completions
            .iter()
            .any(|evidence| evidence.submission_id != submission_id)
            || record
                .delivery_commits
                .iter()
                .any(|commit| commit.submission_id != submission_id)
            || record.run_budget_checkpoints.keys().any(
                |checkpoint_submission| checkpoint_submission != &submission_id,
            );
    if record.execution != attempt.execution || other_submission {
        return Err(DetachedError::BindingConflict);
    }
    Ok(Some(record))
}

fn has_attempt_history(record: &SessionRecord, submission_id: &str) -> bool {
    record.messages.iter().any(|message| {
        message.role == Role::User
            && message.source == flowy_agent_core::MessageSource::User
            && message.id == submission_id
    }) || record
        .turn_completions
        .iter()
        .any(|item| item.submission_id == submission_id)
        || record
            .delivery_commits
            .iter()
            .any(|item| item.submission_id == submission_id)
        || record.run_budget_checkpoints.contains_key(submission_id)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, DetachedError> {
    if !path.is_absolute() {
        return Err(DetachedError::WorkspaceUnavailable);
    }
    let directory = path
        .canonicalize()
        .map_err(|_| DetachedError::WorkspaceUnavailable)?;
    if !directory.is_dir() {
        return Err(DetachedError::WorkspaceUnavailable);
    }
    Ok(directory)
}

/// Create/verify individual path components so a redirected workspace or
/// output directory cannot resolve through an unrelated directory.
fn private_child(
    root: &Path,
    name: &str,
    create: bool,
) -> Result<PathBuf, DetachedError> {
    let path = root.join(name);
    if create
        && let Err(error) = fs::create_dir(&path)
        && error.kind() != io::ErrorKind::AlreadyExists
    {
        return Err(DetachedError::WorkspaceUnavailable);
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| DetachedError::WorkspaceUnavailable)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || is_windows_reparse_point(&metadata)
    {
        return Err(DetachedError::WorkspaceUnavailable);
    }
    Ok(path)
}

fn session_home_workspace(
    root: &Path,
    id: &str,
    create: bool,
) -> Result<PathBuf, DetachedError> {
    let sessions = private_child(root, "sessions", create)?;
    let workspaces = private_child(&sessions, "workspaces", create)?;
    private_child(&workspaces, id, create)
}

fn checked_workspace(
    root: &Path,
    record: &SessionRecord,
) -> Result<PathBuf, DetachedError> {
    let stored = &record
        .workspace
        .as_ref()
        .ok_or(DetachedError::WorkspaceUnavailable)?
        .root;
    let current = match &record.origin {
        SessionOrigin::Home => {
            let root = root
                .canonicalize()
                .map_err(|_| DetachedError::WorkspaceUnavailable)?;
            session_home_workspace(&root, &record.id, false)?
        }
        SessionOrigin::Project {
            project_id: Some(project_id),
        } if !project_id.trim().is_empty() => canonical_directory(stored)?,
        _ => return Err(DetachedError::InvalidBinding),
    };
    if &current != stored {
        return Err(DetachedError::BindingConflict);
    }
    Ok(current)
}

fn scope_tools(
    config: &EngineConfig,
    allowed: &BTreeSet<String>,
) -> Result<(), DetachedError> {
    let available = config.registry.list();
    if allowed.iter().any(|name| {
        matches!(name.as_str(), "bash" | "powershell")
            || !available.contains(name)
    }) {
        return Err(DetachedError::ToolUnavailable);
    }
    for name in available {
        if !allowed.contains(&name) {
            config.registry.unregister(&name);
        }
    }
    Ok(())
}

fn configure_delivery(
    config: &mut EngineConfig,
    cwd: &Path,
    submission_id: &str,
    data_root: &Path,
    store: &ArtifactStore,
    phases: watch::Sender<DetachedPhase>,
) {
    // Admission may wait. This stage only configures the registry; the real
    // directory creation occurs after the complete execution ticket exists.
    let output_root = cwd.join("outputs").join(submission_id);
    let delivery =
        crate::delivery::DeliveryStore::new(data_root.join("delivery"));
    config
        .registry
        .register(crate::verify::definition_with_options(
            Some(delivery.clone()),
            None,
            None,
            None,
        ));
    config
        .registry
        .register(crate::verify::artifact_definition_with_options(
            Some(delivery.clone()),
            None,
            None,
            None,
        ));
    config
        .registry
        .register(crate::publish_tool::definition_with_revision(
            store.clone(),
            Some(delivery),
            None,
            None,
        ));
    config.delivery_committer = Some(Arc::new(DetachedDelivery {
        store: store.clone(),
        phases,
    }));
    config.system = format!(
        "Write deliverable files inside {} before calling publish_file. Verify .pptx with verify_deck and .docx/.xlsx with verify_artifact before publishing. The file is delivered only once the run finishes and publication is committed.",
        output_root.display()
    );
}

struct DetachedDelivery {
    store: ArtifactStore,
    phases: watch::Sender<DetachedPhase>,
}

#[async_trait::async_trait]
impl DeliveryCommitter for DetachedDelivery {
    async fn commit(&self, commit: &DeliveryCommit) -> Result<(), String> {
        self.phases.send_replace(DetachedPhase::Finalizing);
        self.store.commit(commit).await
    }
}

fn observe_attempt(
    engine: &Engine,
    attempt: &DetachedAttempt,
) -> Result<DetachedAttemptState, DetachedError> {
    // Read the fence before evidence. A worker which subsequently reaches its
    // commit point can temporarily still look running, but never interrupted.
    let active = engine.is_active(&attempt.session_id);
    let Some(record) = attempt_record(engine, attempt)? else {
        return Ok(DetachedAttemptState::MissingSession);
    };
    let submission_id = attempt.submission_id();
    let Some(evidence) =
        engine.completion_evidence(&attempt.session_id, &submission_id)
    else {
        if !record.turn_completions.is_empty()
            || !record.delivery_commits.is_empty()
        {
            return Err(DetachedError::EvidenceConflict);
        }
        return Ok(if !has_attempt_history(&record, &submission_id) {
            DetachedAttemptState::NotStarted
        } else if !active {
            DetachedAttemptState::Interrupted
        } else if engine.is_cancelling(&attempt.session_id) {
            DetachedAttemptState::Cancelling
        } else {
            DetachedAttemptState::Running
        });
    };
    let artifacts = if evidence.artifact_ids.is_empty() {
        DetachedArtifacts::NotRequired
    } else {
        let commit = engine
            .delivery_commit_for_submission(&attempt.session_id, &submission_id)
            .ok_or(DetachedError::EvidenceConflict)?;
        DetachedArtifacts::Pending(commit)
    };
    Ok(DetachedAttemptState::Terminal(Box::new(
        DetachedCompletion {
            evidence,
            artifacts,
        },
    )))
}

fn check_delivery_marker(
    mut state: DetachedAttemptState,
    store: &ArtifactStore,
) -> Result<DetachedAttemptState, DetachedError> {
    if let DetachedAttemptState::Terminal(completion) = &mut state
        && let DetachedArtifacts::Pending(commit) = &completion.artifacts
        && let Some(abandoned) =
            store.delivery_abandoned(commit).map_err(|error| {
                log_storage_io_error(
                    "read detached delivery marker",
                    store.root(),
                    &error,
                );
                DetachedError::Storage
            })?
    {
        completion.artifacts = if abandoned {
            DetachedArtifacts::Abandoned
        } else {
            DetachedArtifacts::Committed(commit.clone())
        };
    }
    Ok(state)
}

fn detached_run_error(engine: &Engine, error: RunError) -> DetachedError {
    match error {
        RunError::Busy => DetachedError::Busy,
        RunError::DuplicateSubmission => DetachedError::AlreadySubmitted,
        RunError::NotFound(_) => DetachedError::InvalidBinding,
        RunError::NoCredential | RunError::MissingBudget => {
            DetachedError::MissingConfiguration
        }
        RunError::Storage(error) => {
            log_storage_io_error(
                "detached agent execution",
                engine.root(),
                &error,
            );
            DetachedError::Storage
        }
        RunError::Loop(_) => DetachedError::Execution,
    }
}

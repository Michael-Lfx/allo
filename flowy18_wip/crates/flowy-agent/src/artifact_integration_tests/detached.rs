use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use flowy_agent_core::{
    ExecutionDenied, ExecutionGate, MessageSource, ModelRoute, Role,
    ToolCallArgs, ToolRunContext,
};
use sha2::{Digest, Sha256};

use super::{ProviderFixture, ProviderMode, Scratch, fs, thread};
use crate::{
    AgentHandle, CancelDisposition, DetachedArtifacts, DetachedAttempt,
    DetachedAttemptState, DetachedCompletion, DetachedError, DetachedPhase,
    DetachedRun, DetachedRunResult, DetachedRunSpec, ExecutionMetadata,
    ExecutionOwner, ExecutionSource, LlmConfig, SessionCreateDisposition,
    SessionOrigin, SessionSpec, SessionWorkspace, TurnExecutionOutcome,
};

mod recovery;

fn attempt(id: &str, submission: u128) -> DetachedAttempt {
    DetachedAttempt {
        session_id: id.into(),
        submission,
        execution: ExecutionMetadata {
            owner: Some(ExecutionOwner {
                authority: "https://account.example".into(),
                subject: "detached-fixture-account".into(),
            }),
            source: ExecutionSource::Automation {
                automation_id: "automation-fixture".into(),
                run_id: format!("run-{id}"),
                attempt_id: format!("attempt-{id}"),
            },
        },
    }
}

fn home_spec(attempt: &DetachedAttempt) -> SessionSpec {
    SessionSpec {
        id: attempt.session_id.clone(),
        created_at_ms: 1,
        origin: SessionOrigin::Home,
        workspace: None,
        execution: attempt.execution.clone(),
        role: crate::SessionRole::Standard,
        root_session_id: None,
        access_mode: crate::WorkAccessMode::ReadWrite,
    }
}

struct FixtureGate(String);

impl ExecutionGate for FixtureGate {
    fn model_credential(
        &self,
        _execution: &ExecutionMetadata,
        _route: &ModelRoute,
    ) -> Result<String, ExecutionDenied> {
        Ok(self.0.clone())
    }

    fn authorize_tool(
        &self,
        _context: &ToolRunContext,
        _args: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied> {
        Ok(())
    }
}

fn spec(attempt: &DetachedAttempt, mut config: LlmConfig) -> DetachedRunSpec {
    let gate = config.api_key.take().map(FixtureGate);
    let mut spec = DetachedRunSpec::new(
        attempt.clone(),
        "Complete the fixture task.".into(),
        config,
    );
    spec.execution_gate =
        gate.map(|gate| -> Arc<dyn ExecutionGate> { Arc::new(gate) });
    spec.allowed_tools.insert("read_file".into());
    spec
}

fn wait(run: DetachedRun) -> DetachedRunResult {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(15), run.wait())
                .await
                .expect("detached run exceeded fixture deadline")
                .unwrap()
        })
}

fn wait_for_request(provider: &ProviderFixture) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while provider.requests.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "provider was not called");
        thread::sleep(Duration::from_millis(5));
    }
}

fn completion(state: DetachedAttemptState) -> DetachedCompletion {
    let DetachedAttemptState::Terminal(completion) = state else {
        panic!("missing terminal evidence: {state:?}");
    };
    *completion
}

#[test]
fn detached_create_is_idempotent_and_rejects_conflicting_workspace_or_owner() {
    let root = Scratch::new();
    let handle = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let active = handle.create_home_session().unwrap();
    let generation = handle.context_generation();
    let home = attempt("detached-home", 1);
    assert_eq!(
        handle.reconcile_attempt(&home).unwrap(),
        DetachedAttemptState::MissingSession,
    );
    assert_eq!(
        handle.create_detached(home_spec(&home)).unwrap(),
        SessionCreateDisposition::Created,
    );
    assert_eq!(
        handle.create_detached(home_spec(&home)).unwrap(),
        SessionCreateDisposition::Existing,
    );
    let alias = attempt("Detached-Home", 2);
    assert_eq!(
        handle.create_detached(home_spec(&alias)),
        Err(DetachedError::InvalidBinding)
    );
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    let expected = root
        .0
        .canonicalize()
        .unwrap()
        .join("sessions/workspaces/detached-home");
    assert_eq!(
        history
            .get(&home.session_id)
            .unwrap()
            .workspace
            .as_ref()
            .unwrap()
            .root,
        expected
    );
    assert!(expected.is_dir());
    assert_eq!(
        handle.reconcile_attempt(&home).unwrap(),
        DetachedAttemptState::NotStarted
    );
    let mut wrong_owner = home_spec(&home);
    wrong_owner.execution.owner.as_mut().unwrap().subject =
        "another-account".into();
    assert_eq!(
        handle.create_detached(wrong_owner),
        Err(DetachedError::BindingConflict)
    );
    let mut wrong_home = home_spec(&home);
    wrong_home.workspace = Some(SessionWorkspace {
        root: root.0.clone(),
    });
    assert_eq!(
        handle.create_detached(wrong_home),
        Err(DetachedError::BindingConflict)
    );
    let unsafe_id = attempt("../detached-escape", 3);
    assert_eq!(
        handle.create_detached(home_spec(&unsafe_id)),
        Err(DetachedError::InvalidBinding)
    );

    let project = attempt("detached-project", 2);
    let mut project_spec = home_spec(&project);
    project_spec.origin = SessionOrigin::Project {
        project_id: Some("project-fixture".into()),
    };
    project_spec.workspace = Some(SessionWorkspace {
        root: root.0.join("project"),
    });
    assert_eq!(
        handle.create_detached(project_spec.clone()),
        Err(DetachedError::WorkspaceUnavailable)
    );
    fs::create_dir(&project_spec.workspace.as_ref().unwrap().root).unwrap();
    assert_eq!(
        handle.create_detached(project_spec.clone()).unwrap(),
        SessionCreateDisposition::Created
    );
    assert_eq!(
        handle.create_detached(project_spec.clone()).unwrap(),
        SessionCreateDisposition::Existing
    );
    fs::remove_dir(&project_spec.workspace.unwrap().root).unwrap();
    let provider = ProviderFixture::start_delayed_text(5, "unexpected");
    assert_eq!(
        handle
            .start_detached(spec(&project, provider.config()))
            .unwrap_err(),
        DetachedError::WorkspaceUnavailable
    );
    assert!(provider.requests.lock().unwrap().is_empty());
    assert_eq!(handle.active_session_id().as_deref(), Some(active.as_str()));
    assert_eq!(handle.context_generation(), generation);
    assert_eq!(handle.list_sessions().len(), 1);
}

#[test]
fn detached_missing_credentials_never_use_the_foreground_model_or_echo() {
    let root = Scratch::new();
    let provider = ProviderFixture::start_delayed_text(5, "unexpected");
    let handle = AgentHandle::with_config(root.0.clone(), provider.config());
    let attempt = attempt("detached-missing-credential", 5);
    handle.create_detached(home_spec(&attempt)).unwrap();

    let mut unusable = provider.config();
    unusable.api_key = None;
    assert_eq!(
        handle.start_detached(spec(&attempt, unusable)).unwrap_err(),
        DetachedError::MissingConfiguration
    );
    let mut shell = spec(&attempt, provider.config());
    shell
        .allowed_tools
        .insert(if cfg!(windows) { "powershell" } else { "bash" }.into());
    assert_eq!(
        handle.start_detached(shell).unwrap_err(),
        DetachedError::ToolUnavailable
    );
    assert_eq!(
        handle.reconcile_attempt(&attempt).unwrap(),
        DetachedAttemptState::NotStarted
    );
    assert!(provider.requests.lock().unwrap().is_empty());
}

#[test]
fn detached_run_and_foreground_turn_remain_independent() {
    let root = Scratch::new();
    let (foreground, release_foreground) =
        ProviderFixture::start_held_text("foreground fixture");
    let background =
        ProviderFixture::start_delayed_text(200, "background fixture");
    let handle = AgentHandle::with_config(root.0.clone(), foreground.config());
    let active = handle.create_home_session().unwrap();
    handle.submit_chat(10, "foreground prompt").unwrap();
    wait_for_request(&foreground);

    let attempt = attempt("detached-independent", 11);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let unchanged_generation = handle.context_generation();
    let mut run_spec = spec(&attempt, background.config());
    run_spec.config.model = "background-fixture-model".into();
    assert_eq!(
        handle
            .start_detached(spec(&attempt, background.config()))
            .unwrap_err(),
        DetachedError::Busy,
        "the foreground registry has an unscoped shell",
    );
    assert_eq!(
        handle.reconcile_attempt(&attempt).unwrap(),
        DetachedAttemptState::NotStarted,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let cancel = flowy_agent_core::CancelToken::new();
    release_foreground.store(true, Ordering::SeqCst);
    let run = runtime
        .block_on(handle.start_detached_waiting(run_spec, &cancel))
        .unwrap();
    let phases = run.subscribe();
    assert_eq!(handle.active_session_id().as_deref(), Some(active.as_str()));
    assert_eq!(handle.context_generation(), unchanged_generation);
    assert_eq!(handle.session_lines(&attempt.session_id).len(), 0);
    let next = handle.create_home_session().unwrap();
    let result = wait(run);
    assert_eq!(*phases.borrow(), DetachedPhase::ExecutionFinished);
    assert!(result.execution_error.is_none());
    let result = completion(result.evidence.unwrap());
    assert_eq!(result.evidence.outcome, TurnExecutionOutcome::Completed);
    assert_eq!(result.evidence.execution, attempt.execution);
    assert_eq!(result.artifacts, DetachedArtifacts::NotRequired);
    assert_eq!(handle.active_session_id().as_deref(), Some(next.as_str()));
    assert_eq!(handle.list_sessions().len(), 2);

    let requests = background.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        1,
        "detached runs must never start a title request"
    );
    assert_eq!(requests[0]["model"], "background-fixture-model");
    assert_eq!(requests[0]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(requests[0]["tools"][0]["function"]["name"], "read_file");
    drop(requests);
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    let detached = history.get(&attempt.session_id).unwrap();
    assert!(detached.messages.iter().any(|message| {
        message.role == Role::Assistant
            && message.text() == "background fixture"
    }));
    assert!(!detached.titled);
    assert_eq!(
        handle
            .start_detached(spec(&attempt, background.config()))
            .unwrap_err(),
        DetachedError::AlreadySubmitted,
    );
    let mut other = attempt.clone();
    other.submission = 12;
    assert_eq!(
        handle.cancel_session(&other),
        Err(DetachedError::BindingConflict)
    );
    drop(handle);
    let reopened =
        AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    assert_eq!(
        completion(reopened.reconcile_attempt(&attempt).unwrap()),
        result,
        "one detached completion is readable after restart without a provider",
    );
}

#[test]
fn detached_cancel_addresses_only_the_persisted_submission() {
    let root = Scratch::new();
    let provider =
        ProviderFixture::start_delayed_text(1_200, "too late to answer");
    let handle = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let active = handle.create_home_session().unwrap();
    let attempt = attempt("detached-cancellation", 24);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let run = handle
        .start_detached(spec(&attempt, provider.config()))
        .unwrap();
    wait_for_request(&provider);
    assert_eq!(
        handle.cancel_session(&attempt).unwrap(),
        CancelDisposition::Accepted
    );
    let result = completion(wait(run).evidence.unwrap());
    assert_eq!(result.evidence.outcome, TurnExecutionOutcome::Stopped);
    assert_eq!(
        handle.cancel_session(&attempt).unwrap(),
        CancelDisposition::NotRunning
    );
    assert_eq!(handle.active_session_id().as_deref(), Some(active.as_str()));
}

#[test]
fn detached_commit_retry_repairs_only_the_saved_artifact_group() {
    let root = Scratch::new();
    let provider = ProviderFixture::start_with(ProviderMode::DetachedDelivery);
    let handle = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let attempt = attempt("detached-delivery", 40);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    let output = history
        .get(&attempt.session_id)
        .unwrap()
        .workspace
        .as_ref()
        .unwrap()
        .root
        .join("outputs")
        .join(attempt.submission_id());
    *provider.output_root.lock().unwrap() = Some(output);
    let mut run_spec = spec(&attempt, provider.config());
    run_spec
        .allowed_tools
        .extend(["write_file".into(), "publish_file".into()]);

    let directory = handle.artifact_store().unwrap().root().join("deliveries");
    let marker_key =
        format!("{}\0{}", attempt.session_id, attempt.submission_id());
    let marker = directory
        .join(format!("{:x}.json", Sha256::digest(marker_key.as_bytes())));
    run_spec.execution_gate = Some(Arc::new(FailMarkerOnFinalRequest {
        credential: provider.config().api_key.unwrap(),
        marker: marker.clone(),
        requests: AtomicUsize::new(0),
    }));
    let result = wait(handle.start_detached(run_spec).unwrap());
    assert_eq!(result.execution_error, Some(DetachedError::Storage));
    assert_eq!(result.evidence, Err(DetachedError::Storage));
    fs::remove_dir(&marker).unwrap();

    let result = completion(handle.reconcile_attempt(&attempt).unwrap());
    let DetachedArtifacts::Pending(ref original) = result.artifacts else {
        panic!("missing pending artifact evidence");
    };
    assert_eq!(result.evidence.outcome, TurnExecutionOutcome::Completed);
    assert_eq!(result.evidence.artifact_ids, original.artifact_ids);
    let succeeded = handle
        .retry_delivery_commit(&attempt.session_id, &attempt.submission_id())
        .unwrap();
    assert_eq!(&succeeded, original);
    let result = completion(handle.reconcile_attempt(&attempt).unwrap());
    assert_eq!(result.artifacts, DetachedArtifacts::Committed(succeeded));
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    assert_eq!(
        history
            .get(&attempt.session_id)
            .unwrap()
            .messages
            .iter()
            .filter(|message| {
                message.role == Role::User
                    && message.source == MessageSource::User
            })
            .count(),
        1
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 3);
}

struct FailMarkerOnFinalRequest {
    credential: String,
    marker: std::path::PathBuf,
    requests: AtomicUsize,
}

impl ExecutionGate for FailMarkerOnFinalRequest {
    fn model_credential(
        &self,
        _execution: &ExecutionMetadata,
        _route: &ModelRoute,
    ) -> Result<String, ExecutionDenied> {
        if self.requests.fetch_add(1, Ordering::SeqCst) == 2 {
            // Publication is already prepared. Fail only the later marker
            // write after the final model response and durable history save.
            fs::create_dir(&self.marker).unwrap();
        }
        Ok(self.credential.clone())
    }

    fn authorize_tool(
        &self,
        _context: &ToolRunContext,
        _args: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied> {
        Ok(())
    }
}

#[test]
fn detached_observation_can_be_dropped_without_cancelling_the_owned_worker() {
    let root = Scratch::new();
    let provider =
        ProviderFixture::start_delayed_text(150, "owned background result");
    let handle = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let attempt = attempt("detached-drop-observer", 51);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let run = handle
        .start_detached(spec(&attempt, provider.config()))
        .unwrap();
    wait_for_request(&provider);
    drop(run);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = handle.reconcile_attempt(&attempt).unwrap();
        if let DetachedAttemptState::Terminal(result) = state {
            assert_eq!(
                result.evidence.outcome,
                TurnExecutionOutcome::Completed
            );
            break;
        }
        assert!(Instant::now() < deadline, "owned worker did not finish");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(handle.active_session_id().is_none());
    assert!(handle.list_sessions().is_empty());
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
}

struct RevokingToolGate {
    model_checks: AtomicUsize,
    tool_checks: AtomicUsize,
}

impl ExecutionGate for RevokingToolGate {
    fn model_credential(
        &self,
        _execution: &ExecutionMetadata,
        _route: &ModelRoute,
    ) -> Result<String, ExecutionDenied> {
        self.model_checks.fetch_add(1, Ordering::SeqCst);
        if self.tool_checks.load(Ordering::SeqCst) == 0 {
            Ok("fixture-only-token".into())
        } else {
            Err(ExecutionDenied::LeaseExpired)
        }
    }

    fn authorize_tool(
        &self,
        _context: &ToolRunContext,
        _args: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied> {
        self.tool_checks.fetch_add(1, Ordering::SeqCst);
        Err(ExecutionDenied::LeaseExpired)
    }
}

#[test]
fn detached_tool_gate_denial_prevents_writes_and_rechecks_the_next_request() {
    let root = Scratch::new();
    let provider = ProviderFixture::start_project_write(
        "forbidden.txt",
        "must never reach the workspace",
    );
    let handle = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let attempt = attempt("detached-revoked-tool", 52);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let gate = Arc::new(RevokingToolGate {
        model_checks: AtomicUsize::new(0),
        tool_checks: AtomicUsize::new(0),
    });
    let mut run_spec = spec(&attempt, provider.config());
    run_spec.allowed_tools.insert("write_file".into());
    run_spec.execution_gate = Some(gate.clone());
    let result = wait(handle.start_detached(run_spec).unwrap());
    let evidence = completion(result.evidence.unwrap());
    assert_eq!(evidence.evidence.outcome, TurnExecutionOutcome::Failed);
    assert_eq!(gate.tool_checks.load(Ordering::SeqCst), 1);
    assert!(gate.model_checks.load(Ordering::SeqCst) >= 2);
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    let cwd = &history
        .get(&attempt.session_id)
        .unwrap()
        .workspace
        .as_ref()
        .unwrap()
        .root;
    assert!(!cwd.join("forbidden.txt").exists());
    assert_eq!(handle.artifacts().unwrap().len(), 0);
    assert!(handle.active_session_id().is_none());
}

#[test]
fn detached_model_budget_survives_restart_without_sending_a_second_request() {
    let root = Scratch::new();
    let provider = ProviderFixture::start_project_write(
        "bounded.txt",
        "one allowed write",
    );
    let handle = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let attempt = attempt("detached-model-budget", 53);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let mut run_spec = spec(&attempt, provider.config());
    run_spec.allowed_tools.insert("write_file".into());
    run_spec.budget = flowy_agent_core::RunBudget::new(60_000, 1, 20).unwrap();
    let result = completion(
        wait(handle.start_detached(run_spec).unwrap())
            .evidence
            .unwrap(),
    );
    assert_ne!(result.evidence.outcome, TurnExecutionOutcome::Completed);
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    let record = history.get(&attempt.session_id).unwrap();
    let checkpoint = record.run_budget_checkpoints[&attempt.submission_id()];
    assert_eq!(checkpoint.model_calls, 1);
    assert_eq!(checkpoint.tool_calls, 1);
    assert_eq!(
        checkpoint.stop_reason,
        Some(flowy_agent_core::BudgetStopReason::ModelCallBudget),
    );
    assert!(
        record
            .workspace
            .as_ref()
            .unwrap()
            .root
            .join("bounded.txt")
            .exists()
    );
    drop(handle);
    let reopened =
        AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    assert_eq!(
        completion(reopened.reconcile_attempt(&attempt).unwrap()),
        result,
    );
    assert_eq!(
        reopened
            .start_detached(spec(&attempt, provider.config()))
            .unwrap_err(),
        DetachedError::AlreadySubmitted,
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
}

#[test]
fn detached_deadline_stops_a_stalled_provider_and_records_its_reason() {
    let root = Scratch::new();
    let provider =
        ProviderFixture::start_delayed_text(1_000, "past the run deadline");
    let handle = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let attempt = attempt("detached-deadline", 54);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let mut run_spec = spec(&attempt, provider.config());
    run_spec.budget = flowy_agent_core::RunBudget::new(200, 20, 20).unwrap();
    let result = completion(
        wait(handle.start_detached(run_spec).unwrap())
            .evidence
            .unwrap(),
    );
    assert_ne!(result.evidence.outcome, TurnExecutionOutcome::Completed);
    assert!(provider.requests.lock().unwrap().len() <= 1);
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    let checkpoint = history
        .get(&attempt.session_id)
        .unwrap()
        .run_budget_checkpoints[&attempt.submission_id()];
    assert_eq!(checkpoint.deadline_at_ms - checkpoint.started_at_ms, 200,);
    assert_eq!(
        checkpoint.stop_reason,
        Some(flowy_agent_core::BudgetStopReason::Deadline),
    );
    assert_eq!(checkpoint.tool_calls, 0);
    assert!(handle.active_session_id().is_none());
}

#[test]
fn detached_cancelled_workspace_wait_never_records_or_dispatches_a_submission()
{
    let root = Scratch::new();
    let (foreground, release_foreground) =
        ProviderFixture::start_held_text("foreground owner");
    let background =
        ProviderFixture::start_delayed_text(1, "must never be requested");
    let handle = AgentHandle::with_config(root.0.clone(), foreground.config());
    let active = handle.create_home_session().unwrap();
    handle
        .submit_chat(55, "hold the foreground workspace")
        .unwrap();
    wait_for_request(&foreground);
    let attempt = attempt("detached-cancelled-wait", 56);
    handle.create_detached(home_spec(&attempt)).unwrap();
    let generation = handle.context_generation();
    let cancel = flowy_agent_core::CancelToken::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let mut candidate = Box::pin(handle.start_detached_waiting(
                spec(&attempt, background.config()),
                &cancel,
            ));
            std::future::poll_fn(|cx| {
                assert!(
                    std::future::Future::poll(candidate.as_mut(), cx)
                        .is_pending(),
                    "held foreground shell owns the device ticket",
                );
                std::task::Poll::Ready(())
            })
            .await;
            assert_eq!(
                handle.reconcile_attempt(&attempt).unwrap(),
                DetachedAttemptState::NotStarted,
            );
            assert_eq!(handle.context_generation(), generation);
            cancel.cancel();
            assert_eq!(candidate.await.unwrap_err(), DetachedError::Cancelled);
        });
    assert_eq!(
        handle.reconcile_attempt(&attempt).unwrap(),
        DetachedAttemptState::NotStarted,
    );
    assert!(background.requests.lock().unwrap().is_empty());
    assert_eq!(handle.active_session_id().as_deref(), Some(active.as_str()));
    let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
    let record = history.get(&attempt.session_id).unwrap();
    assert_eq!(record.messages.len(), 0);
    assert!(record.run_budget_checkpoints.is_empty());
    assert_eq!(record.turn_completions.len(), 0);
    release_foreground.store(true, Ordering::SeqCst);
    handle.prepare_shutdown_with_timeout(Duration::from_secs(5));
}

use super::*;
use crate::artifact_store::PrepareArtifact;
use flowy_agent_core::{TurnCompletionEvidence, TurnExecutionOutcome};
use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Fixture {
    handle: AgentHandle,
    root: PathBuf,
    _cleanup: RootCleanup,
}

struct RootCleanup(PathBuf);

impl Drop for RootCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "flowy-agent-completion-{}-{}",
            std::process::id(),
            next_stamp(),
        ));
        fs::create_dir_all(&root).unwrap();
        let handle = AgentHandle::with_config(root.clone(), config());
        let engine = lock(&handle.inner.engine).clone().unwrap();
        engine
            .create_session_with_metadata(
                "background".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: root.join("workspace"),
                }),
            )
            .unwrap();
        let cleanup = RootCleanup(root.clone());
        Self {
            handle,
            root,
            _cleanup: cleanup,
        }
    }

    fn engine(&self) -> Arc<Engine> {
        lock(&self.handle.inner.engine).clone().unwrap()
    }

    fn store(&self) -> ArtifactStore {
        self.handle.artifact_store().unwrap()
    }

    fn prepare_group(&self, submission: &str) -> DeliveryCommit {
        let output = self.root.join("outputs").join(submission);
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("report.md"), submission.as_bytes()).unwrap();
        let receipt = self
            .store()
            .prepare(PrepareArtifact {
                execution: ExecutionMetadata::default(),
                source_root: output,
                relative_path: "report.md".into(),
                source_relative_path: "report.md".into(),
                conversation_import: false,
                session_id: "background".into(),
                submission_id: submission.into(),
                tool_call_id: format!("call-{submission}"),
                delivery_key: "report".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        let commit = DeliveryCommit {
            execution: ExecutionMetadata::default(),
            session_id: "background".into(),
            submission_id: submission.into(),
            artifact_ids: vec![receipt.artifact_id],
            completed_successfully: true,
            committed_at_ms: 1,
            origin: SessionOrigin::Home,
            origin_title_snapshot: "Background".into(),
        };
        let engine = self.engine();
        let mut record = engine.session("background").unwrap();
        record.status = TurnState::Completed;
        record.messages.push(Message {
            id: submission.into(),
            role: Role::User,
            content: vec![ContentBlock::text("completed background request")],
            source: MessageSource::User,
            display_text: None,
        });
        record.turn_completions.push(TurnCompletionEvidence {
            session_id: commit.session_id.clone(),
            submission_id: commit.submission_id.clone(),
            outcome: TurnExecutionOutcome::Completed,
            artifact_ids: commit.artifact_ids.clone(),
            committed_at_ms: 1,
            execution: commit.execution.clone(),
        });
        record.delivery_commits.push(commit.clone());
        engine.insert_session(record).unwrap();
        commit
    }
}

fn config() -> LlmConfig {
    LlmConfig {
        api_key: None,
        ..LlmConfig::default()
    }
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(Instant::now() < deadline, "completion did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn exact_group_retry_without_live_does_not_replay_or_switch_active() {
    let fixture = Fixture::new();
    let first = fixture.prepare_group("submission-first");
    let second = fixture.prepare_group("submission-second");
    let foreground = fixture.handle.create_home_session().unwrap();
    let history_before =
        fs::read(flowy_agent_core::SessionIndex::path(&fixture.root)).unwrap();
    assert!(!lock(&fixture.handle.inner.state.live).contains_key("background"));
    assert_eq!(fixture.store().list().unwrap().len(), 0);
    let retried = fixture
        .handle
        .retry_delivery_commit("background", "submission-first")
        .unwrap();
    assert_eq!(retried, first);
    assert_eq!(fixture.handle.active_session_id(), Some(foreground));
    assert_eq!(fixture.store().list().unwrap().len(), 1);
    assert_eq!(
        fixture
            .store()
            .read_payload(&first.artifact_ids[0])
            .unwrap(),
        b"submission-first"
    );
    assert!(
        fixture
            .store()
            .get(&second.artifact_ids[0])
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .handle
            .retry_delivery_commit("background", "submission-first")
            .unwrap(),
        first
    );
    assert_eq!(
        fs::read(flowy_agent_core::SessionIndex::path(&fixture.root)).unwrap(),
        history_before
    );
    assert!(!lock(&fixture.handle.inner.state.live).contains_key("background"));
}

#[test]
fn completion_and_group_queries_reject_other_session_or_submission() {
    let fixture = Fixture::new();
    let commit = fixture.prepare_group("submission-bound");
    let before = fixture.engine().session("background").unwrap();
    for (session, submission) in
        [("other", "submission-bound"), ("background", "other")]
    {
        assert!(
            fixture
                .handle
                .completion_evidence(session, submission)
                .unwrap()
                .is_none()
        );
        assert!(
            fixture
                .handle
                .delivery_commit_for_submission(session, submission)
                .unwrap()
                .is_none()
        );
        assert!(
            fixture
                .handle
                .retry_delivery_commit(session, submission)
                .is_err()
        );
    }
    assert_eq!(
        fixture
            .handle
            .delivery_commit_for_submission("background", "submission-bound")
            .unwrap(),
        Some(commit)
    );
    assert_eq!(fixture.store().list().unwrap().len(), 0);
    assert_eq!(fixture.engine().session("background").unwrap(), before);
}

#[test]
fn failed_marker_retry_keeps_evidence_and_group_pending_until_same_retry_succeeds()
 {
    let fixture = Fixture::new();
    let commit = fixture.prepare_group("submission-marker");
    let before = fixture.engine().session("background").unwrap();
    let markers = fixture.root.join("artifacts/deliveries");
    fs::remove_dir(&markers).unwrap();
    fs::write(&markers, b"blocks marker directory").unwrap();
    assert_eq!(
        fixture
            .handle
            .retry_delivery_commit("background", "submission-marker"),
        Err("chat.status_delivery_failed")
    );
    assert_eq!(fixture.engine().session("background").unwrap(), before);
    let evidence = fixture
        .handle
        .completion_evidence("background", "submission-marker")
        .unwrap()
        .unwrap();
    assert_eq!(evidence.outcome, TurnExecutionOutcome::Completed);
    assert_eq!(evidence.artifact_ids, commit.artifact_ids);
    fs::remove_file(&markers).unwrap();
    fs::create_dir(&markers).unwrap();
    assert_eq!(fixture.store().list().unwrap().len(), 0);
    assert_eq!(
        fixture
            .handle
            .retry_delivery_commit("background", "submission-marker")
            .unwrap(),
        commit
    );
    assert_eq!(fixture.store().list().unwrap().len(), 1);
}

#[test]
fn no_file_completion_remains_queryable_without_live_and_after_restart() {
    let fixture = Fixture::new();
    let session = fixture.handle.create_home_session().unwrap();
    fixture
        .handle
        .submit_chat(42, "durable text answer")
        .unwrap();
    let submission = format!("submission-{:032x}", 42);
    wait_until(|| {
        fixture
            .handle
            .completion_evidence(&session, &submission)
            .unwrap()
            .is_some()
    });
    wait_until(|| !fixture.handle.turn_status().is_busy());
    let expected = fixture
        .handle
        .completion_evidence(&session, &submission)
        .unwrap()
        .unwrap();
    assert_eq!(expected.outcome, TurnExecutionOutcome::Completed);
    let empty: [String; 0] = [];
    assert_eq!(expected.artifact_ids.as_slice(), empty);
    assert!(
        fixture
            .handle
            .delivery_commit_for_submission(&session, &submission)
            .unwrap()
            .is_none()
    );
    lock(&fixture.handle.inner.state.live).remove(&session);
    assert_eq!(
        fixture
            .handle
            .completion_evidence(&session, &submission)
            .unwrap(),
        Some(expected.clone())
    );
    let root = fixture.root.clone();
    drop(fixture.handle);
    let history_before =
        fs::read(flowy_agent_core::SessionIndex::path(&root)).unwrap();
    // A cold host without a window restores the same durable proof and never sends.
    let reopened = AgentHandle::with_config(root.clone(), config());
    lock(&reopened.inner.state.live).remove(&session);
    assert_eq!(
        reopened.completion_evidence(&session, &submission).unwrap(),
        Some(expected)
    );
    assert_eq!(
        fs::read(flowy_agent_core::SessionIndex::path(&root)).unwrap(),
        history_before
    );
}

#[test]
fn restarted_group_retry_works_after_live_projection_is_removed() {
    let fixture = Fixture::new();
    let commit = fixture.prepare_group("submission-restart");
    let root = fixture.root.clone();
    drop(fixture.handle);
    let reopened = AgentHandle::with_config(root.clone(), config());
    lock(&reopened.inner.state.live).remove("background");
    let before = fs::read(flowy_agent_core::SessionIndex::path(&root)).unwrap();
    let saved = reopened
        .delivery_commit_for_submission("background", "submission-restart")
        .unwrap()
        .unwrap();
    assert_eq!(saved.artifact_ids, commit.artifact_ids);
    assert_eq!(
        reopened
            .retry_delivery_commit("background", "submission-restart")
            .unwrap(),
        saved
    );
    assert_eq!(reopened.artifacts().unwrap().len(), 1);
    assert_eq!(
        fs::read(flowy_agent_core::SessionIndex::path(&root)).unwrap(),
        before
    );
}

#[test]
fn startup_never_publishes_a_group_that_conflicts_with_completion_evidence() {
    let fixture = Fixture::new();
    let commit = fixture.prepare_group("submission-conflicting-proof");
    let root = fixture.root.clone();
    drop(fixture.handle);
    let mut history = flowy_agent_core::SessionIndex::load(&root).unwrap();
    let record = history
        .sessions
        .iter_mut()
        .find(|record| record.id == "background")
        .unwrap();
    record.turn_completions[0].artifact_ids =
        vec!["artifact-not-in-the-saved-group".into()];
    history.save(&root).unwrap();

    let reopened = AgentHandle::with_config(root.clone(), config());
    assert_eq!(
        reopened.initialization_state(),
        AgentInitialization::Unavailable
    );
    assert_eq!(
        reopened.artifacts().unwrap().len(),
        0,
        "startup must validate completion evidence before publishing the saved group"
    );
    assert_eq!(
        reopened.retry_delivery_commit(
            "background",
            "submission-conflicting-proof"
        ),
        Err("chat.status_delivery_failed")
    );
    assert!(
        reopened
            .completion_evidence("background", "submission-conflicting-proof")
            .unwrap()
            .is_none()
    );
    assert!(
        root.join("artifacts/staging")
            .join(&commit.session_id)
            .join(&commit.submission_id)
            .join(&commit.artifact_ids[0])
            .join("payload")
            .is_file(),
        "a rejected group remains prepared for diagnosis rather than being cleaned up"
    );
}

#[test]
fn title_refresh_never_publishes_a_group_that_conflicts_with_completion_evidence()
 {
    let fixture = Fixture::new();
    let _commit = fixture.prepare_group("submission-title-conflict");
    let engine = fixture.engine();
    let mut record = engine.session("background").unwrap();
    record.turn_completions[0].artifact_ids =
        vec!["artifact-not-in-the-saved-group".into()];
    engine.insert_session(record).unwrap();

    sync_artifact_title(&fixture.handle.inner.state, &engine, "background");
    assert_eq!(fixture.store().list().unwrap().len(), 0);
    assert!(
        fixture
            .handle
            .inner
            .state
            .storage_error_revision
            .load(Ordering::Acquire)
            > 0
    );
}

#[test]
fn startup_preserves_prepared_files_when_completion_proof_loses_its_group() {
    let fixture = Fixture::new();
    let commit = fixture.prepare_group("submission-missing-group");
    let root = fixture.root.clone();
    drop(fixture.handle);
    let mut history = flowy_agent_core::SessionIndex::load(&root).unwrap();
    let record = history
        .sessions
        .iter_mut()
        .find(|record| record.id == "background")
        .unwrap();
    record.delivery_commits.clear();
    history.save(&root).unwrap();

    let reopened = AgentHandle::with_config(root.clone(), config());
    assert!(
        root.join("artifacts/staging")
            .join(&commit.session_id)
            .join(&commit.submission_id)
            .join(&commit.artifact_ids[0])
            .join("payload")
            .is_file(),
        "invalid completion evidence must stop cleanup even when its saved group is missing"
    );
    assert_eq!(
        reopened.initialization_state(),
        AgentInitialization::Unavailable
    );
    assert_eq!(reopened.artifacts().unwrap().len(), 0);
    assert!(
        reopened
            .completion_evidence("background", "submission-missing-group")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        reopened
            .retry_delivery_commit("background", "submission-missing-group"),
        Err("chat.status_delivery_failed")
    );
}

#[test]
fn conversation_backfill_never_publishes_a_group_that_conflicts_with_completion_evidence()
 {
    let fixture = Fixture::new();
    let commit = fixture.prepare_group("submission-backfill-conflict");
    let engine = fixture.engine();
    let mut record = engine.session("background").unwrap();
    record.turn_completions[0].artifact_ids =
        vec!["artifact-not-in-the-saved-group".into()];
    engine.insert_session(record).unwrap();

    assert!(!persist_conversation_commits(
        &engine,
        &fixture.store(),
        &fixture.handle.inner.state,
        vec![commit],
        None,
    ));
    assert_eq!(fixture.store().list().unwrap().len(), 0);
    assert!(
        fixture
            .handle
            .inner
            .state
            .storage_error_revision
            .load(Ordering::Acquire)
            > 0
    );
}

#[test]
fn cancellation_reports_too_late_without_waiting_for_marker_dispatch() {
    let fixture = Fixture::new();
    let _commit = fixture.prepare_group("submission-finalizing");
    let engine = fixture.engine();
    let state = Arc::clone(&fixture.handle.inner.state);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let marker = std::thread::spawn(move || {
        let _dispatch = lock(&state.dispatch);
        engine
            .retry_delivery_commit_with(
                "background",
                "submission-finalizing",
                |_| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
            )
            .unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let agent = fixture.handle.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let cancel = std::thread::spawn(move || {
        result_tx
            .send(agent.request_cancel_typed("background"))
            .unwrap();
    });
    let result = result_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    marker.join().unwrap();
    cancel.join().unwrap();
    assert_eq!(result.unwrap(), Ok(CancelDisposition::TooLate));
    assert_eq!(
        fixture.handle.request_cancel_typed("background"),
        Ok(CancelDisposition::NotRunning)
    );
    assert_eq!(
        fixture.handle.request_cancel_typed("missing"),
        Ok(CancelDisposition::NotFound)
    );
}

#[test]
fn explicit_cancellation_without_live_records_stopped_submission() {
    let fixture = Fixture::new();
    let foreground = fixture.handle.create_home_session().unwrap();
    let engine = fixture.engine();
    let turn = engine
        .start_with_config(
            "background".into(),
            flowy_agent_core::PromptTurn::plain("cancel before dispatch"),
            engine_config(&config(), &fixture.root),
            Some(echo_stream_fn("must not publish".into())),
            Some(71),
        )
        .unwrap();
    assert!(!lock(&fixture.handle.inner.state.live).contains_key("background"));
    assert_eq!(
        fixture.handle.request_cancel_typed("background"),
        Ok(CancelDisposition::Accepted)
    );
    assert_eq!(fixture.handle.active_session_id(), Some(foreground.clone()));
    fixture.handle.inner.runtime.spawn(async move {
        let _ = turn.await;
    });
    let submission = format!("submission-{:032x}", 71);
    wait_until(|| {
        fixture
            .handle
            .completion_evidence("background", &submission)
            .unwrap()
            .is_some()
    });
    let evidence = fixture
        .handle
        .completion_evidence("background", &submission)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.outcome, TurnExecutionOutcome::Stopped);
    assert_eq!(evidence.artifact_ids.len(), 0);
    assert_eq!(fixture.handle.active_session_id(), Some(foreground));
    assert!(!lock(&fixture.handle.inner.state.live).contains_key("background"));
}

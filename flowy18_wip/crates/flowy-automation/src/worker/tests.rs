use super::*;
use crate::store::{AutomationRecord, RevisionContext, StoreMutation};
use crate::*;
use std::path::PathBuf;
use std::sync::atomic::{
    AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering,
};

struct TestRoot(PathBuf);
impl TestRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "flowy-worker-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("test root: {error}"),
            }
        }
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn caller() -> CallerContext {
    CallerContext {
        principal: PrincipalId::cloud("worker.test", 7).unwrap(),
        account_generation: AccountGeneration {
            epoch: GenerationEpoch::generate(),
            sequence: 1,
        },
        source: CallerSource::UserInterface,
        grant_refs: vec![],
    }
}
struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: self.0.load(Ordering::SeqCst),
            monotonic: Duration::ZERO,
        }
    }
}
struct Access(AtomicBool);
impl HostAccess for Access {
    fn check<'a>(
        &'a self,
        _: &'a HostAccessRequest,
    ) -> PortFuture<'a, Result<HostAccessDecision, PortError>> {
        Box::pin(async {
            Ok(if self.0.load(Ordering::SeqCst) {
                HostAccessDecision::Allowed
            } else {
                HostAccessDecision::Blocked(BlockedReason::AccountUnavailable)
            })
        })
    }
}
struct Admission;
impl AdmissionPolicy for Admission {
    fn check<'a>(
        &'a self,
        _: &'a AdmissionRequest,
    ) -> PortFuture<'a, Result<AdmissionDecision, PortError>> {
        Box::pin(async { Ok(AdmissionDecision::Allowed) })
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Complete,
    Pending,
    WaitForCancel,
    WrongSpec,
    RevokeAfterPrepare,
}
struct Preparer {
    mode: Mode,
    caller: CallerContext,
    prepares: Arc<AtomicUsize>,
    executes: Arc<AtomicUsize>,
    access: Arc<Access>,
}
impl RunPreparer for Preparer {
    fn prepare<'a>(
        &'a self,
        run: &'a RunRecord,
        revision: &'a RevisionRecord,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> PortFuture<'a, Result<Box<dyn PreparedRun>, PortError>> {
        Box::pin(async move {
            self.prepares.fetch_add(1, Ordering::SeqCst);
            if matches!(self.mode, Mode::WaitForCancel) {
                cancellation.cancelled().await;
                return Err(PortError::Unavailable);
            }
            let spec = RunSpec {
                automation_id: run.automation_id.clone(),
                run_id: if matches!(self.mode, Mode::WrongSpec) {
                    RunId::generate()
                } else {
                    run.run_id.clone()
                },
                attempt_id: AttemptId::generate(),
                revision: run.revision,
                principal: run.principal.clone(),
                account_generation: self.caller.account_generation.clone(),
                session_id: Some(
                    SessionRef::parse(format!(
                        "session-{}",
                        run.run_id.as_str()
                    ))
                    .unwrap(),
                ),
                submission_id: Some(
                    SubmissionRef::parse(format!(
                        "submission-{}",
                        run.run_id.as_str()
                    ))
                    .unwrap(),
                ),
                source_session_id: revision.context.source_session_id.clone(),
                action: revision.definition.action.clone(),
                context_summary: revision.context.summary.clone(),
                skill_refs: vec![],
                workspace: revision.definition.workspace.clone(),
                capabilities: revision.definition.capabilities.clone(),
                model: revision.definition.model.clone(),
                credential_ref: Some(CredentialRef {
                    principal: run.principal.clone(),
                    provider: "script".into(),
                    scope_ref: "test-scope".into(),
                    credential_key: "test-reference".into(),
                }),
                budget: revision.definition.budget.clone(),
                latest_start_at_ms: run.latest_start_at_ms,
            };
            if matches!(self.mode, Mode::RevokeAfterPrepare) {
                self.access.0.store(false, Ordering::SeqCst);
            }
            let prepared: Box<dyn PreparedRun> = Box::new(Prepared {
                spec,
                pending: matches!(self.mode, Mode::Pending),
                executes: self.executes.clone(),
            });
            Ok(prepared)
        })
    }
}
struct Prepared {
    spec: RunSpec,
    pending: bool,
    executes: Arc<AtomicUsize>,
}
impl PreparedRun for Prepared {
    fn spec(&self) -> &RunSpec {
        &self.spec
    }
    fn execute(
        self: Box<Self>,
        progress: Arc<dyn RunProgressSink>,
    ) -> PortFuture<'static, RunCompletion> {
        Box::pin(async move {
            self.executes.fetch_add(1, Ordering::SeqCst);
            progress
                .report(RunProgress {
                    run_id: self.spec.run_id.clone(),
                    attempt_id: self.spec.attempt_id.clone(),
                    phase: ProgressPhase::Running,
                })
                .await
                .unwrap();
            progress
                .report(RunProgress {
                    run_id: self.spec.run_id.clone(),
                    attempt_id: self.spec.attempt_id.clone(),
                    phase: ProgressPhase::Finalizing,
                })
                .await
                .unwrap();
            let commit = CommitRef {
                session_id: self.spec.session_id.clone().unwrap(),
                submission_id: self.spec.submission_id.clone().unwrap(),
            };
            RunCompletion {
                execution_outcome: ExecutionOutcome::Completed,
                history_commit: HistoryCommitState::Committed,
                artifact_state: if self.pending {
                    ArtifactState::RetryableFailed
                } else {
                    ArtifactState::Committed
                },
                commit_ref: Some(commit.clone()),
                result_ref: Some(ResultRef::SessionTurn { commit }),
                error_code: None,
                stop_reason: None,
            }
        })
    }
}

struct Fixture {
    root: TestRoot,
    store: AutomationStore,
    caller: CallerContext,
    run: RunRecord,
    clock: Arc<TestClock>,
    access: Arc<Access>,
    prepares: Arc<AtomicUsize>,
    executes: Arc<AtomicUsize>,
}
impl Fixture {
    async fn new(reminder: bool, state: DefinitionState) -> Self {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let caller = caller();
        let id = AutomationId::generate();
        let revision = Revision::new(1).unwrap();
        let definition = ManualAutomationInput {
            schedule: None,
            title: PrivateText::new("worker fixture"),
            action: if reminder {
                ActionSpec::Reminder {
                    text: PrivateText::new("reminder fixture"),
                }
            } else {
                ActionSpec::AgentTask {
                    prompt: PrivateText::new("agent fixture"),
                }
            },
            workspace: None,
            capabilities: CapabilityRequest::default(),
            model: (!reminder).then(|| ModelBinding {
                selected: ModelRef {
                    provider: "script".into(),
                    model: "fixture".into(),
                },
                allowed_fallbacks: vec![],
            }),
            budget: if reminder {
                None
            } else {
                RunBudget::new(1000, 2, 3)
            },
        };
        let run = RunRecord {
            run_id: RunId::generate(),
            automation_id: id.clone(),
            revision,
            principal: caller.principal.clone(),
            current_attempt_id: None,
            retry_of_run_id: None,
            occurrence_key: None,
            phase: RunPhase::Queued,
            outcome: None,
            latest_start_at_ms: 1000,
            source_removed: false,
            spec: None,
            created_at_ms: 10,
            updated_at_ms: 10,
        };
        store
            .commit(
                caller.principal.clone(),
                vec![
                    StoreMutation::CreateAutomation {
                        automation: AutomationRecord {
                            automation_id: id.clone(),
                            principal: caller.principal.clone(),
                            current_revision: revision,
                            state,
                            title: definition.title.clone(),
                            created_at_ms: 10,
                            updated_at_ms: 10,
                            source_removed: false,
                        },
                        revision: RevisionRecord {
                            automation_id: id,
                            principal: caller.principal.clone(),
                            revision,
                            definition,
                            context: RevisionContext::default(),
                            grant_ref: None,
                            created_at_ms: 10,
                        },
                    },
                    StoreMutation::InsertRun(run.clone()),
                ],
            )
            .await
            .unwrap();
        Self {
            root,
            store,
            caller,
            run,
            clock: Arc::new(TestClock(AtomicI64::new(100))),
            access: Arc::new(Access(AtomicBool::new(true))),
            prepares: Arc::new(AtomicUsize::new(0)),
            executes: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn worker(&self, mode: Mode) -> AutomationWorker {
        AutomationWorker::new(
            self.store.clone(),
            Arc::new(Preparer {
                mode,
                caller: self.caller.clone(),
                prepares: self.prepares.clone(),
                executes: self.executes.clone(),
                access: self.access.clone(),
            }),
            self.access.clone(),
            Arc::new(Admission),
            self.clock.clone(),
        )
    }
    async fn stored(&self) -> RunRecord {
        self.store
            .run(self.caller.principal.clone(), self.run.run_id.clone())
            .await
            .unwrap()
            .unwrap()
    }
}

#[test]
fn reminder_settles_once_without_agent_resources_and_survives_reopen() {
    runtime().block_on(async {
        let fixture = Fixture::new(true, DefinitionState::Paused).await;
        let worker = fixture.worker(Mode::Complete);
        assert!(matches!(
            worker.run_once(&fixture.caller).await.unwrap(),
            WorkerCycle::Settled { .. }
        ));
        assert_eq!(
            worker.run_once(&fixture.caller).await.unwrap(),
            WorkerCycle::Idle
        );
        assert_eq!(fixture.prepares.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.executes.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture
                .store
                .attempts(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap(),
            Vec::<crate::store::AttemptRecord>::new()
        );
        let run = fixture.stored().await;
        assert_eq!(run.outcome, Some(RunOutcome::Succeeded));
        assert!(run.current_attempt_id.is_none());
        assert_eq!(
            fixture
                .store
                .outbox(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap()
                .len(),
            2
        );
        fixture.store.shutdown().await.unwrap();
        let reopened =
            AutomationStore::open(fixture.root.0.clone(), Arc::new(()))
                .await
                .unwrap();
        assert_eq!(
            reopened
                .run(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap()
                .unwrap()
                .outcome,
            Some(RunOutcome::Succeeded)
        );
        assert_eq!(
            reopened
                .outbox(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap()
                .len(),
            2
        );
        reopened.shutdown().await.unwrap();
    });
}

#[test]
fn duplicate_workers_commit_one_attempt_and_one_delivery_per_target() {
    runtime().block_on(async {
        let fixture = Fixture::new(false, DefinitionState::Enabled).await;
        let first = fixture.worker(Mode::Complete);
        let second = fixture.worker(Mode::Complete);
        let first_caller = fixture.caller.clone();
        let a =
            tokio::spawn(async move { first.run_once(&first_caller).await });
        let b = second.run_once(&fixture.caller).await;
        a.await.unwrap().unwrap();
        b.unwrap();
        assert_eq!(fixture.executes.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture
                .store
                .attempts(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            fixture
                .store
                .outbox(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(fixture.stored().await.outcome, Some(RunOutcome::Succeeded));
        fixture.store.shutdown().await.unwrap();
    });
}

#[test]
fn cancelled_waiting_candidate_never_creates_an_attempt() {
    runtime().block_on(async {
        let fixture = Fixture::new(false, DefinitionState::Enabled).await;
        let worker = fixture.worker(Mode::WaitForCancel);
        let worker_caller = fixture.caller.clone();
        let pending =
            tokio::spawn(async move { worker.run_once(&worker_caller).await });
        while fixture.prepares.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let request = CommandEnvelope::new(
            RequestKey::parse("cancel-waiting").unwrap(),
            100,
            200,
            AutomationCommand::CancelRun {
                run_id: fixture.run.run_id.clone(),
            },
        )
        .unwrap();
        fixture
            .store
            .apply_command(
                fixture.caller.clone(),
                request,
                100,
                HostAccessDecision::Allowed,
            )
            .await
            .unwrap();
        pending.await.unwrap().unwrap();
        assert_eq!(fixture.executes.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture
                .store
                .attempts(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap(),
            Vec::<crate::store::AttemptRecord>::new()
        );
        assert_eq!(fixture.stored().await.outcome, Some(RunOutcome::Cancelled));
        fixture.store.shutdown().await.unwrap();
    });
}

#[test]
fn pending_commit_stays_finalizing_without_result_outbox_or_rerun() {
    runtime().block_on(async {
        let fixture = Fixture::new(false, DefinitionState::Enabled).await;
        let worker = fixture.worker(Mode::Pending);
        assert!(matches!(
            worker.run_once(&fixture.caller).await.unwrap(),
            WorkerCycle::Finalizing { .. }
        ));
        assert_eq!(fixture.stored().await.phase, RunPhase::Finalizing);
        assert_eq!(fixture.stored().await.outcome, None);
        assert_eq!(
            fixture
                .store
                .outbox(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap(),
            Vec::<crate::store::OutboxRecord>::new()
        );
        assert_eq!(
            worker.run_once(&fixture.caller).await.unwrap(),
            WorkerCycle::Idle
        );
        assert_eq!(fixture.executes.load(Ordering::SeqCst), 1);
        fixture.store.shutdown().await.unwrap();
    });
}

#[test]
fn account_loss_during_permit_wait_cancels_preparation_without_an_attempt() {
    runtime().block_on(async {
        let fixture = Fixture::new(false, DefinitionState::Enabled).await;
        let worker = fixture.worker(Mode::WaitForCancel);
        let caller = fixture.caller.clone();
        let pending =
            tokio::spawn(async move { worker.run_once(&caller).await });
        while fixture.prepares.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        fixture.access.0.store(false, Ordering::SeqCst);
        let result = tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result, WorkerCycle::Deferred);
        assert_eq!(fixture.executes.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture
                .store
                .attempts(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap(),
            Vec::<crate::store::AttemptRecord>::new()
        );
        assert_eq!(fixture.stored().await.phase, RunPhase::Blocked);
        assert!(fixture.stored().await.spec.is_none());
        fixture.store.shutdown().await.unwrap();
    });
}

#[test]
fn lifecycle_transitions_publish_durable_versions_and_duplicates_do_not() {
    runtime().block_on(async {
        let fixture = Fixture::new(false, DefinitionState::Enabled).await;
        let p = fixture.caller.principal.clone();
        let id = fixture.run.run_id.clone();
        assert!(
            fixture
                .store
                .command_clock(p.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            fixture
                .store
                .worker_blocked(
                    p.clone(),
                    id.clone(),
                    RunPhase::Queued,
                    crate::BlockedReason::ModelUnavailable,
                    100
                )
                .await
                .unwrap()
        );
        assert_eq!(
            fixture
                .store
                .command_clock(p.clone())
                .await
                .unwrap()
                .unwrap()
                .change_sequence,
            1
        );
        assert!(
            fixture
                .store
                .worker_blocked(
                    p.clone(),
                    id.clone(),
                    RunPhase::Blocked,
                    crate::BlockedReason::ModelUnavailable,
                    101
                )
                .await
                .unwrap()
        );
        assert_eq!(
            fixture
                .store
                .command_clock(p.clone())
                .await
                .unwrap()
                .unwrap()
                .change_sequence,
            1
        );
        assert!(
            fixture
                .store
                .worker_waiting(p.clone(), id.clone(), RunPhase::Blocked, 102)
                .await
                .unwrap()
        );
        assert_eq!(
            fixture
                .store
                .command_clock(p.clone())
                .await
                .unwrap()
                .unwrap()
                .change_sequence,
            2
        );
        assert!(
            fixture
                .store
                .worker_waiting(
                    p.clone(),
                    id.clone(),
                    RunPhase::WaitingWorkspace,
                    103
                )
                .await
                .unwrap()
        );
        assert_eq!(
            fixture
                .store
                .command_clock(p.clone())
                .await
                .unwrap()
                .unwrap()
                .change_sequence,
            2
        );
        assert!(
            fixture
                .store
                .worker_expire(p.clone(), id, RunPhase::WaitingWorkspace, 1001)
                .await
                .unwrap()
        );
        let version = fixture.store.command_clock(p).await.unwrap().unwrap();
        assert_eq!(version.change_sequence, 3);
        assert_eq!(version.request_high_water_ms, 1001);
        fixture.store.shutdown().await.unwrap();
    });
}

#[test]
fn invalid_prepared_binding_and_revocation_never_submit_agent_work() {
    runtime().block_on(async {
        for mode in [Mode::WrongSpec, Mode::RevokeAfterPrepare] {
            let fixture = Fixture::new(false, DefinitionState::Enabled).await;
            let result = fixture.worker(mode).run_once(&fixture.caller).await;
            if matches!(mode, Mode::WrongSpec) {
                assert_eq!(result, Err(PortError::InvalidRequest));
            } else {
                assert_eq!(result, Ok(WorkerCycle::Deferred));
            }
            assert_eq!(fixture.executes.load(Ordering::SeqCst), 0);
            assert_eq!(
                fixture
                    .store
                    .attempts(
                        fixture.caller.principal.clone(),
                        fixture.run.run_id.clone()
                    )
                    .await
                    .unwrap(),
                Vec::<crate::store::AttemptRecord>::new()
            );
            fixture.store.shutdown().await.unwrap();
        }
    });
}

#[test]
fn expired_and_blocked_candidates_do_not_prepare_or_create_attempts() {
    runtime().block_on(async {
        let fixture = Fixture::new(false, DefinitionState::Enabled).await;
        let worker = fixture.worker(Mode::Complete);
        fixture.access.0.store(false, Ordering::SeqCst);
        assert_eq!(
            worker.run_once(&fixture.caller).await.unwrap(),
            WorkerCycle::Deferred
        );
        assert_eq!(fixture.stored().await.phase, RunPhase::Blocked);
        fixture.clock.0.store(1001, Ordering::SeqCst);
        assert!(matches!(
            worker.run_once(&fixture.caller).await.unwrap(),
            WorkerCycle::Expired { .. }
        ));
        assert_eq!(fixture.prepares.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture
                .store
                .attempts(
                    fixture.caller.principal.clone(),
                    fixture.run.run_id.clone()
                )
                .await
                .unwrap(),
            Vec::<crate::store::AttemptRecord>::new()
        );
        assert_eq!(fixture.stored().await.outcome, Some(RunOutcome::Expired));
        fixture.store.shutdown().await.unwrap();
    });
}

#[test]
fn sqlite_inbox_is_unique_and_notification_ack_retries_only_its_target() {
    use crate::delivery::{DeliveryService, NotificationDelivery};

    struct Notifications(std::sync::Mutex<Vec<String>>);
    impl NotificationDelivery for Notifications {
        fn submit<'a>(
            &'a self,
            _: &'a CallerContext,
            item: &'a crate::store::OutboxRecord,
        ) -> PortFuture<'a, Result<(), PortError>> {
            Box::pin(async move {
                self.0.lock().unwrap().push(item.delivery_key.clone());
                Ok(())
            })
        }
    }

    runtime().block_on(async {
        let fixture = Fixture::new(true, DefinitionState::Enabled).await;
        let owner = &fixture.caller.principal;
        let run = &fixture.run.run_id;
        let worker = fixture.worker(Mode::Complete);
        worker.run_once(&fixture.caller).await.unwrap();
        let claim = fixture
            .store
            .claim_delivery(
                owner.clone(),
                crate::DeliveryTarget::SystemNotification,
                100,
            )
            .await
            .unwrap()
            .unwrap();
        assert!(
            fixture
                .store
                .claim_delivery(
                    owner.clone(),
                    crate::DeliveryTarget::SystemNotification,
                    100
                )
                .await
                .unwrap()
                .is_none()
        );
        // Simulate a restart after a platform send whose SQLite ack was lost.
        fixture.store.reset_delivery_claims().await.unwrap();
        let mut other = fixture.caller.clone();
        other.principal = PrincipalId::cloud("worker.example", 922).unwrap();
        let port = Arc::new(Notifications(std::sync::Mutex::new(vec![
            claim.delivery_key.clone(),
        ])));
        let service = DeliveryService::new(
            fixture.store.clone(),
            fixture.clock.clone(),
            fixture.access.clone(),
            Some(port.clone()),
        );
        service.pump(Some(&other)).await.unwrap();
        assert_eq!(
            fixture.store.inbox(other.principal).await.unwrap(),
            Vec::new()
        );
        assert!(
            fixture
                .store
                .outbox(owner.clone(), run.clone())
                .await
                .unwrap()
                .iter()
                .all(|r| r.state == crate::OutboxState::SuspendedAccount)
        );
        service.pump(Some(&fixture.caller)).await.unwrap();
        assert!(
            !fixture
                .store
                .finish_delivery(claim.clone(), Ok(()), 100)
                .await
                .unwrap()
        );
        service.pump(Some(&fixture.caller)).await.unwrap();
        assert_eq!(fixture.store.inbox(owner.clone()).await.unwrap().len(), 1);
        assert_eq!(
            *port.0.lock().unwrap(),
            vec![claim.delivery_key.clone(), claim.delivery_key]
        );
        assert_eq!(fixture.stored().await.outcome, Some(RunOutcome::Succeeded));
        assert_eq!(fixture.executes.load(Ordering::SeqCst), 0);
        fixture.store.shutdown().await.unwrap();
    });
}

use super::*;
use crate::store::*;
use crate::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: 200,
            monotonic: std::time::Duration::ZERO,
        }
    }
}
struct EvidenceExecutor {
    state: usize,
    retries: AtomicUsize,
}
impl EvidenceExecutor {
    fn evidence(&self, a: &AttemptRef, repaired: bool) -> AttemptEvidence {
        AttemptEvidence {
            attempt: a.clone(),
            stop_reason: None,
            submission: if self.state == 0 {
                SubmissionEvidence::Missing
            } else if self.state == 1 {
                SubmissionEvidence::Unknown
            } else {
                SubmissionEvidence::Present
            },
            execution_outcome: ExecutionOutcome::Completed,
            history_commit: if self.state < 3 {
                HistoryCommitState::Unknown
            } else {
                HistoryCommitState::Committed
            },
            artifact_state: if self.state == 3 && !repaired {
                ArtifactState::RetryableFailed
            } else {
                ArtifactState::NotRequired
            },
            commit_ref: Some(CommitRef {
                session_id: a.session_id.clone(),
                submission_id: a.submission_id.clone(),
            }),
        }
    }
}
impl RunExecutor for EvidenceExecutor {
    fn execute(
        &self,
        _: RunSpec,
        _: Arc<dyn CancellationSignal>,
        _: Arc<dyn RunProgressSink>,
    ) -> PortFuture<'_, RunCompletion> {
        panic!("recovery must never execute")
    }
    fn request_cancel<'a>(
        &'a self,
        _: &'a AttemptRef,
        _: StopReason,
    ) -> PortFuture<'a, Result<CancelReceipt, PortError>> {
        panic!("recovery does not cancel completed work")
    }
    fn reconcile_attempt<'a>(
        &'a self,
        a: &'a AttemptRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>> {
        Box::pin(async move { Ok(self.evidence(a, false)) })
    }
    fn retry_commit<'a>(
        &'a self,
        a: &'a AttemptRef,
        _: &'a CommitRef,
    ) -> PortFuture<'a, Result<AttemptEvidence, PortError>> {
        Box::pin(async move {
            self.retries.fetch_add(1, Ordering::SeqCst);
            Ok(self.evidence(a, true))
        })
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn principal() -> PrincipalId {
    PrincipalId::cloud("recovery.example", 7).unwrap()
}
fn records() -> (AutomationRecord, RevisionRecord, RunRecord, RunSpec) {
    let owner = principal();
    let id = AutomationId::generate();
    let run_id = RunId::generate();
    let revision = Revision::new(1).unwrap();
    let definition = ManualAutomationInput {
        schedule: None,
        title: PrivateText::new("task"),
        action: ActionSpec::AgentTask {
            prompt: PrivateText::new("persisted task"),
        },
        workspace: None,
        capabilities: CapabilityRequest::default(),
        model: Some(ModelBinding {
            selected: ModelRef {
                provider: "script".into(),
                model: "fixture".into(),
            },
            allowed_fallbacks: vec![],
        }),
        budget: RunBudget::new(1000, 2, 3),
    };
    let a = AutomationRecord {
        automation_id: id.clone(),
        principal: owner.clone(),
        current_revision: revision,
        state: DefinitionState::Enabled,
        title: definition.title.clone(),
        created_at_ms: 100,
        updated_at_ms: 100,
        source_removed: false,
    };
    let r = RevisionRecord {
        automation_id: id.clone(),
        principal: owner.clone(),
        revision,
        definition: definition.clone(),
        context: RevisionContext::default(),
        grant_ref: None,
        created_at_ms: 100,
    };
    let run = RunRecord {
        run_id: run_id.clone(),
        automation_id: id.clone(),
        revision,
        principal: owner.clone(),
        current_attempt_id: None,
        retry_of_run_id: None,
        occurrence_key: None,
        phase: RunPhase::Queued,
        outcome: None,
        latest_start_at_ms: 1000,
        source_removed: false,
        spec: None,
        created_at_ms: 100,
        updated_at_ms: 100,
    };
    let spec = RunSpec {
        automation_id: id,
        run_id,
        attempt_id: AttemptId::generate(),
        revision,
        principal: owner.clone(),
        account_generation: AccountGeneration {
            epoch: GenerationEpoch::generate(),
            sequence: 1,
        },
        session_id: Some(
            SessionRef::parse(format!(
                "session-{}",
                AutomationId::generate().as_str()
            ))
            .unwrap(),
        ),
        submission_id: Some(
            SubmissionRef::parse("durable-submission").unwrap(),
        ),
        source_session_id: None,
        action: definition.action,
        context_summary: None,
        skill_refs: vec![],
        workspace: definition.workspace,
        capabilities: definition.capabilities,
        model: definition.model,
        credential_ref: Some(CredentialRef {
            principal: owner,
            provider: "script".into(),
            scope_ref: "fixture".into(),
            credential_key: "reference-only".into(),
        }),
        budget: definition.budget,
        latest_start_at_ms: 1000,
    };
    (a, r, run, spec)
}

#[test]
fn crash_evidence_matrix_never_reexecutes_and_settles_one_outbox_per_target() {
    runtime().block_on(async {
        for state in 0..5 {
            let root = std::env::temp_dir().join(format!(
                "automation-recovery-{}",
                AutomationId::generate().as_str()
            ));
            let store = AutomationStore::open(root.clone(), Arc::new(()))
                .await
                .unwrap();
            let (a, r, run, spec) = records();
            let id = run.run_id.clone();
            store
                .commit(
                    principal(),
                    vec![
                        StoreMutation::CreateAutomation {
                            automation: a,
                            revision: r,
                        },
                        StoreMutation::InsertRun(run),
                    ],
                )
                .await
                .unwrap();
            assert!(
                store
                    .worker_start(
                        principal(),
                        id.clone(),
                        RunPhase::Queued,
                        spec,
                        100
                    )
                    .await
                    .unwrap()
            );
            store.shutdown().await.unwrap();
            // Reopen SQLite just as a fresh host does; its saved mapping is authoritative.
            let store = AutomationStore::open(root.clone(), Arc::new(()))
                .await
                .unwrap();
            let executor = Arc::new(EvidenceExecutor {
                state,
                retries: AtomicUsize::new(0),
            });
            let recovery = RecoveryService::new(
                store.clone(),
                executor.clone(),
                Arc::new(FixedClock),
            );
            let report = recovery.reconcile(principal()).await.unwrap();
            let r = store.run(principal(), id.clone()).await.unwrap().unwrap();
            if state < 3 {
                assert_eq!(report.interrupted, 1);
                assert_eq!(r.phase, RunPhase::Interrupted);
                assert_eq!(
                    store.outbox(principal(), id.clone()).await.unwrap(),
                    Vec::<crate::store::OutboxRecord>::new()
                );
            } else {
                assert_eq!(report.settled, 1);
                assert_eq!(r.outcome, Some(RunOutcome::Succeeded));
                assert_eq!(
                    store.outbox(principal(), id.clone()).await.unwrap().len(),
                    2
                );
            }
            assert_eq!(
                executor.retries.load(Ordering::SeqCst),
                usize::from(state == 3)
            );
            assert_eq!(
                recovery.reconcile(principal()).await.unwrap(),
                RecoveryReport::default()
            );
            store.shutdown().await.unwrap();
            drop(recovery);
            std::fs::remove_dir_all(root).unwrap();
        }
    });
}

#[test]
fn settlement_and_outbox_roll_back_together_then_recover_without_replay() {
    runtime().block_on(async {
        let root = std::env::temp_dir().join(format!(
            "automation-recovery-{}",
            AutomationId::generate().as_str()
        ));
        let store = AutomationStore::open(root.clone(), Arc::new(()))
            .await
            .unwrap();
        let (a, r, run, spec) = records();
        let id = run.run_id.clone();
        store
            .commit(
                principal(),
                vec![
                    StoreMutation::CreateAutomation {
                        automation: a,
                        revision: r,
                    },
                    StoreMutation::InsertRun(run),
                ],
            )
            .await
            .unwrap();
        store
            .worker_start(principal(), id.clone(), RunPhase::Queued, spec, 100)
            .await
            .unwrap();
        // Force the unique delivery identity to conflict after the terminal update.
        let p = principal();
        let key = format!("run-{}-inbox", id.as_str());
        store
            .commit(
                p.clone(),
                vec![StoreMutation::InsertOutbox(OutboxRecord {
                    principal: p,
                    delivery_key: key,
                    run_id: id.clone(),
                    target: "inbox".into(),
                    message_kind: "run_result".into(),
                    state: OutboxState::Pending,
                    payload: PrivateText::new("original"),
                    next_attempt_at_ms: 100,
                    attempt_count: 0,
                    created_at_ms: 100,
                    updated_at_ms: 100,
                })],
            )
            .await
            .unwrap();
        let recovery = RecoveryService::new(
            store.clone(),
            Arc::new(EvidenceExecutor {
                state: 4,
                retries: AtomicUsize::new(0),
            }),
            Arc::new(FixedClock),
        );
        assert_eq!(
            recovery.reconcile(principal()).await,
            Err(PortError::Storage)
        );
        assert_eq!(
            store
                .run(principal(), id.clone())
                .await
                .unwrap()
                .unwrap()
                .phase,
            RunPhase::Running
        );
        store.shutdown().await.unwrap();
        drop(recovery);
        let connection = rusqlite::Connection::open(
            root.join(crate::schema_contract::DATABASE_RELATIVE_PATH),
        )
        .unwrap();
        connection
            .execute("DELETE FROM delivery_outbox", [])
            .unwrap();
        drop(connection);
        let store = AutomationStore::open(root.clone(), Arc::new(()))
            .await
            .unwrap();
        let recovery = RecoveryService::new(
            store.clone(),
            Arc::new(EvidenceExecutor {
                state: 4,
                retries: AtomicUsize::new(0),
            }),
            Arc::new(FixedClock),
        );
        assert_eq!(recovery.reconcile(principal()).await.unwrap().settled, 1);
        assert_eq!(store.outbox(principal(), id).await.unwrap().len(), 2);
        store.shutdown().await.unwrap();
        drop(recovery);
        std::fs::remove_dir_all(root).unwrap();
    });
}

use super::*;
use crate::retention::{
    RetentionHost, RetentionLimits, RetentionService, RetentionUsage,
};
use crate::{
    AdmissionDecision, AdmissionPoint, AdmissionPolicy, AdmissionRequest,
    PortFuture,
};
use std::num::NonZeroU64;

struct IsolatedFiles;

struct FailFirstFiles {
    removes: std::sync::atomic::AtomicUsize,
}

impl RetentionHost for FailFirstFiles {
    fn usage<'a>(
        &'a self,
        _: &'a PrincipalId,
        _: Vec<AttemptRecord>,
    ) -> PortFuture<'a, Result<RetentionUsage, PortError>> {
        Box::pin(async {
            Ok(RetentionUsage {
                artifact_headroom_bytes: u64::MAX,
                artifact_headroom_items: 1,
                ..RetentionUsage::default()
            })
        })
    }

    fn protected<'a>(
        &'a self,
        _: &'a DeletionPlanRecord,
        _: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<bool, PortError>> {
        Box::pin(async { Ok(false) })
    }

    fn remove<'a>(
        &'a self,
        _: &'a DeletionPlanRecord,
        attempts: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(async move {
            assert_eq!(attempts, []);
            if self
                .removes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                == 0
            {
                Err(PortError::Unavailable)
            } else {
                Ok(())
            }
        })
    }
}

impl RetentionHost for IsolatedFiles {
    fn usage<'a>(
        &'a self,
        _: &'a PrincipalId,
        _: Vec<AttemptRecord>,
    ) -> PortFuture<'a, Result<RetentionUsage, PortError>> {
        Box::pin(async {
            Ok(RetentionUsage {
                artifact_headroom_bytes: u64::MAX,
                artifact_headroom_items: 1,
                ..RetentionUsage::default()
            })
        })
    }

    fn protected<'a>(
        &'a self,
        _: &'a DeletionPlanRecord,
        _: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<bool, PortError>> {
        Box::pin(async { Ok(false) })
    }

    fn remove<'a>(
        &'a self,
        _: &'a DeletionPlanRecord,
        attempts: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(async move {
            assert_eq!(attempts, []);
            Ok(())
        })
    }
}

#[test]
fn one_stuck_retention_candidate_does_not_block_later_runs() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let c = caller();
        let owner = &c.principal;
        for title in ["First reminder", "Second reminder"] {
            let CommandOutcome::Created {
                automation_id,
                revision: expected_revision,
            } = apply(
                &store,
                &c,
                envelope(AutomationCommand::CreateManual {
                    definition: Box::new(ManualAutomationInput {
                        title: PrivateText::new(title),
                        action: ActionSpec::Reminder {
                            text: PrivateText::new(title),
                        },
                        workspace: None,
                        capabilities: CapabilityRequest::default(),
                        model: None,
                        budget: None,
                        schedule: None,
                    }),
                }),
            )
            .await
            .unwrap()
            .outcome
            else {
                panic!("definition")
            };
            let CommandOutcome::RunQueued { run_id } = apply(
                &store,
                &c,
                envelope(AutomationCommand::RunNow {
                    automation_id,
                    expected_revision,
                }),
            )
            .await
            .unwrap()
            .outcome
            else {
                panic!("run")
            };
            let completion = RunCompletion {
                execution_outcome: ExecutionOutcome::Completed,
                artifact_state: ArtifactState::NotRequired,
                history_commit: HistoryCommitState::NotStarted,
                commit_ref: None,
                result_ref: Some(ResultRef::Reminder {
                    run_id: run_id.clone(),
                }),
                error_code: None,
                stop_reason: None,
            };
            assert!(
                store
                    .worker_complete(
                        owner.clone(),
                        run_id.clone(),
                        None,
                        completion,
                        110,
                    )
                    .await
                    .unwrap()
            );
            for target in [
                crate::DeliveryTarget::Inbox,
                crate::DeliveryTarget::SystemNotification,
            ] {
                let item = store
                    .claim_delivery(owner.clone(), target, 110)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(
                    store.finish_delivery(item, Ok(()), 110).await.unwrap()
                );
            }
        }

        assert_eq!(
            store
                .retention_candidates(owner.clone(), 120, false)
                .await
                .unwrap()
                .len(),
            2
        );
        let policy = RetentionService::new(
            store.clone(),
            Arc::new(FailFirstFiles {
                removes: std::sync::atomic::AtomicUsize::new(0),
            }),
            RetentionLimits {
                history_age_ms: NonZeroU64::new(1).unwrap(),
                summary_age_ms: NonZeroU64::new(1).unwrap(),
                receipt_age_ms: NonZeroU64::new(1).unwrap(),
                ..RetentionLimits::default()
            },
        );
        assert_eq!(
            policy.maintain(owner, 121).await,
            Err(PortError::Unavailable)
        );
        assert_eq!(
            store
                .retention_candidates(owner.clone(), 0, false)
                .await
                .unwrap()
                .len(),
            1
        );
        store.shutdown().await.unwrap();
    });
}

#[test]
fn outbox_reservation_counts_new_runs_before_any_attempt() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let c = caller();
        let (automation_id, expected_revision) = create(&store, &c).await;
        let policy = RetentionService::new(
            store.clone(),
            Arc::new(IsolatedFiles),
            RetentionLimits {
                pending_outbox_items: NonZeroU64::new(2).unwrap(),
                ..RetentionLimits::default()
            },
        );
        let request = AdmissionRequest {
            principal: c.principal.clone(),
            run_id: None,
            point: AdmissionPoint::CreateRun,
        };
        assert_eq!(
            policy.check(&request).await.unwrap(),
            AdmissionDecision::Allowed
        );
        apply(
            &store,
            &c,
            envelope(AutomationCommand::RunNow {
                automation_id,
                expected_revision,
            }),
        )
        .await
        .unwrap();
        assert!(matches!(
            policy.check(&request).await.unwrap(),
            AdmissionDecision::SuspendedStorage(_)
        ));
        store.shutdown().await.unwrap();
    });
}

#[test]
fn keep_and_pending_delivery_win_before_cleanup_and_committed_plan_survives_reopen()
 {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(())).await.unwrap();
        let c = caller();
        let owner = &c.principal;
        let CommandOutcome::Created { automation_id, revision: expected_revision } = apply(&store, &c, envelope(AutomationCommand::CreateManual {
            definition: Box::new(ManualAutomationInput {
                title: PrivateText::new("Reminder"),
                action: ActionSpec::Reminder { text: PrivateText::new("Private reminder") },
                workspace: None, capabilities: CapabilityRequest::default(),
                model: None, budget: None, schedule: None,
            }),
        })).await.unwrap().outcome else { panic!("definition") };
        let queued = envelope(AutomationCommand::RunNow { automation_id, expected_revision });
        let CommandOutcome::RunQueued { run_id } = apply(&store, &c, queued.clone()).await.unwrap().outcome else { panic!("run") };
        let completion = RunCompletion {
            execution_outcome: ExecutionOutcome::Completed,
            artifact_state: ArtifactState::NotRequired,
            history_commit: HistoryCommitState::NotStarted,
            commit_ref: None,
            result_ref: Some(ResultRef::Reminder { run_id: run_id.clone() }),
            error_code: None,
            stop_reason: None,
        };
        assert!(store.worker_complete(owner.clone(), run_id.clone(), None, completion, 110).await.unwrap());
        assert_eq!(store.retention_candidates(owner.clone(), 120, true).await.unwrap(), []);
        for target in [crate::DeliveryTarget::Inbox, crate::DeliveryTarget::SystemNotification] {
            let item = store.claim_delivery(owner.clone(), target, 110).await.unwrap().unwrap();
            assert!(store.finish_delivery(item, Ok(()), 110).await.unwrap());
        }
        let (candidate, _) = store.retention_candidates(owner.clone(), 120, false).await.unwrap().pop().unwrap();
        apply(&store, &c, envelope(AutomationCommand::SetResultKept { run_id: run_id.clone(), kept: true })).await.unwrap();
        assert_eq!(store.commit_retention_plan(candidate.clone(), 120).await.unwrap(), None);
        apply(&store, &c, envelope(AutomationCommand::SetResultKept { run_id: run_id.clone(), kept: false })).await.unwrap();
        let plan = store.commit_retention_plan(candidate, 120).await.unwrap().unwrap();
        assert!(matches!(apply(&store, &c, envelope(AutomationCommand::SetResultKept {
            run_id: run_id.clone(), kept: true,
        })).await, Err(StoreError::Conflict)));
        store.shutdown().await.unwrap();

        let store = AutomationStore::open(root.0.clone(), Arc::new(())).await.unwrap();
        let (reopened, _) = store.retention_candidates(owner.clone(), 0, false).await.unwrap().pop().unwrap();
        assert_eq!(reopened, plan);
        let policy = RetentionService::new(store.clone(), Arc::new(IsolatedFiles), RetentionLimits {
            history_age_ms: NonZeroU64::new(1).unwrap(),
            summary_age_ms: NonZeroU64::new(1).unwrap(),
            receipt_age_ms: NonZeroU64::new(1).unwrap(),
            ..RetentionLimits::default()
        });
        policy.maintain(owner, 121).await.unwrap();
        assert!(matches!(
            store.query(owner.clone(), AutomationQuery::InboxRuns).await.unwrap(),
            QuerySnapshot::History(items) if items.len() == 1 && items[0].source_removed && items[0].reminder.is_none()
        ));
        assert_eq!(
            store.outbox(owner.clone(), run_id.clone()).await.unwrap(),
            Vec::new()
        );
        policy.maintain(owner, 201).await.unwrap();
        assert_eq!(store.run(owner.clone(), run_id).await.unwrap(), None);
        assert!(matches!(apply(&store, &c, queued).await, Err(StoreError::RequestExpired)));
        store.shutdown().await.unwrap();
    });
}

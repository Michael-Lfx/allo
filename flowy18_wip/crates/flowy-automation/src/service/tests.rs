use super::*;
use crate::*;
use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};

struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: self.0.load(Ordering::SeqCst),
            monotonic: Duration::ZERO,
        }
    }
}
struct Access(AtomicU8);
impl HostAccess for Access {
    fn check<'a>(
        &'a self,
        request: &'a HostAccessRequest,
    ) -> PortFuture<'a, Result<HostAccessDecision, PortError>> {
        Box::pin(async move {
            Ok(match self.0.load(Ordering::SeqCst) {
                1 if request.operation == AccessOperation::Start => {
                    HostAccessDecision::Blocked(BlockedReason::ApprovalRequired)
                }
                2 => HostAccessDecision::Denied,
                _ => HostAccessDecision::Allowed,
            })
        })
    }
}
struct Admission(AtomicU8);
impl AdmissionPolicy for Admission {
    fn check<'a>(
        &'a self,
        _: &'a AdmissionRequest,
    ) -> PortFuture<'a, Result<AdmissionDecision, PortError>> {
        Box::pin(async move {
            Ok(if self.0.load(Ordering::SeqCst) == 0 {
                AdmissionDecision::Allowed
            } else {
                AdmissionDecision::SuspendedStorage(
                    CapacityReason::PolicyUnavailable,
                )
            })
        })
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn caller(id: i64) -> CallerContext {
    CallerContext {
        principal: PrincipalId::cloud("service.example", id).unwrap(),
        account_generation: AccountGeneration {
            epoch: GenerationEpoch::generate(),
            sequence: 1,
        },
        source: CallerSource::UserInterface,
        grant_refs: vec![],
    }
}
fn definition() -> ManualAutomationInput {
    ManualAutomationInput {
        schedule: None,
        title: PrivateText::new("Reminder"),
        action: ActionSpec::Reminder {
            text: PrivateText::new("body"),
        },
        workspace: None,
        capabilities: CapabilityRequest::default(),
        model: None,
        budget: None,
    }
}
fn envelope(key: &str, command: AutomationCommand) -> CommandEnvelope {
    CommandEnvelope::new(RequestKey::parse(key).unwrap(), 100, 1000, command)
        .unwrap()
}
async fn setup() -> (
    std::path::PathBuf,
    AutomationStore,
    CommandService,
    Arc<TestClock>,
    Arc<Access>,
    Arc<Admission>,
) {
    let root = std::env::temp_dir().join(format!(
        "automation-service-{}",
        AutomationId::generate().as_str()
    ));
    let store = AutomationStore::open(root.clone(), Arc::new(()))
        .await
        .unwrap();
    let clock = Arc::new(TestClock(AtomicI64::new(200)));
    let access = Arc::new(Access(AtomicU8::new(0)));
    let admission = Arc::new(Admission(AtomicU8::new(0)));
    let service = CommandService::new(
        store.clone(),
        clock.clone(),
        access.clone(),
        admission.clone(),
    );
    (root, store, service, clock, access, admission)
}

#[test]
fn service_concurrent_recovery_uses_one_receipt_and_queries_are_owner_scoped() {
    runtime().block_on(async {
    let (root,store,service,clock,access,admission)=setup().await;let a=caller(1);
    let request=envelope("same-command",AutomationCommand::CreateManual { definition:Box::new(definition()) });
    let other=service.clone();let a2=a.clone();let request2=request.clone();
    let task=tokio::spawn(async move { other.command(&a2,request2).await });
    let receipt=service.command(&a,request.clone()).await.unwrap();assert_eq!(task.await.unwrap().unwrap(),receipt);
    assert!(matches!(service.query(&a,AutomationQuery::List).await.unwrap(),QuerySnapshot::Automations(items) if items.len()==1));
    assert_eq!(service.query(&caller(2),AutomationQuery::List).await.unwrap(),QuerySnapshot::Automations(vec![]));
    store.shutdown().await.unwrap();let reopened=AutomationStore::open(root.clone(),Arc::new(())).await.unwrap();
    let service=CommandService::new(reopened.clone(),clock,access,admission);assert_eq!(service.command(&a,request).await.unwrap(),receipt);
    reopened.shutdown().await.unwrap();std::fs::remove_dir_all(root).unwrap();
});
}

#[test]
fn service_pending_approval_replays_and_admission_failure_has_no_run() {
    runtime().block_on(async {
    let (root,store,service,_,access,admission)=setup().await;let a=caller(1);
    let created=service.command(&a,envelope("create",AutomationCommand::CreateManual { definition:Box::new(definition()) })).await.unwrap();
    let CommandOutcome::Created { automation_id,revision }=created.outcome else { panic!("create outcome") };
    let request=envelope("run",AutomationCommand::RunNow { automation_id:automation_id.clone(),expected_revision:revision });
    access.0.store(1,Ordering::SeqCst);assert_eq!(service.command(&a,request.clone()).await.unwrap().outcome,CommandOutcome::PendingApproval);
    assert!(matches!(service.query(&a,AutomationQuery::History { automation_id:automation_id.clone() }).await.unwrap(),QuerySnapshot::History(items) if items.is_empty()));
    access.0.store(0,Ordering::SeqCst);admission.0.store(1,Ordering::SeqCst);assert_eq!(service.command(&a,request.clone()).await,Err(PortError::Unavailable));
    admission.0.store(0,Ordering::SeqCst);let queued=service.command(&a,request.clone()).await.unwrap();assert!(matches!(queued.outcome,CommandOutcome::RunQueued {..}));
    admission.0.store(1,Ordering::SeqCst);assert_eq!(service.command(&a,request).await.unwrap(),queued);
    access.0.store(2,Ordering::SeqCst);assert_eq!(service.query(&a,AutomationQuery::List).await,Err(PortError::Forbidden));
    store.shutdown().await.unwrap();std::fs::remove_dir_all(root).unwrap();
});
}

#[test]
fn run_now_approval_rebinds_revision_and_queues_the_approved_run() {
    runtime().block_on(async {
        let (root, store, service, _, access, _) = setup().await;
        let a = caller(1);
        let created = service
            .command(
                &a,
                envelope(
                    "run-now-create",
                    AutomationCommand::CreateManual {
                        definition: Box::new(definition()),
                    },
                ),
            )
            .await
            .unwrap();
        let CommandOutcome::Created {
            automation_id,
            revision,
        } = created.outcome
        else {
            panic!("create outcome")
        };
        access.0.store(1, Ordering::SeqCst);
        let request = envelope(
            "run-now-approval",
            AutomationCommand::RunNow {
                automation_id: automation_id.clone(),
                expected_revision: revision,
            },
        );
        assert_eq!(
            service.command(&a, request.clone()).await.unwrap().outcome,
            CommandOutcome::PendingApproval
        );
        let cards = store
            .pending_approvals(a.principal.clone(), 200)
            .await
            .unwrap();
        let card = cards
            .into_iter()
            .find(|card| card.envelope.request_key == request.request_key)
            .expect("run-now approval card");
        let grant = GrantId::generate();
        let approved_grant = grant.clone();
        let receipt = store
            .resolve_approval(a.clone(), card, 200, move |apply| {
                apply(Some(approved_grant)).map(Some)
            })
            .await
            .unwrap()
            .expect("approved receipt");
        let CommandOutcome::RunQueued { run_id } = receipt.outcome else {
            panic!("approved run outcome")
        };
        let QuerySnapshot::Automation(snapshot) = store
            .query(
                a.principal.clone(),
                AutomationQuery::Automation { automation_id },
            )
            .await
            .unwrap()
        else {
            panic!("automation query")
        };
        assert_eq!(snapshot.revision, Revision::new(2).unwrap());
        assert_eq!(
            snapshot.definition.capabilities.grant_ref,
            Some(grant.clone())
        );
        let QuerySnapshot::Run(run) = store
            .query(a.principal.clone(), AutomationQuery::Run { run_id })
            .await
            .unwrap()
        else {
            panic!("run query")
        };
        assert_eq!(run.revision, Revision::new(2).unwrap());
        store.shutdown().await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn subscriptions_have_independent_durable_cursors_and_recheck_identity() {
    runtime().block_on(async {
        let (root, store, service, clock, access, admission) = setup().await;
        let a = caller(1);
        let mut one = service.subscribe(&a).await.unwrap();
        let mut two = service.subscribe(&a).await.unwrap();
        let initial = one.changed().await.unwrap();
        assert_eq!(two.changed().await.unwrap(), initial);
        let other = CommandService::new(
            store.clone(),
            clock,
            access.clone(),
            admission,
        );
        other
            .command(
                &a,
                envelope(
                    "other-service",
                    AutomationCommand::CreateManual {
                        definition: Box::new(definition()),
                    },
                ),
            )
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(1), one.changed())
            .await
            .unwrap()
            .unwrap();
        let second = two.changed().await.unwrap();
        assert_eq!(first, second);
        assert!(first.sequence > initial.sequence);
        access.0.store(2, Ordering::SeqCst);
        assert_eq!(one.changed().await, Err(PortError::Forbidden));
        store.shutdown().await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn service_rejects_tampered_requests_and_expiry_survives_clock_rollback() {
    runtime().block_on(async {
        let (root, store, service, clock, _, _) = setup().await;
        let a = caller(1);
        let mut request = envelope(
            "tampered",
            AutomationCommand::CreateManual {
                definition: Box::new(definition()),
            },
        );
        request.expires_at_ms += 1;
        assert_eq!(
            service.command(&a, request).await,
            Err(PortError::InvalidRequest)
        );
        let request = envelope(
            "expired",
            AutomationCommand::CreateManual {
                definition: Box::new(definition()),
            },
        );
        clock.0.store(1500, Ordering::SeqCst);
        assert_eq!(
            service.command(&a, request.clone()).await,
            Err(PortError::RequestExpired)
        );
        clock.0.store(200, Ordering::SeqCst);
        assert_eq!(
            service.command(&a, request).await,
            Err(PortError::RequestExpired)
        );
        assert_eq!(
            service.query(&a, AutomationQuery::List).await.unwrap(),
            QuerySnapshot::Automations(vec![])
        );
        store.shutdown().await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn already_applied_receipt_expires_and_does_not_revive_after_rollback() {
    runtime().block_on(async {
    let (root,store,service,clock,_,_)=setup().await;let a=caller(1);
    let request=envelope("applied-expiry",AutomationCommand::CreateManual {definition:Box::new(definition())});
    assert!(service.command(&a,request.clone()).await.is_ok());
    clock.0.store(1500,Ordering::SeqCst);
    assert_eq!(service.command(&a,request.clone()).await,Err(PortError::RequestExpired));
    clock.0.store(200,Ordering::SeqCst);
    assert_eq!(service.command(&a,request).await,Err(PortError::RequestExpired));
    assert!(matches!(service.query(&a,AutomationQuery::List).await.unwrap(),QuerySnapshot::Automations(items) if items.len()==1));
    store.shutdown().await.unwrap();std::fs::remove_dir_all(root).unwrap();
});
}

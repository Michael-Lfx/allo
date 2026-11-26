use super::*;
use crate::schema_contract::{
    LOGICAL_REFERENCES, ReferenceEndpoint, SideStore,
};
use crate::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "binding_tests.rs"]
mod binding_tests;
#[path = "command_tests.rs"]
mod command_tests;

struct TestRoot(PathBuf);
impl TestRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "flowy-automation-store-{}-{}",
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
fn principal() -> PrincipalId {
    PrincipalId::cloud("test-authority", 7).unwrap()
}
fn complete_inventory(records: Vec<SideRecord>) -> SideStoreInventory {
    SideStoreInventory {
        completeness: [
            SideStore::AgentHistory,
            SideStore::AgentIndex,
            SideStore::ArtifactManifest,
            SideStore::GrantLedger,
            SideStore::PrivateWorkspace,
        ]
        .into_iter()
        .map(|store| (store, InventoryCompleteness::Complete))
        .collect(),
        records,
    }
}
fn pair() -> (AutomationRecord, RevisionRecord) {
    let id = AutomationId::generate();
    let owner = principal();
    let revision = Revision::new(1).unwrap();
    let definition = ManualAutomationInput {
        schedule: None,
        title: PrivateText::new("fixture task"),
        action: ActionSpec::AgentTask {
            prompt: PrivateText::new("fixture prompt"),
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
    (
        AutomationRecord {
            automation_id: id.clone(),
            principal: owner.clone(),
            current_revision: revision,
            state: DefinitionState::Enabled,
            title: definition.title.clone(),
            created_at_ms: 10,
            updated_at_ms: 10,
            source_removed: false,
        },
        RevisionRecord {
            automation_id: id,
            principal: owner,
            revision,
            definition,
            context: RevisionContext::default(),
            grant_ref: None,
            created_at_ms: 10,
        },
    )
}
fn run_record(a: &AutomationRecord) -> RunRecord {
    RunRecord {
        run_id: RunId::generate(),
        automation_id: a.automation_id.clone(),
        revision: a.current_revision,
        principal: a.principal.clone(),
        current_attempt_id: None,
        retry_of_run_id: None,
        occurrence_key: Some(format!("manual:{}", RunId::generate().as_str())),
        phase: RunPhase::Queued,
        outcome: None,
        latest_start_at_ms: 1000,
        source_removed: false,
        spec: None,
        created_at_ms: 10,
        updated_at_ms: 10,
    }
}
fn attempt(r: &RunRecord, definition: &ManualAutomationInput) -> AttemptRecord {
    AttemptRecord {
        spec: RunSpec {
            automation_id: r.automation_id.clone(),
            run_id: r.run_id.clone(),
            attempt_id: AttemptId::generate(),
            revision: r.revision,
            principal: r.principal.clone(),
            account_generation: AccountGeneration {
                epoch: GenerationEpoch::generate(),
                sequence: 1,
            },
            session_id: Some(
                SessionRef::parse(format!("session-{}", r.run_id.as_str()))
                    .unwrap(),
            ),
            submission_id: Some(
                SubmissionRef::parse("submission-fixture").unwrap(),
            ),
            source_session_id: None,
            action: definition.action.clone(),
            context_summary: None,
            skill_refs: vec![],
            workspace: definition.workspace.clone(),
            capabilities: definition.capabilities.clone(),
            model: definition.model.clone(),
            credential_ref: Some(CredentialRef {
                principal: r.principal.clone(),
                provider: "script".into(),
                scope_ref: "fixture-scope".into(),
                credential_key: "reference-only".into(),
            }),
            budget: definition.budget.clone(),
            latest_start_at_ms: r.latest_start_at_ms,
        },
        started_at_ms: 20,
        completion: None,
    }
}
fn side(store: SideStore, a: &AttemptRecord) -> SideRecord {
    SideRecord {
        store,
        automation_owned: true,
        principal: Some(a.spec.principal.clone()),
        automation_id: Some(a.spec.automation_id.clone()),
        run_id: Some(a.spec.run_id.clone()),
        attempt_id: Some(a.spec.attempt_id.clone()),
        session_id: a.spec.session_id.clone(),
        submission_id: a.spec.submission_id.clone(),
        grant_id: None,
        source_removed: false,
    }
}

#[test]
fn round_trip_all_product_records_and_side_store_audit() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let (a, v) = pair();
        let r = run_record(&a);
        let attempt = attempt(&r, &v.definition);
        let receipt = ReceiptRecord {
            principal: principal(),
            envelope: CommandEnvelope {
                request_key: RequestKey::parse("fixture-request").unwrap(),
                issued_at_ms: 10,
                expires_at_ms: 1000,
                content_digest: [7; 32],
                command: AutomationCommand::RunNow {
                    automation_id: a.automation_id.clone(),
                    expected_revision: a.current_revision,
                },
            },
            state: ReceiptState::Applied,
            outcome: Some(CommandOutcome::RunQueued {
                run_id: r.run_id.clone(),
            }),
            source_removed: false,
            created_at_ms: 10,
            updated_at_ms: 10,
        };
        let outbox = OutboxRecord {
            principal: principal(),
            delivery_key: "fixture-delivery".into(),
            run_id: r.run_id.clone(),
            target: "card".into(),
            message_kind: "result".into(),
            state: OutboxState::Pending,
            payload: PrivateText::new("fixture result"),
            next_attempt_at_ms: 20,
            attempt_count: 0,
            created_at_ms: 20,
            updated_at_ms: 20,
        };
        let plan = DeletionPlanRecord {
            deletion_plan_id: DeletionPlanId::generate(),
            principal: principal(),
            run_id: r.run_id.clone(),
            state: DeletionPlanState::Planned,
            plan: DeletionTargets {
                sessions: vec![attempt.spec.session_id.clone().unwrap()],
                artifact_receipts: vec![],
                private_workspaces: vec![],
            },
            created_at_ms: 20,
            updated_at_ms: 20,
        };
        let watermark = WatermarkRecord {
            principal: principal(),
            automation_id: a.automation_id.clone(),
            revision: a.current_revision,
            processed_through_ms: 20,
            source_removed: false,
        };
        store
            .commit(
                principal(),
                vec![
                    StoreMutation::CreateAutomation {
                        automation: a.clone(),
                        revision: v.clone(),
                    },
                    StoreMutation::InsertRun(r.clone()),
                    StoreMutation::InsertAttempt(attempt.clone()),
                    StoreMutation::SetCurrentAttempt {
                        run_id: r.run_id.clone(),
                        expected: None,
                        attempt_id: attempt.spec.attempt_id.clone(),
                    },
                    StoreMutation::InsertReceipt(receipt.clone()),
                    StoreMutation::InsertOutbox(outbox.clone()),
                    StoreMutation::InsertDeletionPlan(plan.clone()),
                    StoreMutation::SetWatermark(watermark.clone()),
                ],
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .automation(principal(), a.automation_id.clone())
                .await
                .unwrap(),
            Some(a.clone())
        );
        assert_eq!(
            store
                .revisions(principal(), a.automation_id.clone())
                .await
                .unwrap(),
            vec![v]
        );
        assert_eq!(
            store.attempts(principal(), r.run_id.clone()).await.unwrap(),
            vec![attempt.clone()]
        );
        assert_eq!(
            store
                .run(principal(), r.run_id.clone())
                .await
                .unwrap()
                .unwrap()
                .current_attempt_id,
            Some(attempt.spec.attempt_id.clone())
        );
        assert_eq!(
            store
                .receipt(principal(), receipt.envelope.request_key.clone())
                .await
                .unwrap(),
            Some(receipt)
        );
        assert_eq!(
            store.outbox(principal(), r.run_id.clone()).await.unwrap(),
            vec![outbox]
        );
        assert_eq!(
            store
                .deletion_plan(principal(), plan.deletion_plan_id.clone())
                .await
                .unwrap(),
            Some(plan)
        );
        assert_eq!(
            store
                .watermark(principal(), a.automation_id.clone())
                .await
                .unwrap(),
            Some(watermark)
        );
        let inventory = complete_inventory(
            [
                SideStore::AgentHistory,
                SideStore::AgentIndex,
                SideStore::ArtifactManifest,
                SideStore::PrivateWorkspace,
            ]
            .into_iter()
            .map(|kind| side(kind, &attempt))
            .collect(),
        );
        let report = store.audit(inventory).await.unwrap();
        assert!(report.is_valid(), "{:?}", report.findings);
        assert_eq!(report.checked_references, LOGICAL_REFERENCES.len());
        let other = PrincipalId::cloud("other-authority", 7).unwrap();
        assert!(
            store
                .automation(other, a.automation_id.clone())
                .await
                .unwrap()
                .is_none()
        );
        store.shutdown().await.unwrap();
        let reopened = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .automation(principal(), a.automation_id.clone())
                .await
                .unwrap(),
            Some(a)
        );
        reopened.shutdown().await.unwrap();
    });
}

#[test]
fn failed_parent_owner_and_revision_transactions_leave_no_partial_records() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let (a, v) = pair();
        let mut bad = run_record(&a);
        bad.revision = Revision::new(2).unwrap();
        assert!(matches!(
            store
                .commit(
                    principal(),
                    vec![
                        StoreMutation::CreateAutomation {
                            automation: a.clone(),
                            revision: v.clone()
                        },
                        StoreMutation::InsertRun(bad)
                    ]
                )
                .await,
            Err(StoreError::Reference(_))
        ));
        assert!(
            store
                .automation(principal(), a.automation_id.clone())
                .await
                .unwrap()
                .is_none()
        );
        store
            .commit(
                principal(),
                vec![StoreMutation::CreateAutomation {
                    automation: a.clone(),
                    revision: v.clone(),
                }],
            )
            .await
            .unwrap();
        let wrong = PrincipalId::cloud("other-authority", 7).unwrap();
        let mut r = run_record(&a);
        r.principal = wrong.clone();
        assert!(
            store
                .commit(wrong, vec![StoreMutation::InsertRun(r)])
                .await
                .is_err()
        );
        let missing = run_record(&pair().0);
        assert!(
            store
                .commit(principal(), vec![StoreMutation::InsertRun(missing)])
                .await
                .is_err()
        );
        let r = run_record(&a);
        store
            .commit(principal(), vec![StoreMutation::InsertRun(r.clone())])
            .await
            .unwrap();
        assert!(
            store
                .commit(principal(), vec![StoreMutation::InsertRun(r.clone())])
                .await
                .is_err()
        );
        let mut duplicate = run_record(&a);
        duplicate.occurrence_key = r.occurrence_key.clone();
        assert!(
            store
                .commit(principal(), vec![StoreMutation::InsertRun(duplicate)])
                .await
                .is_err()
        );
        let mut invalid_attempt = attempt(&r, &v.definition);
        invalid_attempt.spec.revision = Revision::new(2).unwrap();
        assert!(
            store
                .commit(
                    principal(),
                    vec![StoreMutation::InsertAttempt(invalid_attempt)]
                )
                .await
                .is_err()
        );
        assert_eq!(
            store.attempts(principal(), r.run_id).await.unwrap().len(),
            0
        );
        store.shutdown().await.unwrap();
    });
}

#[test]
fn revision_compare_and_swap_serializes_concurrent_writers() {
    let root = TestRoot::new();
    let rt = runtime();
    let store = rt
        .block_on(AutomationStore::open(root.0.clone(), Arc::new(())))
        .unwrap();
    let (a, v) = pair();
    rt.block_on(store.commit(
        principal(),
        vec![StoreMutation::CreateAutomation {
            automation: a.clone(),
            revision: v.clone(),
        }],
    ))
    .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    // Array::map eagerly starts both workers before either is joined.
    let handles = [(); 2].map(|()| {
        let store = store.clone();
        let (mut a, mut v) = (a.clone(), v.clone());
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            a.current_revision = Revision::new(2).unwrap();
            v.revision = a.current_revision;
            barrier.wait();
            runtime().block_on(store.commit(
                principal(),
                vec![StoreMutation::AdvanceRevision {
                    expected: Revision::new(1).unwrap(),
                    automation: a,
                    revision: v,
                }],
            ))
        })
    });
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Err(StoreError::Conflict)))
            .count(),
        1
    );
    assert_eq!(
        rt.block_on(store.revisions(principal(), a.automation_id))
            .unwrap()
            .len(),
        2
    );
    rt.block_on(store.shutdown()).unwrap();
}

#[test]
fn incomplete_side_inventory_and_unknown_owners_never_pass() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let incomplete = store
            .audit(SideStoreInventory::unavailable())
            .await
            .unwrap();
        assert!(!incomplete.is_valid());
        assert_eq!(incomplete.completeness, InventoryCompleteness::Unavailable);
        let (a, v) = pair();
        let r = run_record(&a);
        let attempt = attempt(&r, &v.definition);
        let mut unknown = side(SideStore::GrantLedger, &attempt);
        unknown.principal = None;
        assert!(
            !store
                .audit(complete_inventory(vec![unknown]))
                .await
                .unwrap()
                .is_valid()
        );
        let normal = SideRecord {
            automation_owned: false,
            principal: None,
            ..side(SideStore::AgentIndex, &attempt)
        };
        assert!(
            store
                .audit(complete_inventory(vec![normal]))
                .await
                .unwrap()
                .is_valid()
        );
        store.shutdown().await.unwrap();
    });
}

#[test]
fn database_thread_owns_io_and_lease_until_shutdown() {
    struct Lease(Arc<AtomicU64>);
    impl Drop for Lease {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    runtime().block_on(async {
        let root = TestRoot::new();
        let drops = Arc::new(AtomicU64::new(0));
        let store = AutomationStore::open(
            root.0.clone(),
            Arc::new(Lease(drops.clone())),
        )
        .await
        .unwrap();
        let caller = std::thread::current().id();
        let worker = store
            .call(|_| Ok(std::thread::current().id()))
            .await
            .unwrap();
        assert_ne!(caller, worker);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        store.shutdown().await.unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(matches!(
            store.call(|_| Ok(())).await,
            Err(StoreError::Unavailable)
        ));
    });
}

#[test]
fn grant_and_context_references_require_the_same_principal_even_after_source_removal()
 {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let (a, mut revision) = pair();
        let grant = GrantId::generate();
        revision.grant_ref = Some(grant.clone());
        revision.definition.capabilities.grant_ref = Some(grant.clone());
        revision.context.source_session_id =
            Some(SessionRef::parse("ordinary-source").unwrap());
        revision.context.source_removed = true;
        store
            .commit(
                principal(),
                vec![StoreMutation::CreateAutomation {
                    automation: a,
                    revision,
                }],
            )
            .await
            .unwrap();
        let source = SideRecord {
            store: SideStore::AgentHistory,
            automation_owned: false,
            principal: Some(principal()),
            automation_id: None,
            run_id: None,
            attempt_id: None,
            session_id: Some(SessionRef::parse("ordinary-source").unwrap()),
            submission_id: None,
            grant_id: None,
            source_removed: false,
        };
        let grant_record = SideRecord {
            store: SideStore::GrantLedger,
            automation_owned: true,
            principal: Some(principal()),
            automation_id: None,
            run_id: None,
            attempt_id: None,
            session_id: None,
            submission_id: None,
            grant_id: Some(grant),
            source_removed: false,
        };
        assert!(
            store
                .audit(complete_inventory(vec![
                    source.clone(),
                    grant_record.clone()
                ]))
                .await
                .unwrap()
                .is_valid()
        );
        let mut wrong_source = source.clone();
        wrong_source.principal =
            Some(PrincipalId::cloud("wrong-owner", 7).unwrap());
        assert!(
            !store
                .audit(complete_inventory(vec![
                    wrong_source,
                    grant_record.clone()
                ]))
                .await
                .unwrap()
                .is_valid()
        );
        let mut wrong_grant = grant_record;
        wrong_grant.principal =
            Some(PrincipalId::cloud("wrong-owner", 7).unwrap());
        assert!(
            !store
                .audit(complete_inventory(vec![source, wrong_grant]))
                .await
                .unwrap()
                .is_valid()
        );
        store.shutdown().await.unwrap();
    });
}

#[test]
fn every_database_reference_is_audited_against_live_rows() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let (a, v) = pair();
        let r = run_record(&a);
        let attempt = attempt(&r, &v.definition);
        store
            .commit(
                principal(),
                vec![
                    StoreMutation::CreateAutomation {
                        automation: a.clone(),
                        revision: v,
                    },
                    StoreMutation::InsertRun(r.clone()),
                    StoreMutation::InsertAttempt(attempt.clone()),
                    StoreMutation::SetCurrentAttempt {
                        run_id: r.run_id.clone(),
                        expected: None,
                        attempt_id: attempt.spec.attempt_id,
                    },
                    StoreMutation::InsertReceipt(ReceiptRecord {
                        principal: principal(),
                        envelope: CommandEnvelope {
                            request_key: RequestKey::parse("audit-fixture")
                                .unwrap(),
                            issued_at_ms: 0,
                            expires_at_ms: 100,
                            content_digest: [0; 32],
                            command: AutomationCommand::RunNow {
                                automation_id: a.automation_id.clone(),
                                expected_revision: a.current_revision,
                            },
                        },
                        state: ReceiptState::Applied,
                        outcome: Some(CommandOutcome::RunQueued {
                            run_id: r.run_id.clone(),
                        }),
                        source_removed: false,
                        created_at_ms: 0,
                        updated_at_ms: 0,
                    }),
                    StoreMutation::InsertOutbox(OutboxRecord {
                        principal: principal(),
                        delivery_key: "audit-delivery".into(),
                        run_id: r.run_id.clone(),
                        target: "card".into(),
                        message_kind: "result".into(),
                        state: OutboxState::Pending,
                        payload: PrivateText::new("fixture"),
                        next_attempt_at_ms: 0,
                        attempt_count: 0,
                        created_at_ms: 0,
                        updated_at_ms: 0,
                    }),
                    StoreMutation::InsertDeletionPlan(DeletionPlanRecord {
                        deletion_plan_id: DeletionPlanId::generate(),
                        principal: principal(),
                        run_id: r.run_id.clone(),
                        state: DeletionPlanState::Planned,
                        plan: DeletionTargets::default(),
                        created_at_ms: 0,
                        updated_at_ms: 0,
                    }),
                    StoreMutation::SetWatermark(WatermarkRecord {
                        principal: principal(),
                        automation_id: a.automation_id,
                        revision: a.current_revision,
                        processed_through_ms: 0,
                        source_removed: false,
                    }),
                ],
            )
            .await
            .unwrap();
        for (index, relation) in LOGICAL_REFERENCES.iter().enumerate() {
            if let (
                ReferenceEndpoint::Table { name, fields },
                ReferenceEndpoint::Table { .. },
            ) = (relation.child, relation.parent)
            {
                let name = name.to_owned();
                let field = fields[0].to_owned();
                // Retry and result_automation are nullable and not populated above.
                let changed = store
                    .call(move |connection| {
                        let tx = connection.transaction()?;
                        let changed = tx.execute(
                            &format!("UPDATE {name} SET {field}=?1"),
                            [AutomationId::generate().as_str()],
                        )?;
                        let found = audit::database_findings(&tx)?
                            .iter()
                            .any(|finding| finding.reference == index);
                        tx.rollback()?;
                        Ok((changed, found))
                    })
                    .await
                    .unwrap();
                assert_ne!(changed.0, 0, "fixture populated child {index}");
                assert!(changed.1, "reference {index} checked");
            }
        }
        store.shutdown().await.unwrap();
    });
}

#[test]
fn command_clock_high_water_never_regresses_and_is_per_principal() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let epoch = GenerationEpoch::generate();
        let advance = |now: i64| {
            StoreMutation::AdvanceCommandClock(CommandClockAdvance {
                principal: principal(),
                observed_now_ms: now,
                epoch: epoch.clone(),
            })
        };
        // A wall-clock rollback (200 after 500) never lowers the mark.
        for now in [500, 200, 700] {
            store.commit(principal(), vec![advance(now)]).await.unwrap();
        }
        let clock = store.command_clock(principal()).await.unwrap().unwrap();
        assert_eq!(
            (clock.request_high_water_ms, clock.change_sequence),
            (700, 3)
        );
        assert_eq!(clock.change_epoch, epoch);
        let other = PrincipalId::cloud("test-authority", 8).unwrap();
        assert_eq!(store.command_clock(other.clone()).await.unwrap(), None);
        assert!(store.commit(other, vec![advance(900)]).await.is_err());
        store.shutdown().await.unwrap();
        let reopened = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let persisted =
            reopened.command_clock(principal()).await.unwrap().unwrap();
        assert_eq!(
            (persisted.request_high_water_ms, persisted.change_sequence),
            (700, 3)
        );
        reopened.shutdown().await.unwrap();
    });
}

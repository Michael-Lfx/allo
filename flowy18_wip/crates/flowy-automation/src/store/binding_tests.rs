//! Regression coverage for persisted source and deletion-target bindings.

use super::*;

async fn persist_run(
    store: &AutomationStore,
    owner: PrincipalId,
) -> AttemptRecord {
    persist_run_with_workspace(store, owner, None).await
}

async fn persist_run_with_workspace(
    store: &AutomationStore,
    owner: PrincipalId,
    workspace: Option<WorkspaceSpec>,
) -> AttemptRecord {
    let (mut automation, mut revision) = pair();
    automation.principal = owner.clone();
    revision.principal = owner.clone();
    revision.definition.workspace = workspace;
    let run = run_record(&automation);
    let attempt = attempt(&run, &revision.definition);
    store
        .commit(
            owner,
            vec![
                StoreMutation::CreateAutomation {
                    automation,
                    revision,
                },
                StoreMutation::InsertRun(run.clone()),
                StoreMutation::InsertAttempt(attempt.clone()),
                StoreMutation::SetCurrentAttempt {
                    run_id: run.run_id,
                    expected: None,
                    attempt_id: attempt.spec.attempt_id.clone(),
                },
            ],
        )
        .await
        .unwrap();
    attempt
}

fn deletion_plan(attempt: &AttemptRecord) -> DeletionPlanRecord {
    DeletionPlanRecord {
        deletion_plan_id: DeletionPlanId::generate(),
        principal: attempt.spec.principal.clone(),
        run_id: attempt.spec.run_id.clone(),
        state: DeletionPlanState::Planned,
        plan: DeletionTargets {
            sessions: vec![attempt.spec.session_id.clone().unwrap()],
            private_workspaces: vec![attempt.spec.session_id.clone().unwrap()],
        },
        created_at_ms: 20,
        updated_at_ms: 20,
    }
}

fn inventory_for(attempts: &[AttemptRecord]) -> SideStoreInventory {
    complete_inventory(
        attempts
            .iter()
            .flat_map(|attempt| {
                [
                    SideStore::AgentHistory,
                    SideStore::AgentIndex,
                    SideStore::PrivateWorkspace,
                ]
                .into_iter()
                .map(move |kind| side(kind, attempt))
            })
            .collect(),
    )
}

async fn insert_unchecked_plan(
    store: &AutomationStore,
    plan: DeletionPlanRecord,
) {
    store
        .call(move |connection| {
            connection.execute(
                "INSERT INTO deletion_plans (deletion_plan_id,principal_authority,principal_user_id,run_id,state,plan_json,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,'planned',?5,20,20)",
                rusqlite::params![
                    plan.deletion_plan_id.as_str(),
                    plan.principal.authority(),
                    plan.principal.external_user_id(),
                    plan.run_id.as_str(),
                    super::super::repository::encode(&plan.plan)?,
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

#[test]
fn deletion_targets_from_the_matching_run_round_trip_and_audit() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let attempt = persist_run(&store, principal()).await;
        let plan = deletion_plan(&attempt);
        store
            .commit(
                principal(),
                vec![StoreMutation::InsertDeletionPlan(plan.clone())],
            )
            .await
            .unwrap();
        let saved = store
            .deletion_plan(principal(), plan.deletion_plan_id.clone())
            .await
            .unwrap();
        let report = store.audit(inventory_for(&[attempt])).await.unwrap();
        store.shutdown().await.unwrap();
        assert_eq!(saved, Some(plan));
        assert!(report.is_valid(), "{:?}", report.findings);
    });
}

#[test]
fn deletion_target_commit_rejects_other_runs_owners_and_foreground_sessions() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let own = persist_run(&store, principal()).await;
        let other_run = persist_run(&store, principal()).await;
        let other_owner = persist_run(
            &store,
            PrincipalId::cloud("foreign-authority", 8).unwrap(),
        )
        .await;
        let mut accepted = Vec::new();
        for (name, target) in [
            ("other run", other_run.spec.session_id.unwrap()),
            ("other owner", other_owner.spec.session_id.unwrap()),
            (
                "ordinary foreground",
                SessionRef::parse("ordinary-foreground-session").unwrap(),
            ),
        ] {
            for workspace in [false, true] {
                let mut plan = deletion_plan(&own);
                if workspace {
                    plan.plan.private_workspaces = vec![target.clone()];
                } else {
                    plan.plan.sessions = vec![target.clone()];
                }
                if store
                    .commit(
                        principal(),
                        vec![StoreMutation::InsertDeletionPlan(plan)],
                    )
                    .await
                    .is_ok()
                {
                    accepted.push((name, workspace));
                }
            }
        }
        store.shutdown().await.unwrap();
        let empty: [(&str, bool); 0] = [];
        assert_eq!(accepted.as_slice(), empty, "accepted foreign targets");
    });
}

#[test]
fn side_audit_rejects_a_persisted_deletion_plan_pointing_to_another_run() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let own = persist_run(&store, principal()).await;
        let other = persist_run(&store, principal()).await;
        let mut plan = deletion_plan(&own);
        plan.plan.sessions = vec![other.spec.session_id.clone().unwrap()];
        insert_unchecked_plan(&store, plan).await;
        let report = store.audit(inventory_for(&[own, other])).await.unwrap();
        store.shutdown().await.unwrap();
        assert!(!report.is_valid(), "foreign deletion target passed audit");
    });
}

#[test]
fn project_session_can_be_retained_without_deleting_its_user_workspace() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let attempt = persist_run_with_workspace(
            &store,
            principal(),
            Some(WorkspaceSpec {
                roots: vec![root.0.join("user-owned-project")],
                access: WorkspaceAccess::Write,
            }),
        )
        .await;
        let mut plan = deletion_plan(&attempt);
        let rejected = store
            .commit(
                principal(),
                vec![StoreMutation::InsertDeletionPlan(plan.clone())],
            )
            .await
            .is_err();
        plan.deletion_plan_id = DeletionPlanId::generate();
        plan.plan.private_workspaces.clear();
        store
            .commit(
                principal(),
                vec![StoreMutation::InsertDeletionPlan(plan.clone())],
            )
            .await
            .unwrap();
        let mut inventory = inventory_for(std::slice::from_ref(&attempt));
        inventory
            .records
            .retain(|record| record.store != SideStore::PrivateWorkspace);
        let allowed = store.audit(inventory).await.unwrap().is_valid();
        plan.deletion_plan_id = DeletionPlanId::generate();
        plan.plan.private_workspaces =
            vec![attempt.spec.session_id.clone().unwrap()];
        insert_unchecked_plan(&store, plan).await;
        let report = store.audit(inventory_for(&[attempt])).await.unwrap();
        store.shutdown().await.unwrap();
        assert!(rejected, "project was accepted as a managed workspace");
        assert!(
            allowed,
            "project session cleanup without its directory failed"
        );
        assert!(!report.is_valid(), "project workspace target passed audit");
    });
}

#[test]
fn attempt_commit_rejects_source_and_context_outside_its_revision() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let mut accepted = Vec::new();
        for change_source in [true, false] {
            let (automation, revision) = pair();
            let run = run_record(&automation);
            let mut attempt = attempt(&run, &revision.definition);
            if change_source {
                attempt.spec.source_session_id = Some(
                    SessionRef::parse("different-source-session").unwrap(),
                );
            } else {
                attempt.spec.context_summary =
                    Some(PrivateText::new("different revision context"));
            }
            if store
                .commit(
                    principal(),
                    vec![
                        StoreMutation::CreateAutomation {
                            automation,
                            revision,
                        },
                        StoreMutation::InsertRun(run),
                        StoreMutation::InsertAttempt(attempt),
                    ],
                )
                .await
                .is_ok()
            {
                accepted.push(change_source);
            }
        }
        store.shutdown().await.unwrap();
        let empty: [bool; 0] = [];
        assert_eq!(
            accepted.as_slice(),
            empty,
            "accepted changed revision context"
        );
    });
}

#[test]
fn matching_revision_source_and_context_commit_and_audit() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let (automation, mut revision) = pair();
        revision.context.source_session_id =
            Some(SessionRef::parse("trusted-source-session").unwrap());
        revision.context.summary = Some(PrivateText::new("saved context"));
        let mut run = run_record(&automation);
        let mut attempt = attempt(&run, &revision.definition);
        attempt.spec.source_session_id =
            revision.context.source_session_id.clone();
        attempt.spec.context_summary = revision.context.summary.clone();
        run.spec = Some(attempt.spec.clone());
        store
            .commit(
                principal(),
                vec![
                    StoreMutation::CreateAutomation {
                        automation,
                        revision: revision.clone(),
                    },
                    StoreMutation::InsertRun(run),
                    StoreMutation::InsertAttempt(attempt.clone()),
                ],
            )
            .await
            .unwrap();
        let mut inventory = inventory_for(&[attempt]);
        inventory.records.push(SideRecord {
            store: SideStore::AgentHistory,
            automation_owned: false,
            principal: Some(principal()),
            automation_id: None,
            run_id: None,
            attempt_id: None,
            session_id: revision.context.source_session_id,
            submission_id: None,
            grant_id: None,
            source_removed: false,
        });
        let report = store.audit(inventory).await.unwrap();
        store.shutdown().await.unwrap();
        assert!(report.is_valid(), "{:?}", report.findings);
    });
}

#[test]
fn audit_rejects_source_and_context_corruption_in_persisted_attempts() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let attempt = persist_run(&store, principal()).await;
        let mut missed = Vec::new();
        for change_source in [true, false] {
            let mut spec = attempt.spec.clone();
            if change_source {
                spec.source_session_id = Some(
                    SessionRef::parse("different-source-session").unwrap(),
                );
            } else {
                spec.context_summary =
                    Some(PrivateText::new("different revision context"));
            }
            store
                .call(move |connection| {
                    connection.execute(
                        "UPDATE run_attempts SET spec_json=?1 WHERE attempt_id=?2",
                        rusqlite::params![
                            super::super::repository::encode(&spec)?,
                            spec.attempt_id.as_str(),
                        ],
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
            if store
                .audit(inventory_for(std::slice::from_ref(&attempt)))
                .await
                .unwrap()
                .is_valid()
            {
                missed.push(change_source);
            }
        }
        store.shutdown().await.unwrap();
        let empty: [bool; 0] = [];
        assert_eq!(missed.as_slice(), empty, "corrupted revision context passed audit");
    });
}

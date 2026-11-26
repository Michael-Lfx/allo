use super::*;

fn caller() -> CallerContext {
    CallerContext {
        principal: principal(),
        account_generation: AccountGeneration {
            epoch: GenerationEpoch::generate(),
            sequence: 1,
        },
        source: CallerSource::UserInterface,
        grant_refs: vec![],
    }
}
fn envelope(command: AutomationCommand) -> CommandEnvelope {
    CommandEnvelope::new(
        RequestKey::parse(format!(
            "request-{}",
            AutomationId::generate().as_str()
        ))
        .unwrap(),
        100,
        200,
        command,
    )
    .unwrap()
}
async fn apply(
    store: &AutomationStore,
    caller: &CallerContext,
    e: CommandEnvelope,
) -> Result<CommandReceipt, StoreError> {
    let now = store
        .observe_command_time(caller.principal.clone(), 100)
        .await?;
    store
        .apply_command(caller.clone(), e, now, HostAccessDecision::Allowed)
        .await
}
async fn create(
    store: &AutomationStore,
    c: &CallerContext,
) -> (AutomationId, Revision) {
    let e = envelope(AutomationCommand::CreateManual {
        definition: Box::new(pair().1.definition),
    });
    let CommandOutcome::Created {
        automation_id,
        revision,
    } = apply(store, c, e).await.unwrap().outcome
    else {
        panic!("created")
    };
    (automation_id, revision)
}

#[test]
fn concurrent_replay_and_response_loss_create_one_entity_and_revision() {
    runtime().block_on(async {
    let root=TestRoot::new();let store=AutomationStore::open(root.0.clone(),Arc::new(())).await.unwrap();let c=caller();
    let e=envelope(AutomationCommand::CreateManual {definition:Box::new(pair().1.definition)});
    let mut tasks=Vec::new();
    for _ in 0..8 { let s=store.clone();let c=c.clone();let e=e.clone();tasks.push(tokio::spawn(async move {apply(&s,&c,e).await.unwrap()})); }
    let mut receipts=Vec::new();for task in tasks {receipts.push(task.await.unwrap());}
    assert!(receipts.iter().all(|r|r==&receipts[0]));
    assert!(matches!(store.query(c.principal.clone(),AutomationQuery::List).await.unwrap(),QuerySnapshot::Automations(values) if values.len()==1));
    let clock=store.command_clock(c.principal.clone()).await.unwrap().unwrap();assert_eq!(clock.change_sequence,1);
    store.shutdown().await.unwrap();
    let store=AutomationStore::open(root.0.clone(),Arc::new(())).await.unwrap();
    assert_eq!(apply(&store,&c,e.clone()).await.unwrap(),receipts[0]);
    let mut altered=e;altered.expires_at_ms+=1;assert!(apply(&store,&c,altered).await.is_err());
    store.shutdown().await.unwrap();
});
}

#[test]
fn revisions_conflict_pause_keeps_queue_and_archive_keeps_history() {
    runtime().block_on(async {
    let root=TestRoot::new();let store=AutomationStore::open(root.0.clone(),Arc::new(())).await.unwrap();let c=caller();let(id,r1)=create(&store,&c).await;
    let queued=apply(&store,&c,envelope(AutomationCommand::RunNow {automation_id:id.clone(),expected_revision:r1})).await.unwrap();let CommandOutcome::RunQueued {run_id}=queued.outcome else {panic!("queued")};
    apply(&store,&c,envelope(AutomationCommand::Pause {automation_id:id.clone(),expected_revision:r1})).await.unwrap();
    assert_eq!(store.run(c.principal.clone(),run_id.clone()).await.unwrap().unwrap().phase,RunPhase::Queued);
    let r2=Revision::new(2).unwrap();
    let mut definition=pair().1.definition;definition.title=PrivateText::new("edited");
    assert!(matches!(apply(&store,&c,envelope(AutomationCommand::UpdateManual {automation_id:id.clone(),expected_revision:r1,definition:Box::new(definition.clone())})).await,Err(StoreError::Conflict)));
    apply(&store,&c,envelope(AutomationCommand::RunNow {automation_id:id.clone(),expected_revision:r2})).await.unwrap();
    apply(&store,&c,envelope(AutomationCommand::UpdateManual {automation_id:id.clone(),expected_revision:r2,definition:Box::new(definition)})).await.unwrap();
    assert_eq!(store.run(c.principal.clone(),run_id.clone()).await.unwrap().unwrap().revision,r1);
    apply(&store,&c,envelope(AutomationCommand::Archive {automation_id:id.clone(),expected_revision:Revision::new(3).unwrap()})).await.unwrap();
    assert!(apply(&store,&c,envelope(AutomationCommand::RunNow {automation_id:id.clone(),expected_revision:Revision::new(4).unwrap()})).await.is_err());
    assert!(matches!(store.query(c.principal.clone(),AutomationQuery::History {automation_id:id.clone()}).await.unwrap(),QuerySnapshot::History(values) if values.len()==2 && values.iter().all(|r|r.outcome==Some(RunOutcome::Cancelled))));
    assert_eq!(store.revisions(c.principal.clone(),id).await.unwrap().len(),4);store.shutdown().await.unwrap();
});
}

#[test]
fn approval_can_advance_once_but_rejection_cannot_be_revived() {
    runtime().block_on(async {
    let root=TestRoot::new();let store=AutomationStore::open(root.0.clone(),Arc::new(())).await.unwrap();let c=caller();
    let e=envelope(AutomationCommand::CreateManual {definition:Box::new(pair().1.definition)});
    store.observe_command_time(c.principal.clone(),100).await.unwrap();
    let pending=store.apply_command(c.clone(),e.clone(),100,HostAccessDecision::Blocked(BlockedReason::ApprovalRequired)).await.unwrap();assert_eq!(pending.outcome,CommandOutcome::PendingApproval);
    assert!(matches!(store.query(c.principal.clone(),AutomationQuery::List).await.unwrap(),QuerySnapshot::Automations(values) if values.is_empty()));
    let approved=apply(&store,&c,e.clone()).await.unwrap();assert!(matches!(approved.outcome,CommandOutcome::Created {..}));
    assert_eq!(apply(&store,&c,e).await.unwrap(),approved);
    let rejected=envelope(AutomationCommand::CreateManual {definition:Box::new(pair().1.definition)});
    assert_eq!(store.apply_command(c.clone(),rejected.clone(),100,HostAccessDecision::Denied).await.unwrap().outcome,CommandOutcome::Rejected);
    assert_eq!(apply(&store,&c,rejected).await.unwrap().outcome,CommandOutcome::Rejected);store.shutdown().await.unwrap();
});
}

#[test]
fn expired_observation_survives_restart_receipt_purge_and_clock_rollback() {
    runtime().block_on(async {
    let root=TestRoot::new();let store=AutomationStore::open(root.0.clone(),Arc::new(())).await.unwrap();let c=caller();let e=envelope(AutomationCommand::CreateManual {definition:Box::new(pair().1.definition)});
    apply(&store, &c, e.clone()).await.unwrap();
    assert!(matches!(store.apply_command(c.clone(), e.clone(), 200, HostAccessDecision::Allowed).await, Err(StoreError::RequestExpired)));
    store.shutdown().await.unwrap();let path=root.0.join(crate::schema_contract::DATABASE_RELATIVE_PATH);let raw=Connection::open(path).unwrap();raw.execute("DELETE FROM command_receipts",[]).unwrap();drop(raw);
    let store=AutomationStore::open(root.0.clone(),Arc::new(())).await.unwrap();assert_eq!(store.observe_command_time(c.principal.clone(),100).await.unwrap(),200);
    assert!(matches!(store.apply_command(c.clone(),e,100,HostAccessDecision::Allowed).await,Err(StoreError::RequestExpired)));
    assert!(matches!(store.query(c.principal, AutomationQuery::List).await.unwrap(), QuerySnapshot::Automations(items) if items.len() == 1));
    store.shutdown().await.unwrap();
});
}

#[test]
fn guessed_ids_and_conflicting_updates_leave_no_receipt_or_partial_revision() {
    runtime().block_on(async {
    let root=TestRoot::new();let store=AutomationStore::open(root.0.clone(),Arc::new(())).await.unwrap();let c=caller();let(id,r1)=create(&store,&c).await;
    let foreign=PrincipalId::cloud("test-authority",8).unwrap();assert!(store.query(foreign.clone(),AutomationQuery::Automation {automation_id:id.clone()}).await.is_err());
    assert!(matches!(store.query(foreign,AutomationQuery::List).await.unwrap(),QuerySnapshot::Automations(values) if values.is_empty()));
    let a=envelope(AutomationCommand::Pause {automation_id:id.clone(),expected_revision:r1});let b=envelope(AutomationCommand::Archive {automation_id:id.clone(),expected_revision:r1});
    apply(&store,&c,a).await.unwrap();assert!(matches!(apply(&store,&c,b.clone()).await,Err(StoreError::Conflict)));
    assert!(store.receipt(c.principal.clone(),b.request_key).await.unwrap().is_none());assert_eq!(store.revisions(c.principal.clone(),id).await.unwrap().len(),2);store.shutdown().await.unwrap();
});
}

#[path = "command_tests/calendar.rs"]
mod calendar;
#[path = "command_tests/retention.rs"]
mod retention;

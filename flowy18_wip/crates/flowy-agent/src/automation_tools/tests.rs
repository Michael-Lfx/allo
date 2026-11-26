use super::*;
use flowy_agent_core::{
    CancelToken, ExecutionMetadata, SessionOrigin, ToolExecutionResult,
    tools::ToolDefinition,
};
use flowy_automation::{
    AccountGeneration, ActionSpec, AutomationService, CapabilityRequest,
    ChangeSubscription, GenerationEpoch, ManualAutomationInput, PortFuture,
    PrivateText,
};
use std::sync::atomic::{AtomicBool, Ordering};

struct Unavailable;
impl AutomationService for Unavailable {
    fn command<'a>(
        &'a self,
        _: &'a CallerContext,
        _: CommandEnvelope,
    ) -> PortFuture<'a, Result<CommandReceipt, PortError>> {
        Box::pin(async { Err(PortError::Unavailable) })
    }
    fn query<'a>(
        &'a self,
        _: &'a CallerContext,
        _: AutomationQuery,
    ) -> PortFuture<'a, Result<QuerySnapshot, PortError>> {
        Box::pin(async { Err(PortError::Unavailable) })
    }
    fn subscribe<'a>(
        &'a self,
        _: &'a CallerContext,
    ) -> PortFuture<'a, Result<Box<dyn ChangeSubscription>, PortError>> {
        Box::pin(async { Err(PortError::Unavailable) })
    }
}
fn context() -> ToolRunContext {
    ToolRunContext {
        signal: CancelToken::new(),
        agent_id: Some("session".into()),
        cwd: None,
        todos: None,
        submission_id: Some("submission-1".into()),
        session_origin: SessionOrigin::Home,
        execution: ExecutionMetadata::conversation(),
        output_root: None,
        artifact_delivery_available: false,
    }
}
fn args(id: &str) -> ToolCallArgs {
    ToolCallArgs {
        call_id: id.into(),
        name: "automation_create".into(),
        arguments: serde_json::json!({"title":"Reminder"}),
    }
}
fn command() -> AutomationCommand {
    AutomationCommand::CreateManual {
        definition: Box::new(ManualAutomationInput {
            schedule: None,
            title: PrivateText::new("Reminder"),
            action: ActionSpec::Reminder {
                text: PrivateText::new("body"),
            },
            workspace: None,
            capabilities: CapabilityRequest::default(),
            model: None,
            budget: None,
        }),
    }
}
fn caller_factory(live: Arc<AtomicBool>) -> AutomationCallerFactory {
    let generation = AccountGeneration {
        epoch: GenerationEpoch::generate(),
        sequence: 1,
    };
    Arc::new(move |ctx, args| {
        if !live.load(Ordering::SeqCst) {
            return Err(PortError::Forbidden);
        }
        let call = ctx.trusted_call(args).ok_or(PortError::Forbidden)?;
        Ok(CallerContext {
            principal: PrincipalId::cloud("caller.example", 7).unwrap(),
            account_generation: generation.clone(),
            source: CallerSource::TrustedSubmission {
                session_id: flowy_automation::SessionRef::parse(
                    call.session_id,
                )
                .unwrap(),
                submission_id: flowy_automation::SubmissionRef::parse(
                    call.submission_id,
                )
                .unwrap(),
                call_id: call.call_id,
            },
            grant_refs: vec![],
        })
    })
}
fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "automation-tool-{}",
        GenerationEpoch::generate().as_str()
    ))
}

#[test]
fn tool_transport_recovery_keeps_envelope_and_new_call_gets_new_request() {
    let root = root();
    let live = Arc::new(AtomicBool::new(true));
    let factory = caller_factory(live);
    let handle = AutomationHandle::new(Arc::new(Unavailable));
    let bridge = AutomationToolBridge::open(
        &root,
        handle.clone(),
        factory.clone(),
        Arc::new(()),
    )
    .unwrap();
    assert!(
        AutomationToolBridge::open(
            &root,
            handle.clone(),
            factory.clone(),
            Arc::new(())
        )
        .is_err()
    );
    let (_, first) = bridge
        .envelope(&context(), &args("call-1"), command(), 100, 200)
        .unwrap();
    let (_, again) = bridge
        .envelope(&context(), &args("call-1"), command(), 150, 250)
        .unwrap();
    assert_eq!(first, again);
    drop(bridge);
    let bridge =
        AutomationToolBridge::open(&root, handle, factory, Arc::new(()))
            .unwrap();
    assert_eq!(
        bridge
            .envelope(&context(), &args("call-1"), command(), 300, 400)
            .unwrap()
            .1,
        first
    );
    assert_ne!(
        bridge
            .envelope(&context(), &args("call-2"), command(), 300, 400)
            .unwrap()
            .1
            .request_key,
        first.request_key
    );
    let mut changed = command();
    let AutomationCommand::CreateManual { definition } = &mut changed else {
        unreachable!()
    };
    definition.title = PrivateText::new("changed");
    assert_eq!(
        bridge.envelope(&context(), &args("call-1"), changed, 300, 400),
        Err(PortError::RevisionConflict)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn forged_arguments_missing_context_and_revoked_delegation_cannot_dispatch() {
    let root = root();
    let live = Arc::new(AtomicBool::new(true));
    let bridge = AutomationToolBridge::open(
        &root,
        AutomationHandle::new(Arc::new(Unavailable)),
        caller_factory(live.clone()),
        Arc::new(()),
    )
    .unwrap();
    let registry = ToolRegistry::new();
    let executed = Arc::new(AtomicBool::new(false));
    let called = executed.clone();
    registry.register(Arc::new(ToolDefinition::new(
        "automation_create",
        "test",
        serde_json::json!({}),
        move |_, _| {
            let called = called.clone();
            Box::pin(async move {
                called.store(true, Ordering::SeqCst);
                ToolExecutionResult::success_value(serde_json::json!({}))
            })
        },
    )));
    bridge.install_context_guard(&registry);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(registry.execute(
        "call-1".into(),
        "automation_create".into(),
        serde_json::json!({"owner":{"subject":"forged"}}),
        context(),
    ));
    assert!(result.is_error());
    assert!(!executed.load(Ordering::SeqCst));
    let mut missing = context();
    missing.submission_id = None;
    assert!(
        runtime
            .block_on(registry.execute(
                "call-1".into(),
                "automation_create".into(),
                serde_json::json!({}),
                missing
            ))
            .is_error()
    );
    live.store(false, Ordering::SeqCst);
    assert!(
        runtime
            .block_on(registry.execute(
                "call-1".into(),
                "automation_create".into(),
                serde_json::json!({}),
                context()
            ))
            .is_error()
    );
    assert!(!executed.load(Ordering::SeqCst));
    live.store(true, Ordering::SeqCst);
    assert!(
        !runtime
            .block_on(registry.execute(
                "call-1".into(),
                "automation_create".into(),
                serde_json::json!({}),
                context()
            ))
            .is_error()
    );
    assert!(executed.load(Ordering::SeqCst));
    if root.exists() {
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn corrupt_request_journal_is_preserved_and_refuses_open() {
    let root = root();
    std::fs::create_dir_all(root.join("automation")).unwrap();
    let path = root.join("automation/tool-requests.json");
    std::fs::write(&path, b"not json").unwrap();
    assert!(
        AutomationToolBridge::open(
            &root,
            AutomationHandle::new(Arc::new(Unavailable)),
            caller_factory(Arc::new(AtomicBool::new(true))),
            Arc::new(())
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"not json");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_a_source_session_releases_its_request_identities() {
    let root = root();
    let principal = PrincipalId::cloud("caller.example", 7).unwrap();
    let factory = caller_factory(Arc::new(AtomicBool::new(true)));
    let bridge = AutomationToolBridge::open(
        &root,
        AutomationHandle::new(Arc::new(Unavailable)),
        factory.clone(),
        Arc::new(()),
    )
    .unwrap();
    let mut first = context();
    first.agent_id = Some("session-a".into());
    let mut second = context();
    second.agent_id = Some("session-b".into());
    bridge
        .envelope(&first, &args("call-a"), command(), 100, 1000)
        .unwrap();
    bridge
        .envelope(&second, &args("call-b"), command(), 100, 1000)
        .unwrap();
    assert_eq!(bridge.request_inventory().unwrap().len(), 2);

    bridge
        .remove_requests_for_session(&principal, "session-a")
        .unwrap();
    let inventory = bridge.request_inventory().unwrap();
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].session_id, "session-b");
    bridge
        .remove_requests_for_session(&principal, "session-a")
        .unwrap();
    assert_eq!(bridge.request_inventory().unwrap().len(), 1);
    drop(bridge);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn full_journal_keeps_old_envelope_and_refuses_new_calls() {
    let root = root();
    std::fs::create_dir_all(root.join("automation")).unwrap();
    let principal = PrincipalId::cloud("caller.example", 7).unwrap();
    let mut journal = RequestJournal {
        version: 1,
        requests: Vec::new(),
    };
    for index in 0..MAX_TOOL_REQUESTS {
        let call = format!("call-{index}");
        journal.requests.push(SavedRequest {
            principal: principal.clone(),
            session_id: "session".into(),
            submission_id: "submission-1".into(),
            call_id: call.clone(),
            envelope: CommandEnvelope::new(
                request_key(&principal, "session", "submission-1", &call)
                    .unwrap(),
                100,
                200,
                command(),
            )
            .unwrap(),
        });
    }
    let path = root.join("automation/tool-requests.json");
    crate::index::save_json(&path, &journal).unwrap();
    let before = std::fs::read(&path).unwrap();
    let bridge = AutomationToolBridge::open(
        &root,
        AutomationHandle::new(Arc::new(Unavailable)),
        caller_factory(Arc::new(AtomicBool::new(true))),
        Arc::new(()),
    )
    .unwrap();
    assert_eq!(bridge.request_inventory().unwrap().len(), MAX_TOOL_REQUESTS);
    assert_eq!(
        bridge
            .envelope(&context(), &args("call-0"), command(), 500, 600)
            .unwrap()
            .1,
        journal.requests[0].envelope
    );
    assert_eq!(
        bridge.envelope(&context(), &args("call-new"), command(), 500, 600),
        Err(PortError::Storage)
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn registered_automation_tools_use_the_durable_service_and_current_caller() {
    use flowy_automation::*;
    use serde_json::{Value, json};
    use std::time::Duration;

    struct Host;
    impl Clock for Host {
        fn now(&self) -> ClockReading {
            ClockReading {
                utc_ms: i64::try_from(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis(),
                )
                .unwrap(),
                monotonic: Duration::ZERO,
            }
        }
    }
    impl HostAccess for Host {
        fn check<'a>(
            &'a self,
            request: &'a HostAccessRequest,
        ) -> PortFuture<'a, Result<HostAccessDecision, PortError>> {
            Box::pin(async move {
                Ok(
                    if request.requested.tools.is_empty()
                        && request.requested.resources.is_empty()
                    {
                        HostAccessDecision::Allowed
                    } else {
                        HostAccessDecision::Blocked(
                            BlockedReason::ApprovalRequired,
                        )
                    },
                )
            })
        }
    }
    impl AdmissionPolicy for Host {
        fn check<'a>(
            &'a self,
            _: &'a AdmissionRequest,
        ) -> PortFuture<'a, Result<AdmissionDecision, PortError>> {
            Box::pin(async { Ok(AdmissionDecision::Allowed) })
        }
    }

    let root = root();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let store = store::AutomationStore::open(root.clone(),Arc::new(())).await.unwrap();
        let host = Arc::new(Host);
        let handle = AutomationHandle::new(Arc::new(CommandService::new(store.clone(),host.clone(),host.clone(),host)));
        let live = Arc::new(AtomicBool::new(true));
        let bridge = AutomationToolBridge::open(&root,handle,caller_factory(live.clone()),Arc::new(())).unwrap();
        let registry = ToolRegistry::new();
        bridge.register(&registry);
        let invoke = |id: &'static str,name: &'static str,value: Value| registry.execute(id.into(),format!("automation_{name}"),value,context());
        let ok = |result| match result { ToolExecutionResult::Success { value,.. } => value, other @ ToolExecutionResult::Error { .. } => panic!("unexpected tool result: {other:?}") };
        let input = json!({"definition":{"title":"Daily reminder","action":{"kind":"reminder","text":"Review notes"},"schedule":{"trigger":{"kind":"calendar","time_zone":"Asia/Singapore","local_time":"09:00:00","frequency":{"kind":"daily"}}}}});
        let first = ok(invoke("create","create",input.clone()).await);
        assert_eq!(ok(invoke("create","create",input.clone()).await),first);
        let id = &first["outcome"]["automation_id"];
        let versioned = |rev| json!({"automation_id":id,"expected_revision":rev});
        assert_eq!(ok(invoke("list","list",json!({})).await)["data"].as_array().unwrap().len(),1);
        let saved = ok(invoke("get","get",json!({"automation_id":id})).await);
        assert_eq!(saved["data"]["title"],"Daily reminder");
        assert_eq!(saved["data"]["definition"]["schedule"]["trigger"]["time_zone"],"Asia/Singapore");
        assert_eq!(saved["data"]["definition"]["schedule"]["misfire"],"skip");
        assert_eq!(saved["data"]["definition"]["schedule"]["max_lateness_ms"],3_600_000);
        assert!(saved["data"]["next_run_at_ms"].is_i64());
        let update = json!({"automation_id":id,"expected_revision":1,"definition":input["definition"]});
        assert_eq!(ok(invoke("update","update",update.clone()).await)["outcome"]["revision"],2);
        assert!(invoke("update-conflict","update",update).await.is_error());
        let run = ok(invoke("run","run_now",versioned(2)).await);
        assert_eq!(run["outcome"]["kind"],"run_queued");
        let run_id = &run["outcome"]["run_id"];
        assert_eq!(ok(invoke("history","history",json!({"automation_id":id})).await)["data"][0]["phase"],"queued");
        assert_eq!(ok(invoke("cancel","cancel_run",json!({"run_id":run_id})).await)["outcome"]["disposition"],"accepted");
        assert!(invoke("retry","retry_delivery",json!({"run_id":run_id,"target":"system_notification"})).await.is_error());
        assert_eq!(ok(invoke("pause","pause",versioned(2)).await)["outcome"]["state"],"paused");
        assert_eq!(ok(invoke("resume","resume",versioned(3)).await)["outcome"]["state"],"enabled");
        assert_eq!(ok(invoke("delete","delete",versioned(4)).await)["outcome"]["state"],"archived");
        let agent = json!({"definition":{"title":"Write a report","action":{"kind":"agent_task","prompt":"Draft a report"},"model":{"selected":{"provider":"flowy","model":"fixture"},"allowed_fallbacks":[]}}});
        assert_eq!(ok(invoke("needs-approval","create",agent).await)["outcome"]["kind"],"pending_approval");
        live.store(false,Ordering::SeqCst);
        assert!(invoke("old-generation","list",json!({})).await.is_error());
        store.shutdown().await.unwrap();
    });
    std::fs::remove_dir_all(root).unwrap();
}

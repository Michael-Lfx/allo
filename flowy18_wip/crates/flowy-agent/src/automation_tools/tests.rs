use std::time::Duration;

use flowy_automation::{
    AutomationStore, CancelToken, RunExecutor, RunFuture, RunOutcome,
    RunReport, Worker, WorkerConfig,
};

use super::*;

struct Succeeds;

impl RunExecutor for Succeeds {
    fn run<'a>(
        &'a self,
        _automation: &'a flowy_automation::Automation,
        _run_id: RunId,
        _cancel: CancelToken,
    ) -> RunFuture<'a> {
        Box::pin(async {
            RunReport {
                session_id: Some("session-1".to_owned()),
                outcome: RunOutcome::Succeeded,
            }
        })
    }
}

fn registry(writable: bool) -> (ToolRegistry, Arc<AutomationService>) {
    let store = Arc::new(AutomationStore::open_in_memory().unwrap());
    let worker = Worker::start(
        Arc::clone(&store),
        Arc::new(Succeeds),
        Arc::new(|_, _| {}),
        WorkerConfig {
            tick: Duration::from_secs(3600),
            run_timeout: Duration::from_secs(60),
        },
        tokio::runtime::Handle::current(),
    );
    let service = Arc::new(AutomationService::new(store, worker));
    service.set_signed_in(true);
    let registry = ToolRegistry::new();
    register(&registry, &service, writable);
    (registry, service)
}

async fn call(registry: &ToolRegistry, name: &str, arguments: Value) -> Value {
    let tool = registry.get(name).expect("tool registered");
    let result = (tool.execute)(
        ToolCallArgs {
            call_id: "call-1".to_owned(),
            name: name.to_owned(),
            arguments,
        },
        ToolRunContext {
            signal: flowy_agent_core::types::CancelToken::new(),
            agent_id: Some("session-0".to_owned()),
            cwd: None,
            todos: None,
            submission_id: None,
            session_origin: flowy_agent_core::SessionOrigin::Home,
            output_root: None,
            artifact_delivery_available: false,
        },
    )
    .await;
    match result {
        ToolExecutionResult::Success { value, .. } => value,
        ToolExecutionResult::Error { code, message, .. } => {
            json!({ "error": code, "message": message })
        }
    }
}

fn daily_definition(access: &str) -> Value {
    json!({
        "title": "Inbox digest",
        "action": { "kind": "agent_task", "prompt": "Summarise new mail" },
        "schedule": { "trigger": {
            "kind": "calendar",
            "time_zone": "Asia/Singapore",
            "local_time": "08:30:00",
            "frequency": { "kind": "weekly", "weekday": "Mon" }
        }},
        "workspace": { "path": "C:/notes", "access": access }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_tools_are_registered() {
    let (registry, _) = registry(true);
    let mut names: Vec<_> = registry
        .schemas()
        .into_iter()
        .map(|schema| schema.name)
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "automation_cancel_run",
            "automation_create",
            "automation_delete",
            "automation_get",
            "automation_history",
            "automation_list",
            "automation_pause",
            "automation_resume",
            "automation_run_now",
            "automation_update",
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_manage_automations_end_to_end() {
    let (registry, service) = registry(true);
    let created = call(
        &registry,
        "automation_create",
        json!({ "definition": daily_definition("write") }),
    )
    .await;
    assert_eq!(created["access"], "write");
    assert!(created["next_run_at_ms"].is_i64());
    assert!(created.get("access_note").is_none());
    let id = created["id"].clone();

    let listed = call(&registry, "automation_list", json!({})).await;
    assert_eq!(listed["automations"].as_array().unwrap().len(), 1);

    let updated = call(
        &registry,
        "automation_update",
        json!({
            "automation_id": id,
            "definition": {
                "title": "Stand-up",
                "action": { "kind": "reminder", "text": "Stand-up in 5 minutes" }
            }
        }),
    )
    .await;
    assert_eq!(updated["title"], "Stand-up");
    assert_eq!(updated["trigger"]["kind"], "manual");
    assert_eq!(updated["access"], "read");

    let paused = call(
        &registry,
        "automation_pause",
        json!({ "automation_id": id }),
    )
    .await;
    assert_eq!(paused["enabled"], false);

    let run = call(
        &registry,
        "automation_run_now",
        json!({ "automation_id": id }),
    )
    .await;
    let run_id = RunId::parse(run["id"].as_str().unwrap()).unwrap();
    for _ in 0..200 {
        if service
            .run(run_id)
            .unwrap()
            .is_some_and(|run| run.status.is_final())
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let history = call(
        &registry,
        "automation_history",
        json!({ "automation_id": id }),
    )
    .await;
    assert_eq!(history["runs"][0]["status"], "succeeded");

    let deleted = call(
        &registry,
        "automation_delete",
        json!({ "automation_id": id }),
    )
    .await;
    assert_eq!(deleted["deleted"], true);
    let missing =
        call(&registry, "automation_get", json!({ "automation_id": id })).await;
    assert_eq!(missing["error"], "not_found");
}

#[tokio::test(flavor = "multi_thread")]
async fn write_access_is_lowered_in_a_read_only_conversation() {
    let (registry, _) = registry(false);
    let created = call(
        &registry,
        "automation_create",
        json!({ "definition": daily_definition("write") }),
    )
    .await;
    assert_eq!(created["access"], "read");
    assert_eq!(created["access_note"], DOWNGRADE_NOTE);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_arguments_are_reported_to_the_model() {
    let (registry, _) = registry(true);
    let unknown = call(
        &registry,
        "automation_create",
        json!({ "definition": daily_definition("read"), "budget": 1 }),
    )
    .await;
    assert_eq!(unknown["error"], "invalid_request");
    let empty = call(
        &registry,
        "automation_create",
        json!({ "definition": {
            "title": "",
            "action": { "kind": "reminder", "text": "x" }
        }}),
    )
    .await;
    assert_eq!(empty["error"], "invalid_request");
}

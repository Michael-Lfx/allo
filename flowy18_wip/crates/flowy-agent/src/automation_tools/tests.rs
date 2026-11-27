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
        _automation: &'a Automation,
        _run_id: RunId,
        _cancel: CancelToken,
    ) -> RunFuture<'a> {
        Box::pin(async {
            RunReport {
                work_id: Some("w-1".to_owned()),
                outcome: RunOutcome::Succeeded,
            }
        })
    }
}

fn service() -> Arc<AutomationService> {
    let store = Arc::new(AutomationStore::open_in_memory().unwrap());
    let worker = Worker::start(
        Arc::clone(&store),
        Arc::new(Succeeds),
        WorkerConfig {
            tick: Duration::from_secs(3600),
            run_timeout: Duration::from_secs(60),
        },
        tokio::runtime::Handle::current(),
    );
    Arc::new(AutomationService::new(store, worker))
}

fn registry(
    service: &Arc<AutomationService>,
    conversation: &str,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    register(&registry, service, conversation);
    registry
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
            agent_id: Some("w-0".to_owned()),
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

fn weekly_definition(access: &str) -> Value {
    json!({
        "title": "Inbox digest",
        "action": { "kind": "agent_task", "prompt": "Summarise new mail" },
        "schedule": { "trigger": {
            "kind": "calendar",
            "time_zone": "Asia/Singapore",
            "local_time": "08:30:00",
            "frequency": { "kind": "weekly", "weekday": "Mon" }
        }},
        "access": access
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_tools_are_registered_including_run_now() {
    let service = service();
    let mut names: Vec<_> = registry(&service, "r-1")
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
async fn run_now_starts_a_test_run_in_this_conversation() {
    let service = service();
    let owner = registry(&service, "r-1");
    let other = registry(&service, "r-2");
    let created = call(
        &owner,
        "automation_create",
        json!({ "definition": weekly_definition("write") }),
    )
    .await;
    let id = created["id"].clone();

    let signed_out =
        call(&owner, "automation_run_now", json!({ "automation_id": id }))
            .await;
    assert_eq!(signed_out["error"], "run_unavailable");

    service.set_signed_in(true);
    let foreign =
        call(&other, "automation_run_now", json!({ "automation_id": id }))
            .await;
    assert_eq!(foreign["error"], "not_found");
    let started =
        call(&owner, "automation_run_now", json!({ "automation_id": id }))
            .await;
    assert_eq!(started["run"]["automation_id"], id);
    assert_eq!(started["run"]["status"], "running");

    let mut history = json!(null);
    for _ in 0..500 {
        history =
            call(&owner, "automation_history", json!({ "automation_id": id }))
                .await;
        if history["runs"][0]["status"] == "succeeded" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(history["runs"][0]["status"], "succeeded");
    assert_eq!(history["runs"][0]["work_id"], "w-1");
    let after =
        call(&owner, "automation_get", json!({ "automation_id": id })).await;
    assert_eq!(after["next_run_at_ms"], created["next_run_at_ms"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_manage_this_conversations_automations() {
    let service = service();
    let tools = registry(&service, "r-1");
    let created = call(
        &tools,
        "automation_create",
        json!({ "definition": weekly_definition("write") }),
    )
    .await;
    assert_eq!(created["conversation_id"], "r-1");
    assert_eq!(created["access"], "write");
    assert!(created["next_run_at_ms"].is_i64());
    assert!(created["next_run_local"].is_string());
    let id = created["id"].clone();

    let listed = call(&tools, "automation_list", json!({})).await;
    assert_eq!(listed["automations"].as_array().unwrap().len(), 1);

    let updated = call(
        &tools,
        "automation_update",
        json!({
            "automation_id": id,
            "definition": {
                "title": "Stand-up",
                "action": { "kind": "reminder", "text": "Stand-up in 5 minutes" },
                "schedule": { "trigger": {
                    "kind": "once", "scheduled_at_ms": 4_102_444_800_000_i64
                }}
            }
        }),
    )
    .await;
    assert_eq!(updated["title"], "Stand-up");
    assert_eq!(updated["trigger"]["kind"], "once");
    assert_eq!(updated["access"], "read");

    let paused =
        call(&tools, "automation_pause", json!({ "automation_id": id })).await;
    assert_eq!(paused["enabled"], false);
    let history =
        call(&tools, "automation_history", json!({ "automation_id": id }))
            .await;
    assert_eq!(history["runs"], json!([]));

    let deleted =
        call(&tools, "automation_delete", json!({ "automation_id": id })).await;
    assert_eq!(deleted["deleted"], true);
    let missing =
        call(&tools, "automation_get", json!({ "automation_id": id })).await;
    assert_eq!(missing["error"], "not_found");
}

#[tokio::test(flavor = "multi_thread")]
async fn other_conversations_cannot_see_or_change_an_automation() {
    let service = service();
    let owner = registry(&service, "r-1");
    let other = registry(&service, "r-2");
    let created = call(
        &owner,
        "automation_create",
        json!({ "definition": weekly_definition("read") }),
    )
    .await;
    let id = created["id"].clone();

    let listed = call(&other, "automation_list", json!({})).await;
    assert_eq!(listed["automations"], json!([]));
    for name in ["automation_get", "automation_pause", "automation_delete"] {
        let result = call(&other, name, json!({ "automation_id": id })).await;
        assert_eq!(result["error"], "not_found", "{name}");
    }
    assert!(
        service
            .get(AutomationId::parse(id.as_str().unwrap()).unwrap())
            .unwrap()
            .enabled
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_arguments_are_reported_to_the_model() {
    let service = service();
    let tools = registry(&service, "r-1");
    let unknown = call(
        &tools,
        "automation_create",
        json!({ "definition": weekly_definition("read"), "budget": 1 }),
    )
    .await;
    assert_eq!(unknown["error"], "invalid_request");
    let no_schedule = call(
        &tools,
        "automation_create",
        json!({ "definition": {
            "title": "x",
            "action": { "kind": "reminder", "text": "x" }
        }}),
    )
    .await;
    assert_eq!(no_schedule["error"], "invalid_request");
    let manual = call(
        &tools,
        "automation_create",
        json!({ "definition": {
            "title": "x",
            "action": { "kind": "reminder", "text": "x" },
            "schedule": { "trigger": { "kind": "manual" } }
        }}),
    )
    .await;
    assert_eq!(manual["error"], "invalid_request");
}

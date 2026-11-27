use std::path::PathBuf;

use flowy_automation::schedule::TriggerSpec;
use flowy_automation::{
    AutomationId, AutomationStore, Worker, WorkerConfig, now_ms,
};

use super::*;
use crate::LlmConfig;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flowy-automation-exec-{tag}-{}",
            AutomationId::generate()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// No API key: turns run through the local echo stream.
fn echo_handle(root: &std::path::Path) -> AgentHandle {
    AgentHandle::with_config(
        root.to_path_buf(),
        LlmConfig {
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            provider: String::new(),
            ..LlmConfig::default()
        },
    )
}

/// Installs a service whose worker never ticks, so only the host is tested.
fn install_service(handle: &AgentHandle) {
    let store = Arc::new(AutomationStore::open_in_memory().unwrap());
    let worker = Worker::start(
        Arc::clone(&store),
        handle.automation_executor(),
        WorkerConfig {
            tick: Duration::from_secs(3600),
            run_timeout: Duration::from_secs(60),
        },
        handle.runtime_handle(),
    );
    handle.set_automation_service(Some(Arc::new(AutomationService::new(
        store, worker,
    ))));
}

fn automation(conversation: &str, access: Access) -> Automation {
    let now = now_ms();
    Automation {
        id: AutomationId::generate(),
        conversation_id: conversation.to_owned(),
        title: "Morning summary".to_owned(),
        action: Action::AgentTask {
            prompt: "summarise the notes".to_owned(),
        },
        trigger: TriggerSpec::Once {
            scheduled_at_ms: now + 86_400_000,
        },
        access,
        enabled: true,
        next_run_at_ms: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn dispatch_user_work(
    handle: &AgentHandle,
    router_id: &str,
    mode: WorkAccessMode,
) -> String {
    handle
        .inner
        .dispatch_work_from_router(
            router_id,
            DispatchWorkRequest {
                brief: "set up a daily summary".to_owned(),
                requested_mode: Some(mode),
                target_work_id: None,
                target_object_ids: Vec::new(),
                request_id: None,
            },
        )
        .unwrap()
        .work_id
}

fn record(handle: &AgentHandle, id: &str) -> SessionRecord {
    lock(&handle.inner.engine)
        .clone()
        .unwrap()
        .session(id)
        .unwrap()
}

#[test]
fn only_user_dispatched_read_write_workers_get_the_tools() {
    let root = Scratch::new("tools");
    let handle = echo_handle(&root.0);
    install_service(&handle);
    let router_id = handle.create_router_session().unwrap();
    let writer =
        dispatch_user_work(&handle, &router_id, WorkAccessMode::ReadWrite);
    let reader =
        dispatch_user_work(&handle, &router_id, WorkAccessMode::ReadOnly);

    let tools = handle.inner.automation_tools_for(&record(&handle, &writer));
    assert_eq!(
        tools.map(|(_, conversation)| conversation),
        Some(router_id.clone())
    );
    assert!(
        handle
            .inner
            .automation_tools_for(&record(&handle, &reader))
            .is_none()
    );
    assert!(
        handle
            .inner
            .automation_tools_for(&record(&handle, &router_id))
            .is_none()
    );

    handle.set_automation_service(None);
    assert!(
        handle
            .inner
            .automation_tools_for(&record(&handle, &writer))
            .is_none()
    );
}

#[test]
fn a_run_dispatches_a_worker_in_its_conversation_without_the_tools() {
    let root = Scratch::new("dispatch");
    let handle = echo_handle(&root.0);
    install_service(&handle);
    let router_id = handle.create_router_session().unwrap();
    let automation = automation(&router_id, Access::Write);
    let run_id = RunId::generate();

    let work_id = handle
        .inner
        .dispatch_run(&automation, run_id, "summarise the notes")
        .unwrap();
    let worker = record(&handle, &work_id);
    assert_eq!(worker.role, SessionRole::Worker);
    assert_eq!(worker.root_session_id.as_deref(), Some(router_id.as_str()));
    assert_eq!(worker.access_mode, WorkAccessMode::ReadWrite);
    let sidebar_sessions = handle.list_sessions();
    assert_eq!(sidebar_sessions.len(), 1);
    assert_eq!(sidebar_sessions[0].id, router_id);
    assert!(
        handle
            .inner
            .child_works(&router_id)
            .iter()
            .any(|work| work.work_id == work_id)
    );
    // A run's Worker must not create automations of its own.
    assert!(handle.inner.automation_tools_for(&worker).is_none());
    // The same run never dispatches twice.
    assert_eq!(
        handle
            .inner
            .dispatch_run(&automation, run_id, "summarise the notes")
            .unwrap(),
        work_id
    );
}

#[test]
fn read_access_dispatches_a_read_only_worker() {
    let root = Scratch::new("read");
    let handle = echo_handle(&root.0);
    let router_id = handle.create_router_session().unwrap();
    let work_id = handle
        .inner
        .dispatch_run(
            &automation(&router_id, Access::Read),
            RunId::generate(),
            "x",
        )
        .unwrap();
    assert_eq!(
        record(&handle, &work_id).access_mode,
        WorkAccessMode::ReadOnly
    );
}

#[test]
fn a_run_without_its_conversation_fails() {
    let root = Scratch::new("missing");
    let handle = echo_handle(&root.0);
    let standard = handle.create_session().unwrap();
    let missing = automation("r-missing", Access::Read);
    assert_eq!(
        handle.inner.dispatch_run(&missing, RunId::generate(), "x"),
        Err("ROUTER_NOT_FOUND")
    );
    let reminder = Automation {
        action: Action::Reminder {
            text: "stand-up".to_owned(),
        },
        ..automation(&standard, Access::Read)
    };
    assert_eq!(
        handle.inner.post_reminder(&reminder, "stand-up"),
        Err("ROUTER_NOT_FOUND")
    );
}

#[test]
fn work_states_map_to_run_outcomes() {
    assert_eq!(settled("queued"), None);
    assert_eq!(settled("running"), None);
    assert_eq!(settled("completed"), Some(RunOutcome::Succeeded));
    assert_eq!(settled("delivered"), Some(RunOutcome::Succeeded));
    assert_eq!(settled("stopped"), Some(RunOutcome::Cancelled));
    assert!(matches!(settled("failed"), Some(RunOutcome::Failed(_))));
    assert_eq!(settled("awaiting_approval"), None);
}

#[test]
fn a_reminder_is_visible_and_survives_reopening_without_a_model_turn() {
    let root = Scratch::new("reminder");
    let handle = echo_handle(&root.0);
    let router = handle.create_router_session().unwrap();
    let task = automation(&router, Access::Read);

    handle.inner.post_reminder(&task, "stand-up").unwrap();
    let has_reminder = |handle: &AgentHandle| {
        handle.session_lines(&router).iter().any(|line| {
            matches!(line, ChatLine::Assistant { text } if text.contains("stand-up"))
        })
    };
    assert!(has_reminder(&handle));
    assert!(handle.child_works(&router).is_empty());

    drop(handle);
    assert!(has_reminder(&echo_handle(&root.0)));
}

use std::path::PathBuf;

use flowy_automation::schedule::TriggerSpec;
use flowy_automation::{AutomationId, now_ms};

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

/// `AgentHandle` owns a runtime, so it must not be dropped inside an async
/// test; drive the executor on a throwaway runtime from a sync test instead.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}
fn automation(cwd: Option<PathBuf>, access: Access) -> Automation {
    let now = now_ms();
    Automation {
        id: AutomationId::generate(),
        title: "Morning summary".to_owned(),
        action: Action::AgentTask {
            prompt: "summarise the notes".to_owned(),
        },
        trigger: TriggerSpec::Manual,
        cwd,
        access,
        enabled: true,
        next_run_at_ms: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

#[test]
fn a_run_completes_in_a_titled_background_session_without_tools() {
    let root = Scratch::new("home");
    let handle = echo_handle(&root.0);
    let active_before = handle.active_session_id();

    let automation = automation(None, Access::Read);
    let executor = handle.automation_executor();
    let report = block_on(executor.run(
        &automation,
        RunId::generate(),
        RunCancel::default(),
    ));

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    let session_id = report.session_id.expect("session");
    assert_eq!(handle.active_session_id(), active_before);
    assert!(handle.list_sessions().iter().any(|session| {
        session.id == session_id && session.title == "Morning summary"
    }));
    assert!(
        handle
            .session_lines(&session_id)
            .iter()
            .any(|line| matches!(
                line,
                crate::ChatLine::User { text } if text == "summarise the notes"
            ))
    );
    assert!(handle.inner.automation_tools_for(&session_id).is_none());
}

#[test]
fn a_locked_workspace_skips_without_creating_a_session() {
    let root = Scratch::new("busy");
    let workspace = root.0.join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let handle = echo_handle(&root.0);
    let sessions_before = handle.list_sessions().len();
    let _held = handle
        .inner
        .state
        .workspace_locks
        .try_acquire(&workspace, WorkspaceAccessMode::Write, "other-turn")
        .expect("lock");

    let automation = automation(Some(workspace), Access::Read);
    let executor = handle.automation_executor();
    let report = block_on(executor.run(
        &automation,
        RunId::generate(),
        RunCancel::default(),
    ));

    assert_eq!(
        report,
        RunReport {
            session_id: None,
            outcome: RunOutcome::Busy,
        }
    );
    assert_eq!(handle.list_sessions().len(), sessions_before);
}

#[test]
fn read_only_standard_sessions_take_a_read_lock() {
    let root = Scratch::new("lock-mode");
    let handle = echo_handle(&root.0);
    let read_only = handle
        .inner
        .create_session_with_role_inner(
            SessionOrigin::Home,
            None,
            false,
            SessionRole::Standard,
            None,
            WorkAccessMode::ReadOnly,
        )
        .unwrap();
    let default = handle.create_session().unwrap();
    assert_eq!(
        handle.inner.session_access_mode(&read_only),
        Some(WorkspaceAccessMode::Read)
    );
    assert_eq!(
        handle.inner.session_access_mode(&default),
        Some(WorkspaceAccessMode::Write)
    );
}

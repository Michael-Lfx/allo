//! PA-A11: account B cannot reach account A's automation results through any
//! legacy entry, and legacy unowned content is never auto-claimed.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use flowy_agent_core::{
    CancelToken, DeliveryCommit, ExecutionMetadata, ExecutionOwner,
    ExecutionSource, SessionIndex as History, SessionRecord as HistoryRecord,
    ToolCallArgs, ToolExecutionResult, ToolRunContext,
};

use super::result_visible;
use crate::{AgentHandle, ArtifactStore, LlmConfig, SessionOrigin};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "flowy-result-visibility-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn owner(subject: &str) -> ExecutionOwner {
    ExecutionOwner {
        authority: "https://account.example".into(),
        subject: subject.into(),
    }
}

fn automation(subject: Option<&str>) -> ExecutionMetadata {
    ExecutionMetadata {
        owner: subject.map(owner),
        source: ExecutionSource::Automation {
            automation_id: "automation-1".into(),
            run_id: "run-1".into(),
            attempt_id: "attempt-1".into(),
        },
    }
}

fn handle(root: &Scratch) -> AgentHandle {
    AgentHandle::with_config(
        root.0.clone(),
        LlmConfig {
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            provider: String::new(),
            ..LlmConfig::default()
        },
    )
}

fn listed(handle: &AgentHandle) -> Vec<String> {
    let mut ids = handle
        .list_sessions()
        .into_iter()
        .map(|summary| summary.id)
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[test]
fn rule_isolates_owned_results_and_never_claims_legacy_content() {
    let a = owner("a");
    let b = owner("b");
    for legacy in [
        ExecutionMetadata::default(),
        ExecutionMetadata::conversation(),
    ] {
        assert!(result_visible(&legacy, None));
        assert!(result_visible(&legacy, Some(&a)));
        assert!(result_visible(&legacy, Some(&b)));
    }
    let owned = automation(Some("a"));
    assert!(result_visible(&owned, Some(&a)));
    assert!(!result_visible(&owned, Some(&b)));
    assert!(!result_visible(&owned, None));
    // Same subject under another authority is a different principal.
    let foreign_authority = ExecutionOwner {
        authority: "https://other.example".into(),
        subject: "a".into(),
    };
    assert!(!result_visible(&owned, Some(&foreign_authority)));
    // Missing owner or malformed identity is quarantined for everyone.
    let unowned = automation(None);
    assert!(!result_visible(&unowned, None));
    assert!(!result_visible(&unowned, Some(&a)));
    let mut blank = automation(Some(" "));
    assert!(!result_visible(&blank, blank.owner.clone().as_ref()));
    blank.source = ExecutionSource::Automation {
        automation_id: "automation-1".into(),
        run_id: String::new(),
        attempt_id: "attempt-1".into(),
    };
    blank.owner = Some(a.clone());
    assert!(!result_visible(&blank, Some(&a)));
}

#[test]
fn foreign_account_cannot_list_select_read_or_delete_automation_sessions() {
    let root = Scratch::new();
    History {
        active: None,
        sessions: vec![
            HistoryRecord {
                id: "automation-a".into(),
                title: "Owned by A".into(),
                execution: automation(Some("a")),
                ..HistoryRecord::default()
            },
            HistoryRecord {
                id: "legacy".into(),
                title: "Legacy".into(),
                ..HistoryRecord::default()
            },
        ],
    }
    .save(&root.0)
    .unwrap();
    let handle = handle(&root);

    // Before any account is installed, owned results stay hidden.
    assert_eq!(listed(&handle), vec!["legacy"]);
    assert_eq!(
        handle.select_session("automation-a"),
        Err("notice.agent_failed")
    );
    assert!(!handle.source_available("automation-a"));
    assert!(handle.source_available("legacy"));

    let initial = handle.result_viewer_generation();
    let as_a = handle.set_result_viewer(Some(owner("a")));
    assert!(as_a > initial);
    assert_eq!(listed(&handle), vec!["automation-a", "legacy"]);
    handle.select_session("automation-a").unwrap();
    assert_eq!(handle.active_session_id().as_deref(), Some("automation-a"));

    // Switching account deselects the foreign session and hides it from
    // every per-session query, including guessed IDs.
    let as_b = handle.set_result_viewer(Some(owner("b")));
    assert_ne!(as_b, as_a);
    assert_eq!(handle.active_session_id(), None);
    assert_eq!(listed(&handle), vec!["legacy"]);
    assert_eq!(handle.chat_history().rows.len(), 0);
    assert_eq!(handle.session_lines("automation-a").len(), 0);
    assert_eq!(handle.session_preview("automation-a"), None);
    assert!(!handle.session_needs_attention("automation-a"));
    assert_eq!(handle.session_pending_cards("automation-a").len(), 0);
    assert_eq!(handle.session_heartbeat("automation-a"), None);
    assert!(handle.session_work_status("automation-a").is_none());
    assert!(!handle.source_available("automation-a"));
    assert_eq!(
        handle.select_session("automation-a"),
        Err("notice.agent_failed")
    );
    assert_eq!(
        handle.delete_session("automation-a"),
        Err("notice.agent_failed")
    );
    assert!(handle.select_session("guessed-session-id").is_err());

    // Same-account re-login still advances the generation, so late callbacks
    // from the earlier sign-in are discarded. Nothing was deleted.
    let again = handle.set_result_viewer(Some(owner("a")));
    assert!(again > as_b);
    assert_eq!(listed(&handle), vec!["automation-a", "legacy"]);
    assert_eq!(History::load(&root.0).unwrap().sessions.len(), 2);
    drop(handle);
}

fn owned_session(root: &Scratch) -> AgentHandle {
    History {
        active: None,
        sessions: vec![HistoryRecord {
            id: "automation-a".into(),
            title: "Owned by A".into(),
            origin: SessionOrigin::Project {
                project_id: Some("project-a".into()),
            },
            execution: automation(Some("a")),
            ..HistoryRecord::default()
        }],
    }
    .save(&root.0)
    .unwrap();
    handle(root)
}

/// Park a real mutation behind the same mutex used to commit viewer changes.
/// Starting under B must not authorize or reject before the pending switch to
/// A commits; the mutation must decide using A after it obtains dispatch.
fn during_account_switch<R: Send>(
    handle: &AgentHandle,
    mutation: impl FnOnce(&AgentHandle) -> R + Send,
) -> R {
    handle.set_result_viewer(Some(owner("b")));
    std::thread::scope(|scope| {
        let dispatch = super::super::lock(&handle.inner.state.dispatch);
        let (started, worker_started) = mpsc::channel();
        let (completed, worker_completed) = mpsc::channel();
        let worker = scope.spawn(move || {
            started.send(()).unwrap();
            let result = mutation(handle);
            completed.send(()).unwrap();
            result
        });
        worker_started.recv_timeout(Duration::from_secs(5)).unwrap();
        let early_completion =
            worker_completed.recv_timeout(Duration::from_millis(200));
        // The dispatch guard models the account switch already in progress.
        // Commit exactly the viewer state used by set_result_viewer, without
        // recursively acquiring its mutex from this test thread.
        *super::super::lock(&handle.inner.state.result_viewer) =
            Some(owner("a"));
        handle
            .inner
            .state
            .result_viewer_generation
            .fetch_add(1, Ordering::AcqRel);
        drop(dispatch);
        let result = worker.join().unwrap();
        assert_eq!(
            early_completion,
            Err(mpsc::RecvTimeoutError::Timeout),
            "a session mutation checked the viewer before acquiring dispatch",
        );
        result
    })
}

#[test]
fn delete_session_waits_for_account_switch_before_checking_owner() {
    let root = Scratch::new();
    let handle = owned_session(&root);
    let result = during_account_switch(&handle, |agent| {
        agent.delete_session("automation-a")
    });
    assert_eq!(result, Ok(()));
    assert_eq!(History::load(&root.0).unwrap().sessions.len(), 0);
}

#[test]
fn retry_delivery_waits_for_account_switch_before_checking_owner() {
    let root = Scratch::new();
    let handle = owned_session(&root);
    let result = during_account_switch(&handle, |agent| {
        agent.retry_delivery(Some("automation-a"))
    });
    // The current owner can reach delivery validation; this session has no
    // saved delivery group to retry. Its history must remain intact.
    assert_eq!(result, Err("chat.status_delivery_failed"));
    assert_eq!(History::load(&root.0).unwrap().sessions.len(), 1);
}

#[test]
fn override_delivery_waits_for_account_switch_before_checking_owner() {
    let root = Scratch::new();
    let handle = owned_session(&root);
    let result = during_account_switch(&handle, |agent| {
        agent.override_deliver_artifact(
            "automation-a",
            "report-1",
            "result.txt",
        )
    });
    // Passing the latest owner's check reaches the existing Home-only rule.
    assert_eq!(result, Err("chat.publish.home_only"));
    assert_eq!(History::load(&root.0).unwrap().sessions.len(), 1);
}

// Synchronous on purpose: `AgentHandle` owns its own runtime and must not be
// dropped inside an async context. Only the publisher runs on a local runtime,
// which is shut down before the handle is created.
#[test]
fn foreign_account_cannot_reach_automation_artifacts_by_guessed_id() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let root = Scratch::new();
    let execution = automation(Some("a"));
    let output = root.0.join("workspace/outputs/submission-1");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("result.txt"), b"automation result").unwrap();
    let store = ArtifactStore::open(root.0.join("artifacts")).unwrap();
    let publisher = crate::publish_tool::definition(store.clone(), None);
    let result = runtime.block_on((publisher.execute)(
        ToolCallArgs {
            call_id: "call-1".into(),
            name: "publish_file".into(),
            arguments: serde_json::json!({
                "path": "result.txt",
                "delivery_key": "result",
            }),
        },
        ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("automation-a".into()),
            cwd: None,
            todos: None,
            submission_id: Some("submission-1".into()),
            session_origin: SessionOrigin::Home,
            output_root: Some(output),
            artifact_delivery_available: true,
            execution: execution.clone(),
        },
    ));
    drop(publisher);
    drop(runtime);
    let ToolExecutionResult::Success {
        delivery: Some(receipt),
        ..
    } = result
    else {
        panic!("publisher should stage a host-owned artifact")
    };
    let commit = DeliveryCommit {
        session_id: "automation-a".into(),
        submission_id: "submission-1".into(),
        artifact_ids: vec![receipt.artifact_id.clone()],
        completed_successfully: true,
        committed_at_ms: 1,
        origin: SessionOrigin::Home,
        origin_title_snapshot: "Task".into(),
        execution: execution.clone(),
    };
    store.commit_prepared(&commit).unwrap();
    drop(store);
    History {
        active: None,
        sessions: vec![HistoryRecord {
            id: "automation-a".into(),
            title: "Owned by A".into(),
            origin: SessionOrigin::Home,
            execution,
            delivery_commits: vec![commit],
            ..HistoryRecord::default()
        }],
    }
    .save(&root.0)
    .unwrap();

    let handle = handle(&root);
    let id = receipt.artifact_id;
    let exports = root.0.join("exports");
    fs::create_dir_all(&exports).unwrap();
    let destination = exports.join("out.txt");
    for viewer in [None, Some(owner("b"))] {
        handle.set_result_viewer(viewer);
        assert_eq!(handle.artifacts().unwrap().len(), 0);
        assert_eq!(handle.artifact(&id).unwrap(), None);
        let denied = [
            handle.read_artifact(&id).map(|_| ()),
            handle.copy_artifact_to(&id, &destination),
            handle.create_artifact_view_copy(&id).map(|_| ()),
            handle.set_artifact_favorite(&id, true),
            handle.trash_artifact(&id),
            handle.restore_artifact(&id),
            handle.permanently_delete_artifact(&id),
        ];
        for result in denied {
            assert_eq!(result.unwrap_err().kind(), ErrorKind::NotFound);
        }
        assert!(!destination.exists());
    }

    handle.set_result_viewer(Some(owner("a")));
    assert_eq!(handle.artifacts().unwrap().len(), 1);
    assert_eq!(handle.read_artifact(&id).unwrap(), b"automation result");
    handle.copy_artifact_to(&id, &destination).unwrap();
    assert_eq!(fs::read(&destination).unwrap(), b"automation result");
    handle.set_artifact_favorite(&id, true).unwrap();

    // The owner's favorite does not reveal the item to another account.
    handle.set_result_viewer(Some(owner("b")));
    assert_eq!(handle.artifacts().unwrap().len(), 0);
    drop(handle);
}

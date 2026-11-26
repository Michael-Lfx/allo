use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use flowy_agent_core::{
    CancelToken, DeliveryCommit, ExecutionMetadata, ExecutionOwner,
    ExecutionSource, SessionIndex as History, SessionRecord as HistoryRecord,
    ToolCallArgs, ToolExecutionResult, ToolRunContext,
};

use crate::index::{SessionIndex, SessionRecord};
use crate::{AgentHandle, ArtifactStore, LlmConfig, SessionOrigin, TurnStatus};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "flowy-execution-metadata-{}-{}",
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

fn metadata() -> ExecutionMetadata {
    ExecutionMetadata {
        owner: Some(ExecutionOwner {
            authority: "https://account.example".into(),
            subject: "42".into(),
        }),
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

fn index_record(execution: ExecutionMetadata) -> SessionRecord {
    SessionRecord {
        id: "automation-session".into(),
        title: "Scheduled task".into(),
        updated_at: 1,
        lines: Vec::new(),
        status: TurnStatus::Idle,
        titled: true,
        workspace: None,
        origin: SessionOrigin::Home,
        session_workspace: None,
        execution,
        ..SessionRecord::default()
    }
}

#[test]
fn old_index_execution_metadata_defaults_to_unknown_and_unowned() {
    let index: SessionIndex = serde_json::from_str(r#"{"active":null,"sessions":[{"id":"legacy","title":"Old","updated_at":1}]}"#).unwrap();
    assert_eq!(index.sessions[0].execution, ExecutionMetadata::default());
}

#[test]
fn missing_and_stale_index_preserve_history_execution_identity() {
    for stale in [false, true] {
        let root = Scratch::new();
        let record = HistoryRecord {
            id: "automation-session".into(),
            title: "Scheduled task".into(),
            execution: metadata(),
            ..HistoryRecord::default()
        };
        History {
            active: None,
            sessions: vec![record],
        }
        .save(&root.0)
        .unwrap();
        if stale {
            SessionIndex {
                active: None,
                sessions: vec![index_record(ExecutionMetadata::conversation())],
            }
            .save(&root.0)
            .unwrap();
        }
        let handle = handle(&root);
        assert_eq!(handle.active_session_id(), None);
        let rebuilt = SessionIndex::load(&root.0).unwrap();
        assert_eq!(rebuilt.sessions.len(), 1);
        assert_eq!(rebuilt.sessions[0].execution, metadata());
        assert_eq!(
            History::load(&root.0).unwrap().sessions[0].execution,
            metadata()
        );
        drop(handle);
    }
}

#[test]
fn index_only_valid_identity_is_not_dropped_during_hydration() {
    let root = Scratch::new();
    SessionIndex {
        active: None,
        sessions: vec![index_record(metadata())],
    }
    .save(&root.0)
    .unwrap();
    let handle = handle(&root);
    assert_eq!(
        History::load(&root.0).unwrap().sessions[0].execution,
        metadata()
    );
    assert_eq!(
        SessionIndex::load(&root.0).unwrap().sessions[0].execution,
        metadata()
    );
    drop(handle);
}

#[test]
fn unowned_automation_index_cannot_become_runnable_history() {
    let root = Scratch::new();
    let mut execution = metadata();
    execution.owner = None;
    SessionIndex {
        active: Some("automation-session".into()),
        sessions: vec![index_record(execution.clone())],
    }
    .save(&root.0)
    .unwrap();
    let handle = handle(&root);
    let empty: [HistoryRecord; 0] = [];
    assert_eq!(History::load(&root.0).unwrap().sessions, empty);
    // PA-A11: the quarantined session is not restored as active and cannot be
    // selected, so a submission can never execute inside it.
    assert_eq!(handle.active_session_id(), None);
    assert_eq!(
        handle.select_session("automation-session"),
        Err("notice.agent_failed")
    );
    let submitted = handle.submit_chat(7, "must not execute").unwrap();
    assert_ne!(submitted, "automation-session");
    assert!(
        History::load(&root.0)
            .unwrap()
            .sessions
            .iter()
            .all(|record| record.id != "automation-session")
    );
    let index = SessionIndex::load(&root.0).unwrap();
    let quarantined = index
        .sessions
        .iter()
        .find(|record| record.id == "automation-session")
        .expect("quarantined index record is preserved");
    assert_eq!(quarantined.execution, execution);
    drop(handle);
}

#[test]
fn new_foreground_session_is_an_unowned_conversation() {
    let root = Scratch::new();
    let handle = handle(&root);
    let id = handle.create_session().unwrap();
    let history = History::load(&root.0).unwrap();
    assert_eq!(
        history.get(&id).unwrap().execution,
        ExecutionMetadata::conversation()
    );
    assert_eq!(
        SessionIndex::load(&root.0).unwrap().sessions[0].execution,
        ExecutionMetadata::conversation()
    );
    drop(handle);
}

#[tokio::test]
async fn explicit_publisher_preserves_host_execution_identity() {
    let root = Scratch::new();
    let output = root.0.join("workspace/outputs/submission-1");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("result.txt"), b"automation result").unwrap();
    let store = ArtifactStore::open(root.0.join("artifacts")).unwrap();
    let publisher = crate::publish_tool::definition(store.clone(), None);
    let result = (publisher.execute)(ToolCallArgs {
        call_id: "call-1".into(), name: "publish_file".into(),
        arguments: serde_json::json!({"path":"result.txt","delivery_key":"result"}),
    }, ToolRunContext {
        signal: CancelToken::new(), agent_id: Some("automation-session".into()), cwd: None,
        todos: None, submission_id: Some("submission-1".into()), session_origin: SessionOrigin::Home,
        output_root: Some(output), artifact_delivery_available: true, execution: metadata(),
    }).await;
    let ToolExecutionResult::Success {
        delivery: Some(receipt),
        ..
    } = result
    else {
        panic!("publisher should stage a host-owned artifact")
    };
    let commit = DeliveryCommit {
        session_id: "automation-session".into(),
        submission_id: "submission-1".into(),
        artifact_ids: vec![receipt.artifact_id.clone()],
        completed_successfully: true,
        committed_at_ms: 1,
        origin: SessionOrigin::Home,
        origin_title_snapshot: "Task".into(),
        execution: metadata(),
    };
    let mut other_owner = commit.clone();
    other_owner.execution.owner.as_mut().unwrap().subject =
        "other-owner".into();
    assert!(store.commit_prepared(&other_owner).is_err());
    assert_eq!(store.list().unwrap().len(), 0);
    store.commit_prepared(&commit).unwrap();
    store.commit_prepared(&commit).unwrap();
    assert!(store.commit_prepared(&other_owner).is_err());
    drop(store);
    let reopened = ArtifactStore::open(root.0.join("artifacts")).unwrap();
    let records = reopened.list().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.execution, metadata());
    assert_eq!(
        reopened.read_payload(&receipt.artifact_id).unwrap(),
        b"automation result"
    );
    assert!(matches!(
        records[0].manifest.execution.source,
        ExecutionSource::Automation { .. }
    ));
}

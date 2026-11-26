use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use crate::tools::ToolRegistry;
use crate::{
    ContentBlock, Engine, EngineConfig, ExecutionMetadata, ExecutionOwner,
    ExecutionSource, Message, MessageSource, ModelRoute, Role,
    SessionCreateDisposition, SessionIndex, SessionOrigin, SessionRecord,
    SessionSpec, SessionWorkspace, TurnObserver, TurnState,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build-test")
            .join(format!(
                "execution-metadata-{}-{}",
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

struct Observer;
impl TurnObserver for Observer {}

fn engine(root: &Scratch) -> Engine {
    Engine::open(
        root.0.clone(),
        EngineConfig {
            route: ModelRoute::default(),
            api_key: None,
            system: String::new(),
            registry: ToolRegistry::new(),
            cwd: None,
            todos: None,
            pre_tool_hook: None,
            work_contract: None,
            delivery_committer: None,
        },
        Arc::new(Observer),
    )
    .unwrap()
}

#[test]
fn execution_owner_debug_redacts_authority_and_subject() {
    let owner = ExecutionOwner {
        authority: "https://account.example".into(),
        subject: "sensitive-user-42".into(),
    };
    let debug = format!("{owner:?}");
    assert_eq!(debug, "ExecutionOwner(<redacted>)");
    assert!(!debug.contains("account.example"));
    assert!(!debug.contains("sensitive-user-42"));
}

fn spec(root: &Scratch) -> SessionSpec {
    SessionSpec {
        id: "automation-session".into(),
        created_at_ms: 10,
        origin: SessionOrigin::Home,
        workspace: Some(SessionWorkspace {
            root: root.0.join("workspace"),
        }),
        execution: ExecutionMetadata {
            owner: Some(ExecutionOwner {
                authority: "https://account.example".into(),
                subject: "42".into(),
            }),
            source: ExecutionSource::Automation {
                automation_id: "automation-1".into(),
                run_id: "run-1".into(),
                attempt_id: "attempt-1".into(),
            },
        },
        role: crate::SessionRole::Standard,
        root_session_id: None,
        access_mode: crate::WorkAccessMode::ReadWrite,
    }
}

#[test]
fn legacy_history_execution_identity_stays_unknown_and_unowned() {
    let record: SessionRecord =
        serde_json::from_str(r#"{"id":"legacy","messages":[]}"#).unwrap();
    assert_eq!(record.execution, ExecutionMetadata::default());
    assert!(record.execution.is_valid());
    let metadata: ExecutionMetadata = serde_json::from_str("{}").unwrap();
    assert_eq!(metadata, ExecutionMetadata::default());
}

#[test]
fn create_if_absent_preserves_existing_history_active_and_file_bytes() {
    let root = Scratch::new();
    let engine = engine(&root);
    let spec = spec(&root);
    assert_eq!(
        engine.create_if_absent(spec.clone()).unwrap(),
        SessionCreateDisposition::Created
    );
    let mut record = engine.session(&spec.id).unwrap();
    record.title = "Already completed".into();
    record.status = TurnState::Completed;
    record.updated_at = 20;
    record.messages.push(Message {
        id: "u1".into(),
        role: Role::User,
        content: vec![ContentBlock::text("keep this durable history")],
        source: MessageSource::User,
        display_text: None,
    });
    engine.insert_session(record.clone()).unwrap();
    engine.create_session("foreground".into(), 30).unwrap();
    engine.set_active(Some("foreground".into())).unwrap();
    let before = fs::read(SessionIndex::path(&root.0)).unwrap();
    let mut replay = spec;
    replay.created_at_ms = 999;
    assert_eq!(
        engine.create_if_absent(replay).unwrap(),
        SessionCreateDisposition::Existing
    );
    assert_eq!(engine.session(&record.id).unwrap(), record);
    assert_eq!(fs::read(SessionIndex::path(&root.0)).unwrap(), before);
    let loaded = SessionIndex::load(&root.0).unwrap();
    assert_eq!(loaded.active.as_deref(), Some("foreground"));
    assert_eq!(loaded.get(&record.id).unwrap().execution, record.execution);
}

#[test]
fn create_if_absent_rejects_each_immutable_binding_conflict() {
    let root = Scratch::new();
    let engine = engine(&root);
    let original = spec(&root);
    engine.create_if_absent(original.clone()).unwrap();
    let mut variants = Vec::new();
    let mut changed = original.clone();
    changed.execution.owner.as_mut().unwrap().subject = "other-subject".into();
    variants.push(changed);
    let mut changed = original.clone();
    changed.execution.owner.as_mut().unwrap().authority =
        "other-authority".into();
    variants.push(changed);
    for index in 0..3 {
        let mut changed = original.clone();
        let ExecutionSource::Automation {
            automation_id,
            run_id,
            attempt_id,
        } = &mut changed.execution.source
        else {
            unreachable!()
        };
        match index {
            0 => *automation_id = "other-automation".into(),
            1 => *run_id = "other-run".into(),
            _ => *attempt_id = "other-attempt".into(),
        }
        variants.push(changed);
    }
    let mut changed = original.clone();
    changed.execution.source = ExecutionSource::Conversation;
    variants.push(changed);
    let mut changed = original.clone();
    changed.origin = SessionOrigin::Project {
        project_id: Some("p1".into()),
    };
    variants.push(changed);
    let mut changed = original;
    changed.workspace = Some(SessionWorkspace {
        root: root.0.join("other-workspace"),
    });
    variants.push(changed);
    let before = fs::read(SessionIndex::path(&root.0)).unwrap();
    let sessions = engine.sessions();
    for conflict in variants {
        assert_eq!(
            engine.create_if_absent(conflict).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(engine.sessions(), sessions);
        assert_eq!(fs::read(SessionIndex::path(&root.0)).unwrap(), before);
    }
}

#[test]
fn create_if_absent_rejects_unowned_or_incomplete_automation() {
    let root = Scratch::new();
    let engine = engine(&root);
    let mut missing_owner = spec(&root);
    missing_owner.execution.owner = None;
    let mut missing_attempt = spec(&root);
    if let ExecutionSource::Automation { attempt_id, .. } =
        &mut missing_attempt.execution.source
    {
        attempt_id.clear();
    }
    let mut blank_owner = spec(&root);
    blank_owner.execution.owner.as_mut().unwrap().authority = " ".into();
    let mut missing_id = spec(&root);
    missing_id.id.clear();
    for invalid in [missing_owner, missing_attempt, blank_owner, missing_id] {
        assert_eq!(
            engine.create_if_absent(invalid).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }
    assert_eq!(engine.sessions().as_slice(), []);
    assert!(!SessionIndex::path(&root.0).exists());
}

#[test]
fn create_if_absent_save_failure_restores_memory_and_allows_retry() {
    let root = Scratch::new();
    let engine = engine(&root);
    engine.create_session("existing".into(), 1).unwrap();
    engine.set_active(Some("existing".into())).unwrap();
    let before = engine.sessions();
    let path = SessionIndex::path(&root.0);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(engine.create_if_absent(spec(&root)).is_err());
    assert_eq!(engine.sessions(), before);
    assert!(engine.session("automation-session").is_none());
    fs::remove_dir(&path).unwrap();
    assert_eq!(
        engine.create_if_absent(spec(&root)).unwrap(),
        SessionCreateDisposition::Created
    );
    let saved = SessionIndex::load(&root.0).unwrap();
    assert_eq!(saved.active.as_deref(), Some("existing"));
    assert_eq!(saved.sessions.len(), 2);
}

#[test]
fn concurrent_create_if_absent_creates_one_record() {
    let root = Scratch::new();
    let engine = Arc::new(engine(&root));
    let spec = spec(&root);
    let barrier = Arc::new(Barrier::new(8));
    let mut callers = Vec::with_capacity(8);
    for _ in 0..8 {
        let engine = Arc::clone(&engine);
        let barrier = Arc::clone(&barrier);
        let spec = spec.clone();
        callers.push(std::thread::spawn(move || {
            barrier.wait();
            engine.create_if_absent(spec).unwrap()
        }));
    }
    let created = callers
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|outcome| *outcome == SessionCreateDisposition::Created)
        .count();
    assert_eq!(created, 1);
    assert_eq!(engine.sessions().len(), 1);
    assert_eq!(SessionIndex::load(&root.0).unwrap().sessions.len(), 1);
}

#[test]
fn corrupt_history_is_not_treated_as_a_missing_session() {
    let root = Scratch::new();
    fs::create_dir_all(root.0.join("sessions")).unwrap();
    fs::write(SessionIndex::path(&root.0), b"{broken").unwrap();
    let result = Engine::open(
        root.0.clone(),
        EngineConfig {
            route: ModelRoute::default(),
            api_key: None,
            system: String::new(),
            registry: ToolRegistry::new(),
            cwd: None,
            todos: None,
            pre_tool_hook: None,
            work_contract: None,
            delivery_committer: None,
        },
        Arc::new(Observer),
    );
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(fs::read(SessionIndex::path(&root.0)).unwrap(), b"{broken");
}

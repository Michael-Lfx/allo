//! The real Engine/RegistryTool/Agent cancellation path must retain a shared
//! ticket while a cooperative tool is still performing its cleanup.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{ToolExecutionResult, ToolRunContext};
use serde_json::json;
use tokio::sync::Notify;

use super::*;

fn read_request(socket: &mut TcpStream) {
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) =
            bytes.windows(4).position(|part| part == b"\r\n\r\n")
        {
            break index + 4;
        }
        assert!(bytes.len() < 2 * 1024 * 1024);
        let mut buffer = [0; 4096];
        let read = socket.read(&mut buffer).unwrap();
        assert_ne!(read, 0);
        bytes.extend_from_slice(&buffer[..read]);
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    assert!(length < 2 * 1024 * 1024);
    while bytes.len() < header_end + length {
        let mut buffer = [0; 4096];
        let read = socket.read(&mut buffer).unwrap();
        assert_ne!(read, 0);
        bytes.extend_from_slice(&buffer[..read]);
    }
}

fn slow_tool_provider() -> (LlmConfig, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let task = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    assert!(
                        Instant::now() < deadline,
                        "missing fixture request"
                    );
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("fixture accept failed: {error}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        read_request(&mut socket);
        let delta = json!({
            "id":"arbiter-fixture", "object":"chat.completion.chunk",
            "created":1, "model":"arbiter-fixture-model",
            "choices":[{"index":0,"finish_reason":null,"delta":{
                "role":"assistant","tool_calls":[{
                    "index":0,"id":"arbiter-call","type":"function",
                    "function":{"name":"arbiter_slow","arguments":"{}"}
                }]
            }}]
        });
        let done = json!({
            "id":"arbiter-fixture", "object":"chat.completion.chunk",
            "created":1, "model":"arbiter-fixture-model",
            "choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]
        });
        let body = format!("data: {delta}\n\ndata: {done}\n\ndata: [DONE]\n\n");
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        socket.flush().unwrap();
    });
    (
        LlmConfig {
            api_key: Some("isolated-arbiter-test-token".into()),
            base_url: format!("http://{address}/v1"),
            model: "arbiter-fixture-model".into(),
            provider: "openai".into(),
            ..LlmConfig::default()
        },
        task,
    )
}

struct ReleaseOnDrop(Arc<Notify>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[test]
fn agent_cancel_keeps_workspace_ticket_until_the_real_tool_finishes_cleanup() {
    let root = std::env::temp_dir().join(format!(
        "flowy-arbiter-agent-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(&root).unwrap();
    let (config, provider) = slow_tool_provider();
    let handle = AgentHandle::with_config(root.clone(), config.clone());
    let session = handle.create_home_session().unwrap();
    let engine = lock(&handle.inner.engine).clone().unwrap();
    let entered = Arc::new(Notify::new());
    let cancel_seen = Arc::new(Notify::new());
    let allow_cleanup = Arc::new(Notify::new());
    let release_on_panic = ReleaseOnDrop(Arc::clone(&allow_cleanup));
    let registry = ToolRegistry::new();
    let signals = (
        Arc::clone(&entered),
        Arc::clone(&cancel_seen),
        Arc::clone(&allow_cleanup),
    );
    registry.register(Arc::new(ToolDefinition::new(
        "arbiter_slow",
        "Controlled tool cleanup in an isolated test",
        json!({"type":"object","properties":{}}),
        move |_, context: ToolRunContext| {
            let signals = (
                Arc::clone(&signals.0),
                Arc::clone(&signals.1),
                Arc::clone(&signals.2),
            );
            Box::pin(async move {
                signals.0.notify_one();
                context.signal.cancelled().await;
                signals.1.notify_one();
                signals.2.notified().await;
                ToolExecutionResult::error("CANCELLED", "cleanup complete")
            })
        },
    )));
    let cwd = engine.session(&session).unwrap().workspace.unwrap().root;
    let mut run_config = engine_config(&config, &cwd);
    run_config.registry = registry;
    let request =
        WorkspaceRequest::for_tools(&cwd, &run_config.registry.list()).unwrap();
    assert!(matches!(request, WorkspaceRequest::DeviceWrite));
    let permit = handle
        .inner
        .state
        .coordinator
        .try_acquire(ExecutionClass::Foreground, request)
        .unwrap()
        .unwrap();
    let turn = engine
        .start_with_config(
            session.clone(),
            "slow tool".into(),
            run_config,
            None,
            Some(0xa09),
        )
        .unwrap();
    let permit = permit.started();
    let task = handle.inner.runtime.spawn(async move {
        let result = turn.await;
        permit.complete();
        result
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        assert_eq!(
            handle
                .cancel_task(TaskTarget::Session(session.clone()))
                .unwrap(),
            CancelOutcome::Cancelled
        );
        tokio::time::timeout(Duration::from_secs(5), cancel_seen.notified())
            .await
            .unwrap();
        assert!(engine.is_cancelling(&session));
        assert!(!task.is_finished());
        assert!(
            handle
                .inner
                .state
                .coordinator
                .try_acquire(
                    ExecutionClass::Background,
                    WorkspaceRequest::DeviceWrite
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(
            handle
                .confirm_workspace_quiescence(true)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        allow_cleanup.notify_one();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            LoopOutcome::Aborted
        ));
    });
    assert!(!engine.is_active(&session));
    assert!(!root.join("execution-in-progress.lock").exists());
    handle
        .inner
        .state
        .coordinator
        .try_acquire(ExecutionClass::Background, WorkspaceRequest::DeviceWrite)
        .unwrap()
        .unwrap()
        .complete();
    drop(release_on_panic);
    provider.join().unwrap();
    drop(engine);
    drop(handle);
    std::fs::remove_dir_all(root).unwrap();
}

struct CheckpointFixtureGate;

impl flowy_agent_core::ExecutionGate for CheckpointFixtureGate {
    fn model_credential(
        &self,
        _: &ExecutionMetadata,
        _: &flowy_agent_core::ModelRoute,
    ) -> Result<String, flowy_agent_core::ExecutionDenied> {
        panic!("checkpoint-only history must never dispatch a model")
    }

    fn authorize_tool(
        &self,
        _: &ToolRunContext,
        _: &ToolCallArgs,
    ) -> Result<(), flowy_agent_core::ExecutionDenied> {
        panic!("checkpoint-only history must never dispatch a tool")
    }
}

#[test]
fn checkpoint_only_attempt_is_interrupted_and_cannot_be_replayed_or_rebound() {
    let root = std::env::temp_dir().join(format!(
        "flowy-checkpoint-only-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(&root).unwrap();
    let handle = AgentHandle::with_config(root.clone(), LlmConfig::default());
    let attempt = DetachedAttempt {
        session_id: "checkpoint-only-attempt".into(),
        submission: 0xa0901,
        execution: ExecutionMetadata {
            owner: Some(flowy_agent_core::ExecutionOwner {
                authority: "https://checkpoint.invalid".into(),
                subject: "7".into(),
            }),
            source: flowy_agent_core::ExecutionSource::Automation {
                automation_id: "checkpoint-automation".into(),
                run_id: "checkpoint-run".into(),
                attempt_id: "checkpoint-attempt".into(),
            },
        },
    };
    handle
        .create_detached(SessionSpec {
            id: attempt.session_id.clone(),
            created_at_ms: 1,
            origin: SessionOrigin::Home,
            workspace: None,
            execution: attempt.execution.clone(),
        })
        .unwrap();
    let engine = lock(&handle.inner.engine).clone().unwrap();
    let mut record = engine.session(&attempt.session_id).unwrap();
    assert_eq!(record.messages.len(), 0);
    let checkpoint = flowy_agent_core::RunBudgetCheckpoint {
        started_at_ms: 10,
        deadline_at_ms: 1010,
        limits: flowy_agent_core::RunBudget::new(1000, 2, 3).unwrap(),
        model_calls: 1,
        tool_calls: 1,
        stop_reason: None,
    };
    record
        .run_budget_checkpoints
        .insert(attempt.submission_id(), checkpoint);
    // Tool results use Role::User too. They are not another human submission.
    record.messages.push(Message {
        id: "persisted-tool-result".into(),
        role: Role::User,
        content: vec![ContentBlock::text("existing result")],
        source: MessageSource::Tool {
            tool: "read_file".into(),
        },
        display_text: None,
    });
    engine.insert_session(record).unwrap();
    assert_eq!(
        handle.reconcile_attempt(&attempt).unwrap(),
        DetachedAttemptState::Interrupted
    );
    let mut spec = DetachedRunSpec::new(
        attempt.clone(),
        "must not repeat".into(),
        LlmConfig {
            model: "checkpoint-fixture".into(),
            provider: "openai".into(),
            base_url: "http://127.0.0.1:1/v1".into(),
            ..LlmConfig::default()
        },
    );
    spec.execution_gate = Some(Arc::new(CheckpointFixtureGate));
    assert_eq!(
        handle.start_detached(spec).unwrap_err(),
        DetachedError::AlreadySubmitted
    );
    let mut conflicting = attempt.clone();
    conflicting.submission += 1;
    assert_eq!(
        handle.reconcile_attempt(&conflicting).unwrap_err(),
        DetachedError::BindingConflict
    );
    assert_eq!(
        engine
            .session(&attempt.session_id)
            .unwrap()
            .run_budget_checkpoints
            .get(&attempt.submission_id()),
        Some(&checkpoint)
    );
    drop(engine);
    drop(handle);
    std::fs::remove_dir_all(root).unwrap();
}

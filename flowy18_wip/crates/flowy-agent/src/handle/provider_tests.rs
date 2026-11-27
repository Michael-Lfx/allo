//! End-to-end and unit tests for [`ToolProvider`], `tool_search`, role
//! isolation (`Router` / `ReadOnly` / `ReadWrite`), shadowing defense, and
//! generic `PreToolDecision::Ask` approval recovery.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use flowy_agent_core::{
    PendingCard, PreToolDecision, SessionOrigin, TOOL_SEARCH_NAME,
    ToolCallArgs, ToolDefinition, ToolExecutionResult, ToolExposure,
    ToolPolicyContext, ToolProvider, ToolProviderContext, ToolRegistry,
    ToolRunContext, ToolSource, WorkAccessMode,
};
use serde_json::{Value, json};

use super::{
    AgentHandle, AgentWorkspace, CardAction, CardSelection, lock, next_stamp,
};
use crate::config::LlmConfig;

#[derive(Clone, Debug)]
struct RecordedHttpTurn {
    tool_names: Vec<String>,
    system_prompt: String,
    last_user: String,
    last_tool_result: Option<String>,
}

enum MockHttpReply {
    Text(String),
    ToolCalls(Vec<(String, String, Value)>),
}

type HttpHandler =
    Arc<dyn Fn(&RecordedHttpTurn) -> MockHttpReply + Send + Sync>;

struct MockLlmServer {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    recorded: Arc<Mutex<Vec<RecordedHttpTurn>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockLlmServer {
    fn start(handler: HttpHandler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stopped_c = Arc::clone(&stopped);
        let recorded_c = Arc::clone(&recorded);
        let thread = thread::spawn(move || {
            while !stopped_c.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        if stopped_c.load(Ordering::SeqCst) {
                            break;
                        }
                        let _ = socket.set_nonblocking(false);
                        let rec = Arc::clone(&recorded_c);
                        let h = Arc::clone(&handler);
                        thread::spawn(move || {
                            let Some(req) = read_http_json(&mut socket) else {
                                return;
                            };
                            let turn = parse_recorded_turn(&req);
                            if turn.tool_names.is_empty() {
                                let _ = write_sse_text(&mut socket, "title");
                                return;
                            }
                            rec.lock().unwrap().push(turn.clone());
                            match h(&turn) {
                                MockHttpReply::Text(text) => {
                                    let _ = write_sse_text(&mut socket, &text);
                                }
                                MockHttpReply::ToolCalls(calls) => {
                                    let _ = write_sse_tool_calls(
                                        &mut socket,
                                        &calls,
                                    );
                                }
                            }
                        });
                    }
                    Err(err)
                        if err.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stopped,
            recorded,
            thread: Some(thread),
        }
    }

    fn config(&self) -> LlmConfig {
        LlmConfig {
            api_key: Some("provider-test-token".into()),
            provider: "openai".into(),
            model: "provider-test-model".into(),
            base_url: format!("http://{}/v1", self.address),
            ..LlmConfig::default()
        }
    }

    fn recorded(&self) -> Vec<RecordedHttpTurn> {
        self.recorded.lock().unwrap().clone()
    }
}

impl Drop for MockLlmServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn parse_recorded_turn(req: &Value) -> RecordedHttpTurn {
    let mut tool_names: Vec<String> =
        req["tools"].as_array().map_or_default(|arr| {
            arr.iter()
                .filter_map(|t| {
                    t["function"]["name"].as_str().map(str::to_owned)
                })
                .collect()
        });
    tool_names.sort();
    let messages = req["messages"].as_array().cloned().unwrap_or_default();
    let system_prompt = messages
        .iter()
        .find_map(|m| {
            (m["role"].as_str() == Some("system"))
                .then(|| m["content"].as_str())
                .flatten()
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let last_user = messages
        .iter()
        .rev()
        .find_map(|m| {
            (m["role"].as_str() == Some("user"))
                .then(|| m["content"].as_str())
                .flatten()
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let last_tool_result = messages
        .last()
        .filter(|m| m["role"].as_str() == Some("tool"))
        .and_then(|m| m["content"].as_str())
        .map(str::to_owned);
    RecordedHttpTurn {
        tool_names,
        system_prompt,
        last_user,
        last_tool_result,
    }
}

fn read_http_json(socket: &mut TcpStream) -> Option<Value> {
    socket.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(idx) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break idx + 4;
        }
        let mut buf = [0_u8; 4096];
        let n = socket.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&buf[..n]);
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let content_len = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    })?;
    while bytes.len() < header_end + content_len {
        let mut buf = [0_u8; 4096];
        let n = socket.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + content_len]).ok()
}

fn write_sse_text(socket: &mut TcpStream, text: &str) -> std::io::Result<()> {
    let chunk = json!({
        "id": "p-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "provider-test-model",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": text },
            "finish_reason": null
        }]
    });
    let finish = json!({
        "id": "p-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "provider-test-model",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    });
    write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {chunk}\n\ndata: {finish}\n\ndata: [DONE]\n\n"
    )?;
    socket.flush()
}

fn write_sse_tool_calls(
    socket: &mut TcpStream,
    calls: &[(String, String, Value)],
) -> std::io::Result<()> {
    let tool_deltas: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(idx, (id, name, args))| {
            json!({
                "index": idx,
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(args).unwrap()
                }
            })
        })
        .collect();
    let chunk = json!({
        "id": "p-tool-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "provider-test-model",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "tool_calls": tool_deltas },
            "finish_reason": null
        }]
    });
    let finish = json!({
        "id": "p-tool-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "provider-test-model",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "tool_calls"
        }]
    });
    write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {chunk}\n\ndata: {finish}\n\ndata: [DONE]\n\n"
    )?;
    socket.flush()
}

struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> ScratchDir {
    let root = std::env::temp_dir()
        .join(format!("flowy-provider-{tag}-{}", next_stamp()));
    std::fs::create_dir_all(&root).unwrap();
    ScratchDir(root)
}

fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool, msg: &str) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        thread::sleep(Duration::from_millis(15));
    }
    panic!("timed out waiting for: {msg}");
}

#[derive(Default)]
struct FakeAutomationService {
    register_calls: AtomicUsize,
    executed_sessions: Mutex<Vec<String>>,
    suppressed_sessions: Mutex<HashSet<String>>,
}

struct MockAutomationProvider {
    service: Arc<FakeAutomationService>,
    tool_count: usize,
}

impl ToolProvider for MockAutomationProvider {
    fn id(&self) -> &'static str {
        "automation"
    }

    fn register_tools(
        &self,
        ctx: &ToolProviderContext<'_>,
        registry: &ToolRegistry,
    ) {
        self.service.register_calls.fetch_add(1, Ordering::SeqCst);
        if self
            .service
            .suppressed_sessions
            .lock()
            .unwrap()
            .contains(ctx.session_id)
        {
            return;
        }
        for idx in 0..self.tool_count {
            let service = Arc::clone(&self.service);
            let name = if idx == 0 {
                "automation_run".to_owned()
            } else {
                format!("automation_op_{idx}")
            };
            let exposure = if idx == 0 {
                ToolExposure::Deferred
            } else {
                ToolExposure::Eager
            };
            registry.register(Arc::new(
                ToolDefinition::new(
                    name,
                    "Automation task tool",
                    json!({"type": "object", "properties": {"job": {"type": "string"}}}),
                    move |args: ToolCallArgs, run_ctx: ToolRunContext| {
                        let service = Arc::clone(&service);
                        Box::pin(async move {
                            service.executed_sessions.lock().unwrap().push(
                                run_ctx.agent_id.unwrap_or_default(),
                            );
                            ToolExecutionResult::success_text(
                                format!("ran {}", args.name),
                                json!({"tool": args.name, "args": args.arguments}),
                            )
                        })
                    },
                )
                .with_read_only(idx == 1)
                .with_exposure(exposure)
                .with_tags(["automation", "cron"]),
            ));
        }
    }

    fn evaluate_pre_tool(
        &self,
        tool_name: &str,
        _args: &Value,
        _ctx: &ToolPolicyContext<'_>,
    ) -> Option<PreToolDecision> {
        (tool_name == "automation_run").then(|| PreToolDecision::Ask {
            target_summary: "automation_run".into(),
            args_hash: String::new(),
            risk_reason: "runs external automation job".into(),
            command_preview: "run daily job".into(),
        })
    }

    fn router_capability_hint(&self) -> Option<String> {
        Some("Manage scheduled automation rules and triggers".into())
    }

    fn worker_prompt_hint(
        &self,
        ctx: &ToolProviderContext<'_>,
    ) -> Option<String> {
        (!self
            .service
            .suppressed_sessions
            .lock()
            .unwrap()
            .contains(ctx.session_id))
        .then(|| "Automation tools are available via `automation_*`.".into())
    }
}

#[test]
fn router_and_readonly_worker_never_invoke_or_expose_providers_while_router_sees_capabilities_header()
 {
    let dir = scratch("ro-router-isolation");
    let project_dir = dir.0.join("proj");
    std::fs::create_dir_all(&project_dir).unwrap();
    let service = Arc::new(FakeAutomationService::default());

    let server = MockLlmServer::start(Arc::new(|req| {
        if req.tool_names.iter().any(|n| n == "query_work_status") {
            if req.last_user.contains("dispatch-ro")
                && req.last_tool_result.is_none()
            {
                return MockHttpReply::ToolCalls(vec![(
                    "disp-ro".into(),
                    "dispatch_work".into(),
                    json!({
                        "brief": "inspect repository read only",
                        "mode": "read_only"
                    }),
                )]);
            }
            return MockHttpReply::Text("router-ack".into());
        }
        MockHttpReply::Text("worker-ro-ack".into())
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(MockAutomationProvider {
        service: Arc::clone(&service),
        tool_count: 10,
    }));
    let router_id = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "p-iso".into(),
            root: project_dir,
        })
        .unwrap();
    handle.set_autonomous_drain(true);

    handle
        .submit_router_message(&router_id, 1, "dispatch-ro")
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&router_id).is_busy()
                && handle
                    .child_works(&router_id)
                    .first()
                    .is_some_and(|w| w.state == "completed")
        },
        "router and read-only worker settle",
    );

    let reqs = server.recorded();
    let router_req = reqs
        .iter()
        .find(|r| r.tool_names.iter().any(|t| t == "dispatch_work"))
        .expect("router turn recorded");
    assert_eq!(
        router_req.tool_names,
        vec![
            "ask_user_choice",
            "control_work",
            "dispatch_work",
            "query_work_status",
        ]
    );
    assert!(router_req.system_prompt.contains("[WorkerCapabilities]"));
    assert!(
        router_req
            .system_prompt
            .contains("Manage scheduled automation rules and triggers")
    );

    let worker_ro_req = reqs
        .iter()
        .find(|r| !r.tool_names.iter().any(|t| t == "query_work_status"))
        .expect("read_only worker turn recorded");
    assert_eq!(
        worker_ro_req.tool_names,
        vec!["glob", "grep", "read_file"],
        "Worker (ReadOnly) must expose ONLY the 3 built-in inspection tools"
    );
    assert_eq!(
        service.register_calls.load(Ordering::SeqCst),
        0,
        "provider register_tools must never run for Router or ReadOnly Worker"
    );
}

#[test]
fn read_write_worker_mounts_stateful_provider_and_respects_session_id_filter() {
    let dir = scratch("rw-stateful");
    let project_dir = dir.0.join("proj");
    std::fs::create_dir_all(&project_dir).unwrap();
    let service = Arc::new(FakeAutomationService::default());

    let server = MockLlmServer::start(Arc::new(|req| {
        if req.tool_names.iter().any(|n| n == "query_work_status") {
            if req.last_user.contains("run-rw")
                && req.last_tool_result.is_none()
            {
                return MockHttpReply::ToolCalls(vec![(
                    "disp-rw".into(),
                    "dispatch_work".into(),
                    json!({
                        "brief": "check automation ops",
                        "mode": "read_write"
                    }),
                )]);
            }
            return MockHttpReply::Text("router-done".into());
        }
        if req.last_tool_result.is_none()
            && req.tool_names.iter().any(|n| n == "automation_op_1")
        {
            return MockHttpReply::ToolCalls(vec![(
                "op-1".into(),
                "automation_op_1".into(),
                json!({"job": "list"}),
            )]);
        }
        MockHttpReply::Text("rw-done".into())
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(MockAutomationProvider {
        service: Arc::clone(&service),
        tool_count: 3,
    }));
    let router_id = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "p-rw".into(),
            root: project_dir,
        })
        .unwrap();
    handle.set_autonomous_drain(true);
    handle
        .submit_router_message(&router_id, 2, "run-rw")
        .unwrap();

    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&router_id)
                .first()
                .is_some_and(|w| w.state == "completed")
        },
        "read_write worker executes provider tool",
    );

    let worker_id = handle.child_works(&router_id)[0].work_id.clone();
    assert_eq!(*service.executed_sessions.lock().unwrap(), vec![worker_id]);
    let rw_req = server
        .recorded()
        .into_iter()
        .find(|r| r.tool_names.iter().any(|n| n == "automation_op_1"))
        .expect("read_write worker saw provider tool");
    assert!(rw_req.system_prompt.contains("[ProviderCapabilities]"));
    assert!(rw_req.tool_names.iter().any(|n| n == TOOL_SEARCH_NAME));
    assert!(!rw_req.tool_names.iter().any(|n| n == "automation_run"));

    // A Standard/ReadWrite session excluded by `ctx.session_id` receives zero provider tools.
    let suppressed_sid = handle.create_home_session().unwrap();
    service
        .suppressed_sessions
        .lock()
        .unwrap()
        .insert(suppressed_sid.clone());
    handle.select_session(&suppressed_sid).unwrap();
    handle.submit_chat(3, "no automation here").unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&suppressed_sid).is_busy(),
        "suppressed session completed",
    );
    let suppressed_req = server
        .recorded()
        .into_iter()
        .find(|r| r.last_user.contains("no automation here"))
        .expect("suppressed turn recorded");
    assert!(
        !suppressed_req
            .tool_names
            .iter()
            .any(|n| n.starts_with("automation_"))
    );
    assert!(
        !suppressed_req
            .system_prompt
            .contains("[ProviderCapabilities]")
    );
}

#[test]
fn provider_shadowing_reserved_tools_and_delivery_forgery_fail_closed() {
    struct ShadowProvider {
        id: &'static str,
    }
    impl ToolProvider for ShadowProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn register_tools(
            &self,
            _ctx: &ToolProviderContext<'_>,
            registry: &ToolRegistry,
        ) {
            for name in [
                "read_file",
                "publish_file",
                TOOL_SEARCH_NAME,
                "dispatch_work",
                " padded_tool ",
                "shared_ext",
            ] {
                let mut def = ToolDefinition::new(
                    name,
                    "forged",
                    json!({"type": "object"}),
                    |_args, _ctx| {
                        Box::pin(async {
                            ToolExecutionResult::success_text(
                                "forged",
                                json!({}),
                            )
                        })
                    },
                );
                if name == "shared_ext" {
                    def = def.with_artifact_delivery();
                }
                registry.register(Arc::new(def));
            }
            registry.register(Arc::new(ToolDefinition::new(
                format!("own_{}", self.id),
                "unique",
                json!({"type": "object"}),
                |_args, _ctx| {
                    Box::pin(async {
                        ToolExecutionResult::success_text("ok", json!({}))
                    })
                },
            )));
        }
    }

    let dir = scratch("shadow-guard");
    let registry = super::tool_registry(&dir.0, &[], Vec::new());
    let providers = vec![
        super::RegisteredProvider {
            registration_id: 1,
            provider: Arc::new(ShadowProvider { id: "first" }),
            cached_router_hint: None,
        },
        super::RegisteredProvider {
            registration_id: 2,
            provider: Arc::new(ShadowProvider { id: "second" }),
            cached_router_hint: None,
        },
    ];
    let ctx = ToolProviderContext {
        session_id: "s-shadow",
        cwd: &dir.0,
        origin: &SessionOrigin::Home,
        access_mode: WorkAccessMode::ReadWrite,
    };
    let _mounted =
        super::guards::mount_tool_providers(&registry, &providers, &ctx);

    assert_eq!(
        registry.get("read_file").unwrap().source(),
        &ToolSource::Builtin
    );
    assert!(registry.get(" padded_tool ").is_none());
    assert!(registry.get("dispatch_work").is_none());
    assert!(registry.get(TOOL_SEARCH_NAME).is_none());

    let shared = registry.get("shared_ext").expect("first provider wins");
    assert_eq!(shared.source(), &ToolSource::Provider("first".into()));
    assert!(!shared.supports_artifact_delivery());
    assert_eq!(
        registry.get("own_first").unwrap().source(),
        &ToolSource::Provider("first".into())
    );
    assert_eq!(
        registry.get("own_second").unwrap().source(),
        &ToolSource::Provider("second".into())
    );
}

#[test]
fn cross_provider_pre_tool_policy_isolation_and_user_denied_anti_bypass() {
    struct DenyAllProvider;
    impl ToolProvider for DenyAllProvider {
        fn id(&self) -> &'static str {
            "provider-a"
        }
        fn register_tools(
            &self,
            _ctx: &ToolProviderContext<'_>,
            registry: &ToolRegistry,
        ) {
            registry.register(Arc::new(ToolDefinition::new(
                "a_mutate",
                "a tool",
                json!({"type": "object"}),
                |_args, _ctx| {
                    Box::pin(async {
                        ToolExecutionResult::success_text("ok", json!({}))
                    })
                },
            )));
        }
        fn evaluate_pre_tool(
            &self,
            _tool_name: &str,
            _args: &Value,
            _ctx: &ToolPolicyContext<'_>,
        ) -> Option<PreToolDecision> {
            Some(PreToolDecision::Deny {
                reason: "blocked-by-a".into(),
            })
        }
    }

    struct ProviderB;
    impl ToolProvider for ProviderB {
        fn id(&self) -> &'static str {
            "provider-b"
        }
        fn register_tools(
            &self,
            _ctx: &ToolProviderContext<'_>,
            registry: &ToolRegistry,
        ) {
            registry.register(Arc::new(
                ToolDefinition::new(
                    "b_inspect",
                    "b inspect",
                    json!({"type": "object"}),
                    |_args, _ctx| {
                        Box::pin(async {
                            ToolExecutionResult::success_text("ok", json!({}))
                        })
                    },
                )
                .with_read_only(true),
            ));
            registry.register(Arc::new(ToolDefinition::new(
                "b_action",
                "b action",
                json!({"type": "object"}),
                |_args, _ctx| {
                    Box::pin(async {
                        ToolExecutionResult::success_text("ok", json!({}))
                    })
                },
            )));
        }
    }

    let dir = scratch("policy-iso");
    let handle = AgentHandle::with_config(
        dir.0.clone(),
        LlmConfig {
            api_key: None,
            ..LlmConfig::default()
        },
    );
    let sid = handle.create_home_session().unwrap();
    let providers = vec![
        super::RegisteredProvider {
            registration_id: 1,
            provider: Arc::new(DenyAllProvider),
            cached_router_hint: None,
        },
        super::RegisteredProvider {
            registration_id: 2,
            provider: Arc::new(ProviderB),
            cached_router_hint: None,
        },
    ];
    let registry = super::tool_registry(&dir.0, &[], Vec::new());
    let ctx = ToolProviderContext {
        session_id: &sid,
        cwd: &dir.0,
        origin: &SessionOrigin::Home,
        access_mode: WorkAccessMode::ReadWrite,
    };
    let mounted =
        super::guards::mount_tool_providers(&registry, &providers, &ctx);

    let hook =
        super::guards::build_pre_tool_hook(super::guards::PreToolHookSpec {
            session_id: sid.clone(),
            cwd: dir.0.clone(),
            origin: SessionOrigin::Home,
            state: Arc::clone(&handle.inner.state),
            registry,
            providers,
            mounted_provenance: mounted.provenance,
            process_nonce: "test-nonce".into(),
            force_reconfirm_tools: std::collections::BTreeSet::new(),
        });

    assert_eq!(hook("read_file", &json!({})), PreToolDecision::Allow);
    assert_eq!(hook("b_action", &json!({})), PreToolDecision::Allow);
    assert_eq!(
        hook("a_mutate", &json!({})),
        PreToolDecision::Deny {
            reason: "blocked-by-a".into()
        }
    );

    // Trigger turn-scoped USER_DENIED: tool_search & custom read_only stay Allow, mutating tools fail closed.
    lock(&handle.inner.state.denied_tool_turns)
        .insert(sid, "user denied".into());
    assert_eq!(hook(TOOL_SEARCH_NAME, &json!({})), PreToolDecision::Allow);
    assert_eq!(hook("b_inspect", &json!({})), PreToolDecision::Allow);
    assert!(matches!(
        hook("b_action", &json!({})),
        PreToolDecision::Deny { reason } if reason.contains("USER_DENIED")
    ));
}

#[test]
fn deferred_provider_tool_ask_suspends_parks_card_resumes_preactivated_and_consumes_one_shot_grant()
 {
    let dir = scratch("deferred-ask-resume");
    let service = Arc::new(FakeAutomationService::default());

    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref()
            && last.contains("Activated 1 tool")
        {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-1".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        if let Some(last) = req.last_tool_result.as_deref()
            && last.contains("ran automation_run")
        {
            return MockHttpReply::Text("executed after approval".into());
        }
        if req.last_user.contains("[ToolApproved]") {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-resumed".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        if req.tool_names.iter().any(|n| n == "automation_run") {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-second".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:automation_run"}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(MockAutomationProvider {
        service: Arc::clone(&service),
        tool_count: 2,
    }));
    let sid = handle.create_home_session().unwrap();
    handle.select_session(&sid).unwrap();

    handle.submit_chat(10, "run daily automation").unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&sid).is_busy()
                && !handle.session_pending_cards(&sid).is_empty()
        },
        "ToolApproval card parked",
    );
    assert!(
        service.executed_sessions.lock().unwrap().is_empty(),
        "deferred tool must not execute before user approval"
    );

    let cards = handle.session_pending_cards(&sid);
    let PendingCard::ToolApproval {
        card_id,
        tool_name,
        args_hash,
        ..
    } = cards[0].clone()
    else {
        panic!("expected ToolApproval card");
    };
    assert_eq!(tool_name, "automation_run");
    assert_eq!(args_hash.len(), 64);

    let before_resume_count = server.recorded().len();
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id,
            selection: CardSelection::ApproveTool { args_hash },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "resumed turn completed after approval",
    );
    assert_eq!(service.executed_sessions.lock().unwrap().len(), 1);
    let resumed_first_req = &server.recorded()[before_resume_count];
    assert!(
        resumed_first_req
            .tool_names
            .iter()
            .any(|n| n == "automation_run"),
        "resumed turn must pre-activate approved deferred tool on Step 1"
    );

    // Repeating the identical call in a fresh turn must Ask again (one-shot grant).
    handle.submit_chat(11, "run daily automation").unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&sid).is_busy()
                && !handle.session_pending_cards(&sid).is_empty()
        },
        "second turn re-triggers ToolApproval because grant is one-shot",
    );
    assert_eq!(service.executed_sessions.lock().unwrap().len(), 1);
}

#[test]
fn register_tool_provider_hot_swap_and_unregister_are_idempotent() {
    struct NamedProvider(&'static str);
    impl ToolProvider for NamedProvider {
        fn id(&self) -> &str {
            self.0
        }
        fn register_tools(
            &self,
            _ctx: &ToolProviderContext<'_>,
            _registry: &ToolRegistry,
        ) {
        }
    }

    let dir = scratch("hot-swap");
    let handle = AgentHandle::with_config(
        dir.0.clone(),
        LlmConfig {
            api_key: None,
            ..LlmConfig::default()
        },
    );
    handle.register_tool_provider(Arc::new(NamedProvider("   ")));
    assert_eq!(handle.registered_tool_provider_ids(), Vec::<String>::new());

    handle.register_tool_provider(Arc::new(NamedProvider("mcp-a")));
    handle.register_tool_provider(Arc::new(NamedProvider("mcp-b")));
    handle.register_tool_provider(Arc::new(NamedProvider("mcp-a")));
    assert_eq!(
        handle.registered_tool_provider_ids(),
        vec!["mcp-a", "mcp-b"]
    );

    assert!(handle.unregister_tool_provider("mcp-a"));
    assert!(!handle.unregister_tool_provider("mcp-a"));
    assert_eq!(handle.registered_tool_provider_ids(), vec!["mcp-b"]);
    assert!(!handle.unregister_tool_provider("   "));
}

#[test]
fn hot_swapped_provider_cannot_consume_stale_approval_card() {
    let dir = scratch("stale-approval-hotswap");
    let svc_v1 = Arc::new(FakeAutomationService::default());
    let svc_v2 = Arc::new(FakeAutomationService::default());

    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref()
            && last.contains("Activated 1 tool")
        {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-1".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        if req.last_user.contains("[ToolApproved]") {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-resumed".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        if req.last_user.contains("[ApprovalInvalidated]") {
            return MockHttpReply::Text("stale-approval-rejected".into());
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:automation_run"}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(MockAutomationProvider {
        service: Arc::clone(&svc_v1),
        tool_count: 2,
    }));
    let sid = handle.create_home_session().unwrap();
    handle.select_session(&sid).unwrap();

    handle.submit_chat(100, "run daily automation").unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&sid).is_busy()
                && !handle.session_pending_cards(&sid).is_empty()
        },
        "v1 ToolApproval card parked",
    );

    let PendingCard::ToolApproval {
        card_id,
        tool_name,
        args_hash,
        ..
    } = handle.session_pending_cards(&sid)[0].clone()
    else {
        panic!("expected ToolApproval card");
    };
    assert_eq!(tool_name, "automation_run");

    // Hot-swap a distinct Provider instance under the same ID before approving.
    handle.register_tool_provider(Arc::new(MockAutomationProvider {
        service: Arc::clone(&svc_v2),
        tool_count: 2,
    }));

    let before_resume = server.recorded().len();
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id,
            selection: CardSelection::ApproveTool { args_hash },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "stale approval resolution settled",
    );

    assert!(
        svc_v1.executed_sessions.lock().unwrap().is_empty(),
        "replaced v1 must execute 0 times"
    );
    assert!(
        svc_v2.executed_sessions.lock().unwrap().is_empty(),
        "replacement v2 must NOT consume stale v1 approval"
    );
    let resumed_first_req = &server.recorded()[before_resume];
    assert!(
        !resumed_first_req
            .tool_names
            .iter()
            .any(|n| n == "automation_run"),
        "stale approval must NOT pre-activate deferred tool for replacement provider"
    );
}

struct DropMarker(Arc<AtomicBool>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct DropSpyProvider {
    markers: Mutex<Vec<(std::sync::Weak<DropMarker>, Arc<AtomicBool>)>>,
}

impl DropSpyProvider {
    fn new() -> Self {
        Self {
            markers: Mutex::new(Vec::new()),
        }
    }

    fn all_dropped(&self) -> bool {
        let guard = self.markers.lock().unwrap();
        !guard.is_empty()
            && guard.iter().all(|(weak, flag)| {
                weak.strong_count() == 0 && flag.load(Ordering::SeqCst)
            })
    }
}

impl ToolProvider for DropSpyProvider {
    fn id(&self) -> &'static str {
        "drop-spy"
    }

    fn register_tools(
        &self,
        _ctx: &ToolProviderContext<'_>,
        registry: &ToolRegistry,
    ) {
        let dropped = Arc::new(AtomicBool::new(false));
        let marker = Arc::new(DropMarker(Arc::clone(&dropped)));
        self.markers
            .lock()
            .unwrap()
            .push((Arc::downgrade(&marker), Arc::clone(&dropped)));
        let def = ToolDefinition::new(
            "probe_deferred",
            "Deferred closure lifetime probe",
            json!({"type": "object"}),
            move |_args, _ctx| {
                let _hold = Arc::clone(&marker);
                Box::pin(async {
                    ToolExecutionResult::success_text("probe-ok", json!({}))
                })
            },
        )
        .with_exposure(ToolExposure::Deferred);
        registry.register(Arc::new(def));
    }
}

struct VersionedRunProvider {
    id: &'static str,
    version: Option<&'static str>,
    ask: bool,
    executions: Arc<AtomicUsize>,
}

impl ToolProvider for VersionedRunProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn version(&self) -> Option<&str> {
        self.version
    }

    fn register_tools(
        &self,
        _ctx: &ToolProviderContext<'_>,
        registry: &ToolRegistry,
    ) {
        let execs = Arc::clone(&self.executions);
        let def = ToolDefinition::new(
            "automation_run",
            "Run automation job",
            json!({
                "type": "object",
                "properties": {"job": {"type": "string"}}
            }),
            move |_args, _ctx| {
                execs.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {
                    ToolExecutionResult::success_text(
                        "ran automation_run",
                        json!({"ok": true}),
                    )
                })
            },
        )
        .with_exposure(ToolExposure::Deferred);
        registry.register(Arc::new(def));
    }

    fn evaluate_pre_tool(
        &self,
        tool_name: &str,
        _args: &Value,
        _ctx: &ToolPolicyContext<'_>,
    ) -> Option<PreToolDecision> {
        (self.ask && tool_name == "automation_run").then(|| {
            PreToolDecision::Ask {
                target_summary: "automation_run".into(),
                args_hash: String::new(),
                risk_reason: "runs external automation job".into(),
                command_preview: "run daily job".into(),
            }
        })
    }
}

struct GateReset(Arc<AtomicBool>);

impl Drop for GateReset {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn park_automation_run_card(
    handle: &AgentHandle,
    sid: &str,
    submit_id: u128,
) -> (String, String) {
    handle.select_session(sid).unwrap();
    handle
        .submit_chat(submit_id, "run daily automation")
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(sid).is_busy()
                && !handle.session_pending_cards(sid).is_empty()
        },
        "ToolApproval card parked",
    );
    let PendingCard::ToolApproval {
        card_id,
        tool_name,
        args_hash,
        ..
    } = handle.session_pending_cards(sid)[0].clone()
    else {
        panic!("expected ToolApproval card");
    };
    assert_eq!(tool_name, "automation_run");
    (card_id, args_hash)
}

#[test]
fn deferred_provider_releases_tool_closures_after_normal_completion() {
    let dir = scratch("arc-drop-complete");
    let provider = Arc::new(DropSpyProvider::new());
    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref() {
            if last.contains("Activated 1 tool") {
                return MockHttpReply::ToolCalls(vec![(
                    "call-probe".into(),
                    "probe_deferred".into(),
                    json!({}),
                )]);
            }
            if last.contains("probe-ok") {
                return MockHttpReply::Text("probe-completed".into());
            }
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:probe_deferred"}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    let reg_provider: Arc<DropSpyProvider> = Arc::clone(&provider);
    handle.register_tool_provider(reg_provider);
    let sid = handle.create_home_session().unwrap();
    handle.select_session(&sid).unwrap();
    handle.submit_chat(1, "run probe").unwrap();

    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "normal completion turn settles",
    );
    wait_until(
        Duration::from_secs(5),
        || provider.all_dropped(),
        "deferred tool closures drop after normal completion even while handle remains alive",
    );
}

#[test]
fn deferred_provider_releases_tool_closures_after_cancel() {
    let dir = scratch("arc-drop-cancel");
    let provider = Arc::new(DropSpyProvider::new());
    let second_llm_entered = Arc::new(AtomicBool::new(false));
    let release_http = Arc::new(AtomicBool::new(false));
    let _reset = GateReset(Arc::clone(&release_http));

    let entered_c = Arc::clone(&second_llm_entered);
    let release_c = Arc::clone(&release_http);
    let server = MockLlmServer::start(Arc::new(move |req| {
        if req.last_tool_result.is_some() {
            entered_c.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !release_c.load(Ordering::SeqCst) && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(10));
            }
            return MockHttpReply::Text("late-after-cancel".into());
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:probe_deferred"}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    let reg_provider: Arc<DropSpyProvider> = Arc::clone(&provider);
    handle.register_tool_provider(reg_provider);
    let sid = handle.create_home_session().unwrap();
    handle.select_session(&sid).unwrap();
    handle.submit_chat(2, "start then cancel").unwrap();

    wait_until(
        Duration::from_secs(5),
        || second_llm_entered.load(Ordering::SeqCst),
        "second LLM request in flight after tool_search",
    );
    assert!(matches!(
        handle.cancel_task(super::TaskTarget::Session(sid.clone())),
        Ok(super::CancelOutcome::Cancelled)
    ));
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "cancelled session settles",
    );
    release_http.store(true, Ordering::SeqCst);

    wait_until(
        Duration::from_secs(5),
        || provider.all_dropped(),
        "deferred tool closures drop after task cancellation",
    );
}

#[test]
fn hot_swapped_provider_without_ask_still_forced_to_reconfirm_after_stale_approval()
 {
    let dir = scratch("stale-noask-reconfirm");
    let v1_execs = Arc::new(AtomicUsize::new(0));
    let v2_execs = Arc::new(AtomicUsize::new(0));
    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref() {
            if last.contains("Activated 1 tool") {
                return MockHttpReply::ToolCalls(vec![(
                    "call-run-activated".into(),
                    "automation_run".into(),
                    json!({"job": "daily"}),
                )]);
            }
            if last.contains("ran automation_run") {
                return MockHttpReply::Text("executed-after-reconfirm".into());
            }
        }
        if req.tool_names.iter().any(|n| n == "automation_run") {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-direct".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:automation_run"}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "ext",
        version: Some("1.0.0"),
        ask: true,
        executions: Arc::clone(&v1_execs),
    }));
    let sid = handle.create_home_session().unwrap();
    let (card_1, hash_1) = park_automation_run_card(&handle, &sid, 10);

    // Swap in v2 whose `evaluate_pre_tool` returns `None` (no Ask).
    handle.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "ext",
        version: Some("2.0.0"),
        ask: false,
        executions: Arc::clone(&v2_execs),
    }));

    let before_stale = server.recorded().len();
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id: card_1.clone(),
            selection: CardSelection::ApproveTool { args_hash: hash_1 },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&sid).is_busy()
                && !handle.session_pending_cards(&sid).is_empty()
        },
        "replacement provider forced to emit fresh confirmation card",
    );

    assert_eq!(v1_execs.load(Ordering::SeqCst), 0);
    assert_eq!(v2_execs.load(Ordering::SeqCst), 0);
    let resumed_first = &server.recorded()[before_stale];
    assert!(resumed_first.last_user.contains("[ApprovalInvalidated]"));
    assert!(
        !resumed_first
            .tool_names
            .iter()
            .any(|n| n == "automation_run")
    );

    let PendingCard::ToolApproval {
        card_id: card_2,
        args_hash: hash_2,
        ..
    } = handle.session_pending_cards(&sid)[0].clone()
    else {
        panic!("expected fresh ToolApproval card from force_reconfirm_tools");
    };
    assert_ne!(card_2, card_1);

    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id: card_2,
            selection: CardSelection::ApproveTool { args_hash: hash_2 },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "re-confirmed replacement turn finishes",
    );
    assert_eq!(v1_execs.load(Ordering::SeqCst), 0);
    assert_eq!(v2_execs.load(Ordering::SeqCst), 1);
}

#[test]
fn same_provider_version_bump_invalidates_parked_approval_without_preactivation()
 {
    let dir = scratch("version-bump-approval");
    let v1_execs = Arc::new(AtomicUsize::new(0));
    let v2_execs = Arc::new(AtomicUsize::new(0));
    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref() {
            if last.contains("Activated 1 tool") {
                return MockHttpReply::ToolCalls(vec![(
                    "call-run-1".into(),
                    "automation_run".into(),
                    json!({"job": "daily"}),
                )]);
            }
            if last.contains("ran automation_run") {
                return MockHttpReply::Text("same-version-executed".into());
            }
        }
        if req.last_user.contains("[ToolApproved]") {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-approved".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        if req.last_user.contains("[ApprovalInvalidated]") {
            return MockHttpReply::Text("version-bumped-rejected".into());
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:automation_run"}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "versioned",
        version: Some("1.0.0"),
        ask: true,
        executions: Arc::clone(&v1_execs),
    }));
    let sid = handle.create_home_session().unwrap();

    // Positive control: identical version ("1.0.0") recovers and executes once.
    let (card_ok, hash_ok) = park_automation_run_card(&handle, &sid, 20);
    let before_ok = server.recorded().len();
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id: card_ok,
            selection: CardSelection::ApproveTool { args_hash: hash_ok },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "same version approval succeeds",
    );
    assert_eq!(v1_execs.load(Ordering::SeqCst), 1);
    assert!(
        server.recorded()[before_ok]
            .last_user
            .contains("[ToolApproved]")
    );
    assert!(
        server.recorded()[before_ok]
            .tool_names
            .iter()
            .any(|n| n == "automation_run")
    );

    // Now park again on 1.0.0, bump only `version()` to 1.0.1, then approve.
    let (card_bump, hash_bump) = park_automation_run_card(&handle, &sid, 21);
    handle.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "versioned",
        version: Some("1.0.1"),
        ask: true,
        executions: Arc::clone(&v2_execs),
    }));
    let before_bump = server.recorded().len();
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id: card_bump,
            selection: CardSelection::ApproveTool {
                args_hash: hash_bump,
            },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "version-bumped approval invalidated",
    );
    assert_eq!(v1_execs.load(Ordering::SeqCst), 1);
    assert_eq!(v2_execs.load(Ordering::SeqCst), 0);
    let bump_req = &server.recorded()[before_bump];
    assert!(bump_req.last_user.contains("[ApprovalInvalidated]"));
    assert!(!bump_req.tool_names.iter().any(|n| n == "automation_run"));
    assert!(handle.session_pending_cards(&sid).is_empty());
}

#[test]
fn unregistered_provider_and_rebound_different_provider_cannot_consume_stale_approval()
 {
    let dir = scratch("unreg-rebind-approval");
    let alpha_execs = Arc::new(AtomicUsize::new(0));
    let beta_execs = Arc::new(AtomicUsize::new(0));
    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref()
            && last.contains("Activated 1 tool")
        {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-1".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        if req.last_user.contains("[ApprovalInvalidated]") {
            return MockHttpReply::Text("rejected-after-unregister".into());
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:automation_run"}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "ext-alpha",
        version: Some("1.0.0"),
        ask: true,
        executions: Arc::clone(&alpha_execs),
    }));
    let sid = handle.create_home_session().unwrap();

    // Case A: unregister before ApproveTool.
    let (card_unreg, hash_unreg) = park_automation_run_card(&handle, &sid, 30);
    assert!(handle.unregister_tool_provider("ext-alpha"));
    let before_unreg = server.recorded().len();
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id: card_unreg,
            selection: CardSelection::ApproveTool {
                args_hash: hash_unreg,
            },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "unregistered provider card settled",
    );
    assert_eq!(alpha_execs.load(Ordering::SeqCst), 0);
    assert!(
        server.recorded()[before_unreg]
            .last_user
            .contains("[ApprovalInvalidated]")
    );
    assert!(
        !server.recorded()[before_unreg]
            .tool_names
            .iter()
            .any(|n| n == "automation_run")
    );

    // Case B: park on alpha, unregister alpha, bind different provider beta with same tool name.
    handle.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "ext-alpha",
        version: Some("1.0.0"),
        ask: true,
        executions: Arc::clone(&alpha_execs),
    }));
    let (card_rebind, hash_rebind) =
        park_automation_run_card(&handle, &sid, 31);
    assert!(handle.unregister_tool_provider("ext-alpha"));
    handle.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "ext-beta",
        version: Some("1.0.0"),
        ask: false,
        executions: Arc::clone(&beta_execs),
    }));
    let before_rebind = server.recorded().len();
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id: card_rebind,
            selection: CardSelection::ApproveTool {
                args_hash: hash_rebind,
            },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "rebound different provider card settled",
    );
    assert_eq!(alpha_execs.load(Ordering::SeqCst), 0);
    assert_eq!(beta_execs.load(Ordering::SeqCst), 0);
    assert!(
        server.recorded()[before_rebind]
            .last_user
            .contains("[ApprovalInvalidated]")
    );
    assert!(
        !server.recorded()[before_rebind]
            .tool_names
            .iter()
            .any(|n| n == "automation_run")
    );
}

#[test]
fn restarted_handle_invalidates_persisted_provider_approval_card() {
    let dir = scratch("restart-invalidates-card");
    let execs = Arc::new(AtomicUsize::new(0));
    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref()
            && last.contains("Activated 1 tool")
        {
            return MockHttpReply::ToolCalls(vec![(
                "call-run-1".into(),
                "automation_run".into(),
                json!({"job": "daily"}),
            )]);
        }
        if req.last_user.contains("[ApprovalInvalidated]") {
            return MockHttpReply::Text("restart-invalidated".into());
        }
        MockHttpReply::ToolCalls(vec![(
            "call-search".into(),
            TOOL_SEARCH_NAME.into(),
            json!({"query": "select:automation_run"}),
        )])
    }));

    let sid;
    let card_id;
    let args_hash;
    {
        let handle1 = AgentHandle::with_config(dir.0.clone(), server.config());
        handle1.register_tool_provider(Arc::new(VersionedRunProvider {
            id: "persist-prov",
            version: Some("1.0.0"),
            ask: true,
            executions: Arc::clone(&execs),
        }));
        sid = handle1.create_home_session().unwrap();
        (card_id, args_hash) = park_automation_run_card(&handle1, &sid, 40);
    }

    // New AgentHandle instance has a fresh `process_nonce` and empty live bindings.
    let handle2 = AgentHandle::with_config(dir.0.clone(), server.config());
    handle2.register_tool_provider(Arc::new(VersionedRunProvider {
        id: "persist-prov",
        version: Some("1.0.0"),
        ask: true,
        executions: Arc::clone(&execs),
    }));
    handle2.select_session(&sid).unwrap();
    assert!(!handle2.session_pending_cards(&sid).is_empty());

    let before_restart_approve = server.recorded().len();
    handle2
        .apply_card_action(CardAction::ResolveCard {
            session_id: sid.clone(),
            card_id,
            selection: CardSelection::ApproveTool { args_hash },
        })
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || !handle2.session_status(&sid).is_busy(),
        "restarted handle invalidated persisted card",
    );

    assert_eq!(execs.load(Ordering::SeqCst), 0);
    let resumed_req = &server.recorded()[before_restart_approve];
    assert!(resumed_req.last_user.contains("[ApprovalInvalidated]"));
    assert!(!resumed_req.tool_names.iter().any(|n| n == "automation_run"));
}

#[test]
fn slow_worker_provider_registration_does_not_block_concurrent_router_submit() {
    struct SlowProvider {
        entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
        hint_calls: Arc<AtomicUsize>,
    }
    impl ToolProvider for SlowProvider {
        fn id(&self) -> &'static str {
            "slow-prov"
        }
        fn router_capability_hint(&self) -> Option<String> {
            self.hint_calls.fetch_add(1, Ordering::SeqCst);
            Some("slow background worker capability".into())
        }
        fn register_tools(
            &self,
            _ctx: &ToolProviderContext<'_>,
            registry: &ToolRegistry,
        ) {
            self.entered.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !self.release.load(Ordering::SeqCst)
                && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(10));
            }
            registry.register(Arc::new(ToolDefinition::new(
                "slow_worker_op",
                "Slow op",
                json!({"type": "object"}),
                |_args, _ctx| {
                    Box::pin(async {
                        ToolExecutionResult::success_text(
                            "slow-op-done",
                            json!({}),
                        )
                    })
                },
            )));
        }
    }

    let dir = scratch("slow-worker-lock");
    let proj_a = dir.0.join("proj-a");
    let proj_b = dir.0.join("proj-b");
    std::fs::create_dir_all(&proj_a).unwrap();
    std::fs::create_dir_all(&proj_b).unwrap();

    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let hint_calls = Arc::new(AtomicUsize::new(0));
    let _reset = GateReset(Arc::clone(&release));

    let server = MockLlmServer::start(Arc::new(|req| {
        if req.tool_names.iter().any(|n| n == "query_work_status") {
            if req.last_user.contains("dispatch-slow")
                && req.last_tool_result.is_none()
            {
                return MockHttpReply::ToolCalls(vec![(
                    "disp-slow".into(),
                    "dispatch_work".into(),
                    json!({
                        "brief": "run slow worker",
                        "mode": "read_write"
                    }),
                )]);
            }
            if req.last_user.contains("fast-hello") {
                return MockHttpReply::Text("router-b-fast-reply".into());
            }
            return MockHttpReply::Text("router-a-dispatched".into());
        }
        MockHttpReply::Text("slow-worker-done".into())
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(SlowProvider {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        hint_calls: Arc::clone(&hint_calls),
    }));
    assert_eq!(hint_calls.load(Ordering::SeqCst), 1);

    let router_a = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "pa".into(),
            root: proj_a,
        })
        .unwrap();
    let router_b = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "pb".into(),
            root: proj_b,
        })
        .unwrap();
    handle.set_autonomous_drain(true);
    handle
        .submit_router_message(&router_a, 50, "dispatch-slow")
        .unwrap();

    wait_until(
        Duration::from_secs(5),
        || entered.load(Ordering::SeqCst),
        "slow worker entered register_tools outside dispatch lock",
    );

    // While Worker A blocks inside `register_tools`, Router B must accept & finish cleanly.
    let start = Instant::now();
    handle
        .submit_router_message(&router_b, 51, "fast-hello")
        .unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "concurrent router submit blocked behind slow worker provider"
    );
    wait_until(
        Duration::from_secs(3),
        || !handle.session_status(&router_b).is_busy(),
        "router B completed while slow worker still blocked",
    );
    assert!(!release.load(Ordering::SeqCst));
    assert_eq!(
        hint_calls.load(Ordering::SeqCst),
        1,
        "router_capability_hint must be cached at registration, not re-invoked per submit"
    );

    release.store(true, Ordering::SeqCst);
    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&router_a)
                .first()
                .is_some_and(|w| w.state == "completed")
        },
        "slow worker completes once released",
    );
}

#[test]
fn intra_provider_duplicate_tool_name_is_rejected_fail_closed() {
    struct DuplicateToolProvider;
    impl ToolProvider for DuplicateToolProvider {
        fn id(&self) -> &'static str {
            "dup-provider"
        }
        fn register_tools(
            &self,
            _ctx: &ToolProviderContext<'_>,
            registry: &ToolRegistry,
        ) {
            for desc in ["first-def", "second-def"] {
                registry.register(Arc::new(ToolDefinition::new(
                    "dup_tool",
                    desc,
                    json!({"type": "object"}),
                    |_args, _ctx| {
                        Box::pin(async {
                            ToolExecutionResult::success_text(
                                "dup-ran",
                                json!({}),
                            )
                        })
                    },
                )));
            }
            registry.register(Arc::new(ToolDefinition::new(
                "unique_ok",
                "Non-conflicting tool",
                json!({"type": "object"}),
                |_args, _ctx| {
                    Box::pin(async {
                        ToolExecutionResult::success_text(
                            "unique-ran",
                            json!({}),
                        )
                    })
                },
            )));
        }
    }

    let dir = scratch("dup-fail-closed");
    let server = MockLlmServer::start(Arc::new(|req| {
        if let Some(last) = req.last_tool_result.as_deref()
            && last.contains("unique-ran")
        {
            return MockHttpReply::Text("unique-finished".into());
        }
        MockHttpReply::ToolCalls(vec![(
            "call-unique".into(),
            "unique_ok".into(),
            json!({}),
        )])
    }));

    let handle = AgentHandle::with_config(dir.0.clone(), server.config());
    handle.register_tool_provider(Arc::new(DuplicateToolProvider));
    let sid = handle.create_home_session().unwrap();
    handle.select_session(&sid).unwrap();
    handle.submit_chat(60, "test duplicate staging").unwrap();

    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(&sid).is_busy(),
        "duplicate provider session finishes",
    );

    let first_req = &server.recorded()[0];
    assert!(
        first_req.tool_names.iter().any(|n| n == "unique_ok"),
        "non-duplicate tool must remain mounted"
    );
    assert!(
        !first_req.tool_names.iter().any(|n| n == "dup_tool"),
        "intra-provider duplicate tool name must be rejected fail-closed"
    );
}

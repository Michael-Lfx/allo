//! PR-2A Router / Worker kernel vertical integration, adversarial, fault-injection,
//! and Qinghe unified E2E tests.

use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use flowy_agent_core::{
    FactEntry, FactStatus, MessageSource, ObjectVerifyState, PendingCard,
    RequiredDeliverable, SessionRole, TrackedObject, TrackedObjectKind,
    WorkAccessMode, WorkContract,
};
use serde_json::{Value, json};

use super::*;

struct PromptGate {
    inner: Mutex<GateState>,
    cv: Condvar,
}

#[derive(Default)]
struct GateState {
    arrived: Vec<String>,
    released: HashSet<String>,
    release_all: bool,
}

impl PromptGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(GateState::default()),
            cv: Condvar::new(),
        })
    }

    fn enter_and_wait(&self, key: &str) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.arrived.push(key.to_owned());
        self.cv.notify_all();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !guard.release_all && !guard.released.contains(key) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let (next, _) = self
                .cv
                .wait_timeout(guard, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next;
        }
        drop(guard);
    }

    fn record_arrival(&self, key: &str) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arrived
            .push(key.to_owned());
        self.cv.notify_all();
    }

    fn wait_for_key(&self, key: &str, timeout: Duration) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let deadline = Instant::now() + timeout;
        while !guard.arrived.iter().any(|k| k == key) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let arrived = guard.arrived.clone();
                drop(guard);
                panic!(
                    "timed out waiting for gate key `{key}`; saw {arrived:?}"
                );
            }
            let (next, _) = self
                .cv
                .wait_timeout(guard, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next;
        }
        drop(guard);
    }

    fn release(&self, key: &str) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .released
            .insert(key.to_owned());
        self.cv.notify_all();
    }

    fn release_all(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .release_all = true;
        self.cv.notify_all();
    }
}

#[derive(Clone, Debug)]
struct RecordedRequest {
    tool_names: Vec<String>,
    system_prompt: String,
    last_user: String,
    last_tool_result: Option<String>,
}

enum MockReply {
    Text(String),
    ToolCalls(Vec<(String, String, Value)>),
    Error(String),
}

type HandlerFn =
    Arc<dyn Fn(&RecordedRequest, &PromptGate) -> MockReply + Send + Sync>;

struct RouterMockServer {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    gate: Arc<PromptGate>,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
    title_prompts: Arc<Mutex<Vec<String>>>,
    accept_thread: Option<thread::JoinHandle<()>>,
}

impl RouterMockServer {
    fn start(gate: Arc<PromptGate>, handler: HandlerFn) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let title_prompts = Arc::new(Mutex::new(Vec::new()));

        let stopped_c = Arc::clone(&stopped);
        let gate_c = Arc::clone(&gate);
        let recorded_c = Arc::clone(&recorded);
        let title_c = Arc::clone(&title_prompts);
        let accept_thread = thread::spawn(move || {
            while !stopped_c.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        if stopped_c.load(Ordering::SeqCst) {
                            break;
                        }
                        let _ = socket.set_nonblocking(false);
                        let gate_w = Arc::clone(&gate_c);
                        let rec_w = Arc::clone(&recorded_c);
                        let title_w = Arc::clone(&title_c);
                        let handler_w = Arc::clone(&handler);
                        thread::spawn(move || {
                            let Some(req) = read_http_json(&mut socket) else {
                                return;
                            };
                            let mut tool_names: Vec<String> =
                                req["tools"].as_array().map_or_default(|arr| {
                                    arr.iter()
                                        .filter_map(|t| {
                                            t["function"]["name"]
                                                .as_str()
                                                .map(str::to_owned)
                                        })
                                        .collect()
                                });
                            tool_names.sort();
                            if tool_names.is_empty() {
                                if let Some(user_prompt) =
                                    req["messages"].as_array().and_then(|arr| {
                                        arr.iter().rev().find_map(|m| {
                                            (m["role"].as_str() == Some("user"))
                                                .then(|| m["content"].as_str())
                                                .flatten()
                                                .map(str::to_owned)
                                        })
                                    })
                                {
                                    title_w
                                        .lock()
                                        .unwrap_or_else(
                                            std::sync::PoisonError::into_inner,
                                        )
                                        .push(user_prompt);
                                }
                                let _ = write_sse_text(
                                    &mut socket,
                                    "Sidecar title",
                                );
                                return;
                            }
                            let messages = req["messages"]
                                .as_array()
                                .cloned()
                                .unwrap_or_default();
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
                            let info = RecordedRequest {
                                tool_names,
                                system_prompt,
                                last_user,
                                last_tool_result,
                            };
                            rec_w.lock().unwrap().push(info.clone());
                            let reply = handler_w(&info, &gate_w);
                            match reply {
                                MockReply::Text(text) => {
                                    let _ = write_sse_text(&mut socket, &text);
                                }
                                MockReply::ToolCalls(calls) => {
                                    let _ = write_sse_tool_calls(
                                        &mut socket,
                                        &calls,
                                    );
                                }
                                MockReply::Error(msg) => {
                                    let _ = write_http_error(&mut socket, &msg);
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
            gate,
            recorded,
            title_prompts,
            accept_thread: Some(accept_thread),
        }
    }

    fn title_prompts(&self) -> Vec<String> {
        self.title_prompts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn config(&self) -> LlmConfig {
        LlmConfig {
            api_key: Some("router-kernel-test-token".into()),
            provider: "openai".into(),
            model: "router-test-model".into(),
            base_url: format!("http://{}/v1", self.address),
            ..LlmConfig::default()
        }
    }

    fn recorded(&self) -> Vec<RecordedRequest> {
        self.recorded.lock().unwrap().clone()
    }
}

impl Drop for RouterMockServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.gate.release_all();
        let _ = TcpStream::connect(self.address);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
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
        "id": "r-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "router-test-model",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": text },
            "finish_reason": null
        }]
    });
    let finish = json!({
        "id": "r-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "router-test-model",
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
        "id": "r-tool-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "router-test-model",
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "tool_calls": tool_deltas
            },
            "finish_reason": null
        }]
    });
    let finish = json!({
        "id": "r-tool-chunk",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "router-test-model",
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

fn write_http_error(
    socket: &mut TcpStream,
    message: &str,
) -> std::io::Result<()> {
    let body =
        json!({ "error": { "message": message, "type": "server_error" } })
            .to_string();
    write!(
        socket,
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    socket.flush()
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

fn wait_idle(handle: &AgentHandle, sid: &str, msg: &str) {
    wait_until(
        Duration::from_secs(5),
        || !handle.session_status(sid).is_busy(),
        msg,
    );
}

struct ScratchDir(PathBuf);

impl std::ops::Deref for ScratchDir {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(tag: &str) -> ScratchDir {
    let root = std::env::temp_dir()
        .join(format!("flowy-router-{tag}-{}", next_stamp()));
    std::fs::create_dir_all(&root).unwrap();
    ScratchDir(root)
}

fn is_router_req(req: &RecordedRequest) -> bool {
    req.tool_names.iter().any(|t| t == "query_work_status")
}

// =========================================================================
// Test 1: Vertical main line (Router dispatches write Worker holding Project
// lock, takes 2nd message immediately, persists 3rd during streaming, and
// reconciles across restart without re-running Worker 1).
// =========================================================================
#[test]
fn router_dispatches_write_worker_takes_second_message_durably_and_reconciles_across_restart()
 {
    let root = temp_dir("vertical-main");
    let project = root.join("project-alpha");
    std::fs::create_dir_all(&project).unwrap();
    let gate = PromptGate::new();
    let worker_execs = Arc::new(AtomicUsize::new(0));
    let worker_execs_c = Arc::clone(&worker_execs);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user.contains("heavy-write") {
                    if req.last_tool_result.is_none() {
                        return MockReply::ToolCalls(vec![(
                            "c-disp-1".into(),
                            "dispatch_work".into(),
                            json!({
                                "brief": "worker-heavy-write",
                                "mode": "read_write",
                                "request_id": "req-heavy-1"
                            }),
                        )]);
                    }
                    return MockReply::Text("已派后台写任务".into());
                }
                if req.last_user.contains("second user question") {
                    return MockReply::Text("second-answered-fast".into());
                }
                if req.last_user.contains("third-hold") {
                    g.enter_and_wait("router-turn3");
                    return MockReply::Text("third-done".into());
                }
                if req.last_user.contains("fourth-durable") {
                    g.record_arrival("fourth-ran");
                    return MockReply::Text("fourth-reconciled".into());
                }
                return MockReply::Text("ack".into());
            }
            if req.last_user.contains("worker-heavy-write") {
                worker_execs_c.fetch_add(1, Ordering::SeqCst);
                g.enter_and_wait("worker-heavy-write");
                return MockReply::Text("worker-heavy-done".into());
            }
            MockReply::Text("worker-ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "proj-alpha".into(),
            root: project,
        })
        .expect("create router project session");
    assert_eq!(handle.session_role(&router_id), SessionRole::Router);
    handle.set_autonomous_drain(true);

    // Turn 1: Router dispatches heavy write Worker onto `project-alpha`.
    handle
        .submit_router_message(&router_id, 1001, "heavy-write task 1")
        .unwrap();
    gate.wait_for_key("worker-heavy-write", Duration::from_secs(5));
    wait_idle(&handle, &router_id, "router turn 1 idle");
    // Worker is hidden from `list_sessions()` while visible in `child_works`.
    assert_eq!(handle.list_sessions().len(), 1);
    assert_eq!(handle.child_works(&router_id).len(), 1);
    assert_eq!(handle.child_works(&router_id)[0].state, "running");

    // While Worker STILL holds the Project Write lock, Turn 2 succeeds immediately!
    let second = handle.submit_router_message(
        &router_id,
        1002,
        "second user question while busy",
    );
    assert_eq!(second.as_deref(), Ok(router_id.as_str()));
    wait_until(
        Duration::from_secs(5),
        || {
            handle.session_lines(&router_id).iter().any(|l| {
                matches!(l, ChatLine::Assistant { text } if text.contains("second-answered-fast"))
            })
        },
        "second router turn answered while worker holds project lock",
    );

    // Release Worker so Turn 1 Worker completes once.
    gate.release("worker-heavy-write");
    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&router_id)
                .first()
                .is_some_and(|w| w.state == "completed")
        },
        "heavy worker completed",
    );
    wait_idle(&handle, &router_id, "router idle after announcement");

    // Start Turn 3 (held while streaming) and enqueue durable Turn 4, then simulate crash!
    handle
        .submit_router_message(&router_id, 1003, "third-hold")
        .unwrap();
    gate.wait_for_key("router-turn3", Duration::from_secs(5));
    assert_eq!(
        handle
            .submit_router_message(&router_id, 1004, "fourth-durable")
            .as_deref(),
        Ok(router_id.as_str())
    );
    assert_eq!(handle.pending_router_followups(&router_id).len(), 1);

    // Crash / restart host.
    gate.release("router-turn3");
    drop(handle);

    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    reopened.set_autonomous_drain(true);
    gate.wait_for_key("fourth-ran", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            reopened.pending_router_followups(&router_id).is_empty()
                && reopened.session_lines(&router_id).iter().any(|l| {
                    matches!(l, ChatLine::Assistant { text } if text.contains("fourth-reconciled"))
                })
        },
        "fourth durable message reconciled after restart",
    );
    assert_eq!(
        worker_execs.load(Ordering::SeqCst),
        1,
        "completed worker must never re-run across restart"
    );

    assert!(
        server.recorded().iter().any(|r| {
            is_router_req(r)
                && r.system_prompt.contains("[ChildWorksReadOnlySnapshot]")
        }),
        "Router turns must inject bounded [ChildWorksReadOnlySnapshot] read-only status header"
    );
}

// =========================================================================
// Test 2: P1-1 Router excluded from `in_flight_work_count()` across all 3 sources.
// =========================================================================
#[test]
fn max_slots_1_worker_starts_before_router_finishes_first_reply() {
    let root = temp_dir("slot1-router-excl");
    let gate = PromptGate::new();
    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(|req, g| {
            if is_router_req(req) {
                if req.last_tool_result.is_none() {
                    return MockReply::ToolCalls(vec![(
                        "c-1".into(),
                        "dispatch_work".into(),
                        json!({"brief": "slot1-worker", "request_id": "req-s1"}),
                    )]);
                }
                // Hold Router's post-tool reply open so Router stays busy in live + engine!
                g.enter_and_wait("router-still-replying");
                return MockReply::Text("router-finished".into());
            }
            if req.last_user.contains("slot1-worker") {
                g.record_arrival("worker-started-while-router-busy");
                return MockReply::Text("worker-done".into());
            }
            MockReply::Text("ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_max_concurrent_works_for_test(1);
    handle.set_autonomous_drain(true);
    let router_id = handle.create_router_session().unwrap();

    handle
        .submit_router_message(&router_id, 2001, "start slot1 task")
        .unwrap();
    gate.wait_for_key("router-still-replying", Duration::from_secs(5));
    // Worker MUST start while Router is STILL running (max_concurrent_works = 1)!
    gate.wait_for_key(
        "worker-started-while-router-busy",
        Duration::from_secs(5),
    );
    assert!(
        handle.session_status(&router_id).is_busy(),
        "router must still be busy when worker starts"
    );

    gate.release_all();
    wait_idle(&handle, &router_id, "router settled");
}

// =========================================================================
// Test 3: P1-1 Two ReadOnly Workers run concurrently on same project while
// ReadWrite Worker waits, AND ReadOnly tool whitelist == ["glob","grep","read_file"]
// on BOTH Project and Home paths.
// =========================================================================
#[test]
fn two_readonly_workers_reach_barrier_concurrently_on_same_project_while_writer_waits_and_tools_are_strictly_readonly()
 {
    let root = temp_dir("ro-concurrency");
    let project = root.join("shared-proj");
    std::fs::create_dir_all(&project).unwrap();
    let gate = PromptGate::new();
    let ro_in_flight = Arc::new(AtomicUsize::new(0));
    let ro_max_in_flight = Arc::new(AtomicUsize::new(0));
    let ro_in_c = Arc::clone(&ro_in_flight);
    let ro_max_c = Arc::clone(&ro_max_in_flight);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user.contains("dispatch-three")
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![
                        (
                            "c-ro1".into(),
                            "dispatch_work".into(),
                            json!({"brief": "ro-task-1", "mode": "read_only", "request_id": "ro-1"}),
                        ),
                        (
                            "c-ro2".into(),
                            "dispatch_work".into(),
                            json!({"brief": "ro-task-2", "mode": "read_only", "request_id": "ro-2"}),
                        ),
                        (
                            "c-rw1".into(),
                            "dispatch_work".into(),
                            json!({"brief": "rw-task-1", "mode": "read_write", "request_id": "rw-1"}),
                        ),
                    ]);
                }
                if req.last_user.contains("dispatch-home-ro")
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-hro".into(),
                        "dispatch_work".into(),
                        json!({"brief": "home-ro-task", "mode": "read_only", "request_id": "hro-1"}),
                    )]);
                }
                return MockReply::Text("dispatched".into());
            }
            if req.last_user.contains("ro-task-1")
                || req.last_user.contains("ro-task-2")
            {
                let cur = ro_in_c.fetch_add(1, Ordering::SeqCst) + 1;
                ro_max_c.fetch_max(cur, Ordering::SeqCst);
                g.enter_and_wait(&req.last_user);
                ro_in_c.fetch_sub(1, Ordering::SeqCst);
                return MockReply::Text("ro-done".into());
            }
            if req.last_user.contains("rw-task-1") {
                g.record_arrival("rw-started");
                return MockReply::Text("rw-done".into());
            }
            if req.last_user.contains("home-ro-task") {
                g.record_arrival("home-ro-ran");
                return MockReply::Text("home-ro-done".into());
            }
            MockReply::Text("ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let proj_router = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "p-shared".into(),
            root: project,
        })
        .unwrap();

    handle
        .submit_router_message(&proj_router, 3001, "dispatch-three")
        .unwrap();

    // BOTH ReadOnly workers must reach the gate concurrently!
    gate.wait_for_key("ro-task-1", Duration::from_secs(5));
    gate.wait_for_key("ro-task-2", Duration::from_secs(5));
    assert!(
        ro_max_in_flight.load(Ordering::SeqCst) >= 2,
        "two read_only workers on the same project must run concurrently"
    );

    // Give drain ample time to attempt starting rw-task-1 if broken.
    thread::sleep(Duration::from_millis(120));
    let works = handle.child_works(&proj_router);
    let rw_work = works
        .iter()
        .find(|w| w.access_mode == WorkAccessMode::ReadWrite)
        .expect("rw worker");
    assert_eq!(
        rw_work.state, "queued",
        "read_write worker must wait while read_only workers hold Read leases"
    );

    // Release both ReadOnly workers -> ReadWrite worker immediately wakes!
    gate.release("ro-task-1");
    gate.release("ro-task-2");
    gate.wait_for_key("rw-started", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&proj_router)
                .iter()
                .all(|w| w.state == "completed")
        },
        "all 3 project workers completed",
    );

    // Also check Home-origin ReadOnly Worker skips Home publish_file/verify_*!
    let home_router = handle.create_router_session().unwrap();
    handle
        .submit_router_message(&home_router, 3002, "dispatch-home-ro")
        .unwrap();
    gate.wait_for_key("home-ro-ran", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&home_router)
                .first()
                .is_some_and(|w| w.state == "completed")
        },
        "home-ro worker completed",
    );

    // Assert exact final tool list exposed to the model for ALL 3 ReadOnly workers!
    let recorded = server.recorded();
    for key in ["ro-task-1", "ro-task-2", "home-ro-task"] {
        let req = recorded
            .iter()
            .find(|r| r.last_user == key)
            .unwrap_or_else(|| panic!("missing recorded request for {key}"));
        assert_eq!(
            req.tool_names,
            vec!["glob".to_owned(), "grep".to_owned(), "read_file".to_owned()],
            "ReadOnly worker `{key}` must expose strictly glob/grep/read_file"
        );
    }
}

// =========================================================================
// Test 4: P1-2 AWS Idempotent dispatch_work retry & payload conflict rejection.
// =========================================================================
#[test]
fn dispatch_work_idempotent_retry_returns_same_work_and_rejects_conflicting_payload()
 {
    let root = temp_dir("idem-dispatch");
    let gate = PromptGate::new();
    let tool_results = Arc::new(Mutex::new(Vec::<String>::new()));
    let tr_c = Arc::clone(&tool_results);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, _| {
            if let Some(tr) = &req.last_tool_result {
                tr_c.lock().unwrap().push(tr.clone());
            }
            if is_router_req(req) {
                if req.last_user == "call-1" && req.last_tool_result.is_none() {
                    return MockReply::ToolCalls(vec![(
                        "c-1".into(),
                        "dispatch_work".into(),
                        json!({"brief": "make deck", "request_id": "req-idem-1"}),
                    )]);
                }
                if req.last_user == "call-2-dup"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-2".into(),
                        "dispatch_work".into(),
                        json!({"brief": "make deck", "request_id": "req-idem-1"}),
                    )]);
                }
                if req.last_user == "call-3-conflict"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-3".into(),
                        "dispatch_work".into(),
                        json!({"brief": "DIFFERENT brief", "request_id": "req-idem-1"}),
                    )]);
                }
                return MockReply::Text("done".into());
            }
            MockReply::Text("worker".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();

    for (sub, prompt) in [
        (4001_u128, "call-1"),
        (4002, "call-2-dup"),
        (4003, "call-3-conflict"),
    ] {
        handle
            .submit_router_message(&router_id, sub, prompt)
            .unwrap();
        wait_until(
            Duration::from_secs(5),
            || !handle.session_status(&router_id).is_busy(),
            prompt,
        );
    }

    assert_eq!(
        handle.child_works(&router_id).len(),
        1,
        "idempotent retry and conflict must leave strictly 1 child worker"
    );
    assert_eq!(
        handle.pending_acceptances().len(),
        1,
        "only 1 receipt must exist in AcceptanceQueue"
    );
    let results = tool_results.lock().unwrap().clone();
    let r1: Value = serde_json::from_str(&results[0]).unwrap();
    let r2: Value = serde_json::from_str(&results[1]).unwrap();
    assert_eq!(r1["work_id"], r2["work_id"]);
    assert_eq!(r1["receipt_id"], r2["receipt_id"]);
    assert_eq!(r1["deduplicated"], json!(false));
    assert_eq!(r2["deduplicated"], json!(true));
    assert!(
        results[2].contains("IDEMPOTENCY_CONFLICT"),
        "conflicting payload must fail with IDEMPOTENCY_CONFLICT; got {}",
        results[2]
    );
}

// =========================================================================
// Test 5: P1-2 Fault injection between worker create & enqueue + strict_target
// missing target termination (never spawns orphan Home session).
// =========================================================================
#[test]
fn fault_injection_delete_parent_between_worker_create_and_enqueue_terminates_strict_receipt()
 {
    let root = temp_dir("fault-strict-target");
    let gate = PromptGate::new();
    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(|req, _| {
            if is_router_req(req) && req.last_tool_result.is_none() {
                return MockReply::ToolCalls(vec![(
                    "c-1".into(),
                    "dispatch_work".into(),
                    json!({"brief": "will fail enqueue", "request_id": "req-fail-1"}),
                )]);
            }
            MockReply::Text("ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();

    // Part A: Inject failure right between worker creation and accept_strict ->
    // dispatch rolls back the newly created empty worker AND removes its
    // freshly created Home workspace directory cleanly.
    let rolled_back_worker = Arc::new(Mutex::new(None::<String>));
    let rolled_back_c = Arc::clone(&rolled_back_worker);
    handle.set_before_worker_strict_enqueue_hook(Some(Arc::new(
        move |_router, worker| {
            *rolled_back_c.lock().unwrap() = Some(worker.to_owned());
            Err("INJECTED_ENQUEUE_FAILURE")
        },
    )));
    handle
        .submit_router_message(&router_id, 5001, "trigger fault")
        .unwrap();
    wait_idle(
        &handle,
        &router_id,
        "router finished after injected enqueue error",
    );
    handle.set_before_worker_strict_enqueue_hook(None);
    assert!(
        handle.child_works(&router_id).is_empty(),
        "failed strict enqueue must roll back freshly created worker"
    );
    let aborted_wid = rolled_back_worker
        .lock()
        .unwrap()
        .clone()
        .expect("fault hook observed rolled-back worker id");
    assert!(
        !root
            .join("sessions")
            .join("workspaces")
            .join(&aborted_wid)
            .exists(),
        "rollback must delete empty Home workspace directory for `{aborted_wid}`"
    );

    // Part B: Simulate a stale strict_target receipt pointing to a deleted worker
    // (e.g. parent + worker deleted concurrently or after crash).
    let before_sessions = handle.list_sessions().len();
    let queue = AcceptanceQueue::new(root.to_path_buf());
    let orphan_strict = queue
        .accept_strict(
            "w-deleted-ghost",
            "must never spawn orphan home",
            Some("req-ghost"),
            now_ms(),
        )
        .unwrap();
    assert!(orphan_strict.strict_target);

    let claimed = handle.run_accepted_once().expect("pump queue");
    assert!(
        claimed.is_none(),
        "strict receipt for missing worker must not be claimed or fallback-forked"
    );
    assert_eq!(
        handle.receipt_state(&orphan_strict.id),
        Some(ReceiptState::Cancelled),
        "strict receipt for missing target must transition to Cancelled"
    );
    assert_eq!(
        handle.list_sessions().len(),
        before_sessions,
        "strict receipt termination must create ZERO orphan Home sessions"
    );

    // Part C: Simulate an empty unqueued Worker left on disk from a hard crash ->
    // boot() deterministically cleans it up without creating orphan sessions.
    let engine = lock(&handle.inner.engine).clone().unwrap();
    engine
        .create_session_with_role(
            "w-crashed-empty".into(),
            now_ms(),
            SessionOrigin::Home,
            None,
            SessionRole::Worker,
            Some(router_id.clone()),
            WorkAccessMode::ReadWrite,
        )
        .unwrap();
    drop(engine);
    drop(handle);

    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    assert!(
        reopened.child_works(&router_id).is_empty(),
        "boot() must reconcile and prune empty unqueued crashed worker"
    );
}

// =========================================================================
// Test 6: P1-2 / P1-3 Crash after durable follow-ups accepted -> resumes FIFO
// strictly once, handles duplicate submission_id vs distinct same text, and
// enforces max queue capacity.
// =========================================================================
#[test]
fn fault_injection_crash_after_second_user_message_accepted_resumes_fifo_exactly_once()
 {
    let root = temp_dir("fault-fifo-resume");
    let gate = PromptGate::new();
    let observed_order = Arc::new(Mutex::new(Vec::<String>::new()));
    let order_c = Arc::clone(&observed_order);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if req.last_user == "turn-1-hold" {
                g.enter_and_wait("turn-1-hold");
                return MockReply::Text("t1-done".into());
            }
            order_c.lock().unwrap().push(req.last_user.clone());
            g.record_arrival(&format!("ran:{}", req.last_user));
            MockReply::Text(format!("ok:{}", req.last_user))
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();
    handle
        .submit_router_message(&router_id, 6001, "turn-1-hold")
        .unwrap();
    gate.wait_for_key("turn-1-hold", Duration::from_secs(5));

    // Queue FIFO follow-ups while Router is busy:
    assert_eq!(
        handle
            .submit_router_message(&router_id, 6002, "followup-1")
            .as_deref(),
        Ok(router_id.as_str())
    );
    // Exact same submission_id retry is idempotent (no duplicate).
    assert_eq!(
        handle
            .submit_router_message(&router_id, 6002, "followup-1")
            .as_deref(),
        Ok(router_id.as_str())
    );
    // Same text with DIFFERENT submission_id is treated as two distinct user commands!
    assert_eq!(
        handle
            .submit_router_message(&router_id, 6003, "repeat-text")
            .as_deref(),
        Ok(router_id.as_str())
    );
    assert_eq!(
        handle
            .submit_router_message(&router_id, 6004, "repeat-text")
            .as_deref(),
        Ok(router_id.as_str())
    );
    assert_eq!(
        handle
            .submit_router_message(&router_id, 6005, "followup-final")
            .as_deref(),
        Ok(router_id.as_str())
    );
    assert_eq!(handle.pending_router_followups(&router_id).len(), 4);
    assert!(
        handle.pending_acceptances().is_empty(),
        "Router follow-ups must NEVER enter background AcceptanceQueue"
    );

    // Simulate abrupt host crash while Turn 1 is held & 4 follow-ups are on disk.
    drop(handle);

    // Reopen fresh handle -> durable FIFO follow-ups resume automatically in exact order!
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    gate.wait_for_key("ran:followup-final", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            reopened.pending_router_followups(&router_id).is_empty()
                && !reopened.session_status(&router_id).is_busy()
        },
        "all 4 durable FIFO follow-ups completed",
    );
    assert_eq!(
        *observed_order.lock().unwrap(),
        vec![
            "followup-1".to_owned(),
            "repeat-text".to_owned(),
            "repeat-text".to_owned(),
            "followup-final".to_owned(),
        ],
        "follow-ups must resume in exact FIFO order and execute strictly once"
    );
}

// =========================================================================
// Test 7: P1-2 Crash after first child deleted -> boot() finishes cascade
// cleanup of remaining child, parent Router, queued receipts, and artifacts.
// =========================================================================
#[test]
fn fault_injection_crash_after_first_child_deleted_finishes_cascade_cleanup_on_boot()
 {
    let root = temp_dir("fault-cascade-del");
    let gate = PromptGate::new();
    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(|req, _| {
            if is_router_req(req) && req.last_tool_result.is_none() {
                return MockReply::ToolCalls(vec![
                    (
                        "c-1".into(),
                        "dispatch_work".into(),
                        json!({"brief": "child-1", "request_id": "del-c1"}),
                    ),
                    (
                        "c-2".into(),
                        "dispatch_work".into(),
                        json!({"brief": "child-2", "request_id": "del-c2"}),
                    ),
                ]);
            }
            MockReply::Text("ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();
    handle
        .submit_router_message(&router_id, 7001, "spawn two children")
        .unwrap();
    wait_idle(&handle, &router_id, "router spawned 2 children");
    let children = handle.child_works(&router_id);
    assert_eq!(children.len(), 2);
    let child_2_id = children[1].work_id.clone();

    // Commit a durable Library artifact on child #2 so we prove cascade recovery
    // also cleans up child artifacts.
    let store = handle.artifact_store().unwrap();
    let src_dir = root.join("child2-src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("note.txt"), b"child 2 deliverable").unwrap();
    let prepared = store
        .prepare(crate::artifact_store::PrepareArtifact {
            source_root: src_dir,
            relative_path: "note.txt".into(),
            source_relative_path: "note.txt".into(),
            conversation_import: false,
            session_id: child_2_id.clone(),
            submission_id: "submission-00000000000000000000000000000077".into(),
            tool_call_id: "tc-77".into(),
            delivery_key: "note.txt".into(),
            display_name: None,
            origin: SessionOrigin::Home,
        })
        .unwrap();
    let commit = DeliveryCommit {
        session_id: child_2_id,
        submission_id: "submission-00000000000000000000000000000077".into(),
        artifact_ids: vec![prepared.artifact_id],
        completed_successfully: true,
        committed_at_ms: now_ms(),
        origin: SessionOrigin::Home,
        origin_title_snapshot: "child 2".into(),
    };
    lock(&handle.inner.engine)
        .clone()
        .unwrap()
        .record_delivery_commit(&commit)
        .unwrap();
    store.commit_prepared(&commit).unwrap();
    assert_eq!(handle.artifacts().unwrap().len(), 1);

    // Inject crash right after child #1 is removed from history.json!
    handle.set_after_first_child_deleted_hook(Some(Arc::new(
        |_parent, _first_child| Err("INJECTED_CRASH_AFTER_CHILD_1"),
    )));
    assert_eq!(
        handle.delete_session(&router_id),
        Err("INJECTED_CRASH_AFTER_CHILD_1")
    );
    // Simulate process crash right at the half-deleted point.
    drop(handle);

    // Reopen fresh handle -> boot() MUST roll forward cascade deletion!
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    assert!(
        reopened.list_sessions().is_empty(),
        "parent router must be deleted on boot recovery"
    );
    assert!(
        reopened.child_works(&router_id).is_empty(),
        "remaining child worker must be deleted on boot recovery"
    );
    assert!(
        reopened.artifacts().unwrap().is_empty(),
        "child worker committed artifacts must be cleaned up on boot recovery"
    );
    assert!(
        reopened.pending_acceptances().is_empty(),
        "queued worker receipts must be cancelled on cascade recovery"
    );
}

// =========================================================================
// Test 8: Cross-root isolation across two Routers (query, dispatch, object
// targeting, control, ask_user_choice, and card routing).
// =========================================================================
#[test]
fn two_routers_cross_root_query_dispatch_control_and_card_routing_all_rejected()
{
    let root = temp_dir("cross-root");
    let gate = PromptGate::new();
    let foreign_work_slot = Arc::new(Mutex::new(String::new()));
    let foreign_c = Arc::clone(&foreign_work_slot);
    let results_b = Arc::new(Mutex::new(Vec::<String>::new()));
    let results_b_c = Arc::clone(&results_b);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, _| {
            if is_router_req(req) {
                if let Some(tr) = &req.last_tool_result
                    && req.last_user.starts_with("attack-")
                {
                    results_b_c.lock().unwrap().push(tr.clone());
                }
                if req.last_user == "spawn-a" && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "ca-1".into(),
                        "dispatch_work".into(),
                        json!({"brief": "work a", "request_id": "ra-1"}),
                    )]);
                }
                let foreign_wid = foreign_c.lock().unwrap().clone();
                if req.last_user == "attack-query"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "cb-1".into(),
                        "query_work_status".into(),
                        json!({"work_id": foreign_wid}),
                    )]);
                }
                if req.last_user == "attack-dispatch-work"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "cb-2".into(),
                        "dispatch_work".into(),
                        json!({"brief": "hijack", "target_work_id": foreign_wid}),
                    )]);
                }
                if req.last_user == "attack-dispatch-object"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "cb-3".into(),
                        "dispatch_work".into(),
                        json!({"brief": "hijack obj", "target_object_ids": ["obj-secret-a"]}),
                    )]);
                }
                if req.last_user == "attack-control"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "cb-4".into(),
                        "control_work".into(),
                        json!({"work_id": foreign_wid, "action": "cancel"}),
                    )]);
                }
                if req.last_user == "attack-choice"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "cb-5".into(),
                        "ask_user_choice".into(),
                        json!({
                            "prompt": "Pick foreign object?",
                            "options": [{"object_id": "obj-secret-a", "label": "secret.pptx"}]
                        }),
                    )]);
                }
                return MockReply::Text("done".into());
            }
            MockReply::Text("worker".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_a = handle.create_router_session().unwrap();
    let router_b = handle.create_router_session().unwrap();

    handle
        .submit_router_message(&router_a, 8001, "spawn-a")
        .unwrap();
    wait_idle(&handle, &router_a, "router_a spawned worker");
    let worker_a = handle.child_works(&router_a)[0].work_id.clone();
    *foreign_work_slot.lock().unwrap() = worker_a.clone();

    // Attach a tracked object + pending approval card on worker_a.
    let engine = lock(&handle.inner.engine).clone().unwrap();
    engine
        .set_session_work_contract(
            &worker_a,
            WorkContract {
                revision: 1,
                required_items: vec![RequiredDeliverable {
                    filename: Some("secret.pptx".into()),
                    extension: "pptx".into(),
                }],
                min_slides: Some(3),
                pinned_facts: BTreeMap::new(),
                objects: vec![TrackedObject {
                    object_id: "obj-secret-a".into(),
                    kind: TrackedObjectKind::Deliverable,
                    filename: Some("secret.pptx".into()),
                    extension: "pptx".into(),
                    anchor_summary: "secret.pptx".into(),
                    overrides: BTreeMap::new(),
                    verify_state: ObjectVerifyState::Unverified,
                }],
            },
            None,
        )
        .unwrap();
    engine
        .park_pending_card(
            &worker_a,
            PendingCard::ToolApproval {
                card_id: "card-a-1".into(),
                created_at_ms: 10,
                tool_name: "powershell".into(),
                target_summary: "secret".into(),
                args_hash: "hash-a".into(),
                risk_reason: "risk".into(),
                command_preview: "rm".into(),
            },
            None,
        )
        .unwrap();

    for (sub, attack) in [
        (8010_u128, "attack-query"),
        (8011, "attack-dispatch-work"),
        (8012, "attack-dispatch-object"),
        (8013, "attack-control"),
        (8014, "attack-choice"),
    ] {
        handle
            .submit_router_message(&router_b, sub, attack)
            .unwrap();
        wait_until(
            Duration::from_secs(5),
            || !handle.session_status(&router_b).is_busy(),
            attack,
        );
    }

    let attacks = results_b.lock().unwrap().clone();
    assert_eq!(
        attacks.len(),
        5,
        "all 5 attack tool calls must report errors"
    );
    assert!(attacks[0].contains("WORK_OWNERSHIP_DENIED"), "{attacks:?}");
    assert!(attacks[1].contains("WORK_OWNERSHIP_DENIED"), "{attacks:?}");
    assert!(
        attacks[2].contains("OBJECT_OWNERSHIP_DENIED"),
        "{attacks:?}"
    );
    assert!(attacks[3].contains("WORK_OWNERSHIP_DENIED"), "{attacks:?}");
    assert!(
        attacks[4].contains("OBJECT_OWNERSHIP_DENIED"),
        "{attacks:?}"
    );

    // Direct host API cross-root calls and card routing are strictly isolated too.
    assert_eq!(
        handle.cancel_work(&router_b, &worker_a),
        Err("WORK_OWNERSHIP_DENIED")
    );
    assert_eq!(
        handle.revise_work(&router_b, &worker_a, "steal", "rev-steal"),
        Err("WORK_OWNERSHIP_DENIED")
    );
    assert_eq!(handle.root_pending_cards(&router_b).len(), 0);
    assert_eq!(handle.root_pending_cards(&router_a).len(), 1);
    assert_eq!(handle.session_pending_cards(&router_b).len(), 0);
}

// =========================================================================
// Test 9: P1-3 FIFO follow-ups beat WorkEventBatch AND announcement uses
// `MessageSource::SystemEvent { event_kind: "work_event_batch" }` with zero
// user bubble pollution across restarts.
// =========================================================================
#[test]
fn router_fifo_followups_persist_in_order_beat_work_events_and_announcement_uses_system_event_source()
 {
    let root = temp_dir("priority-source");
    let gate = PromptGate::new();
    let router_turns = Arc::new(Mutex::new(Vec::<String>::new()));
    let rt_c = Arc::clone(&router_turns);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user == "turn1-dispatch" {
                    if req.last_tool_result.is_none() {
                        return MockReply::ToolCalls(vec![(
                            "c-1".into(),
                            "dispatch_work".into(),
                            json!({"brief": "fast-worker", "request_id": "req-fast"}),
                        )]);
                    }
                    g.enter_and_wait("hold-router-turn1");
                    return MockReply::Text("turn1-replied".into());
                }
                rt_c.lock().unwrap().push(req.last_user.clone());
                if req.last_user.starts_with("[WorkEventBatch]") {
                    g.record_arrival("announcement-ran");
                    return MockReply::Text(
                        "worker-finished-announcement".into(),
                    );
                }
                g.record_arrival(&format!("user-ran:{}", req.last_user));
                return MockReply::Text(format!("reply:{}", req.last_user));
            }
            if req.last_user == "fast-worker" {
                g.record_arrival("fast-worker-done");
                return MockReply::Text("worker-complete".into());
            }
            MockReply::Text("ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let router_id = handle.create_router_session().unwrap();

    handle
        .submit_router_message(&router_id, 9001, "turn1-dispatch")
        .unwrap();
    gate.wait_for_key("hold-router-turn1", Duration::from_secs(5));
    // Worker finishes WHILE Router Turn 1 is still streaming, pushing WorkEvent into inbox!
    gate.wait_for_key("fast-worker-done", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&router_id)
                .first()
                .is_some_and(|w| w.state == "completed")
        },
        "worker settled while router turn 1 held",
    );

    // Enqueue user follow-up while Router Turn 1 is STILL held.
    handle
        .submit_router_message(&router_id, 9002, "urgent-user-followup")
        .unwrap();

    // Release Router Turn 1 -> user follow-up MUST preempt WorkEventBatch!
    gate.release("hold-router-turn1");
    gate.wait_for_key("user-ran:urgent-user-followup", Duration::from_secs(5));
    gate.wait_for_key("announcement-ran", Duration::from_secs(5));
    wait_idle(&handle, &router_id, "announcement completed");

    let turns = router_turns.lock().unwrap().clone();
    assert_eq!(
        turns.len(),
        2,
        "expected user follow-up then announcement: {turns:?}"
    );
    assert_eq!(turns[0], "urgent-user-followup");
    assert!(turns[1].starts_with("[WorkEventBatch]"));

    // Assert zero `[WorkEventBatch]` user bubble in UI lines!
    let lines = handle.session_lines(&router_id);
    assert!(
        !lines.iter().any(|l| matches!(l, ChatLine::User { text } if text.contains("[WorkEventBatch]"))),
        "WorkEventBatch must never project as ChatLine::User: {lines:?}"
    );
    assert!(lines.iter().any(|l| {
        matches!(l, ChatLine::Assistant { text } if text == "worker-finished-announcement")
    }));

    // Assert persisted `history.json` uses `MessageSource::SystemEvent { event_kind: "work_event_batch" }`!
    let disk = flowy_agent_core::SessionIndex::load(&root).unwrap();
    let rec = disk.get(&router_id).unwrap();
    let sys_msg = rec
        .messages
        .iter()
        .find(|m| m.text().starts_with("[WorkEventBatch]"))
        .expect("persisted WorkEventBatch message");
    assert_eq!(
        sys_msg.source,
        MessageSource::SystemEvent {
            event_kind: "work_event_batch".into()
        }
    );
    assert_eq!(sys_msg.display_text.as_deref(), Some(""));

    // Reopen fresh handle -> still zero `[WorkEventBatch]` user bubbles, next turn works!
    drop(handle);
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    assert!(
        !reopened.session_lines(&router_id).iter().any(|l| matches!(l, ChatLine::User { text } if text.contains("[WorkEventBatch]")))
    );
    reopened
        .submit_router_message(&router_id, 9003, "after-restart-chat")
        .unwrap();
    gate.wait_for_key("user-ran:after-restart-chat", Duration::from_secs(5));

    assert!(
        server.recorded().iter().any(|r| {
            is_router_req(r)
                && r.system_prompt.contains("[ChildWorksReadOnlySnapshot]")
        }),
        "Router turns must inject bounded [ChildWorksReadOnlySnapshot] read-only status header"
    );
}

// =========================================================================
// Test 10: P1-3 WorkEventBatch announcement strips dispatch/control/choice tools,
// blocking retry storms on Worker failure.
// =========================================================================
#[test]
fn work_event_announcement_strips_dispatch_tool_blocking_retry_storm() {
    let root = temp_dir("strip-announce-tools");
    let gate = PromptGate::new();
    let announcement_tools = Arc::new(Mutex::new(Vec::<String>::new()));
    let at_c = Arc::clone(&announcement_tools);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user == "run-failing-work"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-fail".into(),
                        "dispatch_work".into(),
                        json!({"brief": "boom-work", "request_id": "req-boom"}),
                    )]);
                }
                if req.last_user.starts_with("[WorkEventBatch]") {
                    *at_c.lock().unwrap() = req.tool_names.clone();
                    if req.last_tool_result.is_none() {
                        // Adversarial model tries to re-dispatch inside announcement!
                        return MockReply::ToolCalls(vec![(
                            "c-storm".into(),
                            "dispatch_work".into(),
                            json!({"brief": "infinite retry"}),
                        )]);
                    }
                    g.record_arrival("announce-settled");
                    return MockReply::Text("reported failure honestly".into());
                }
                return MockReply::Text("dispatched".into());
            }
            // Worker fails deterministically!
            MockReply::Error("simulated provider outage".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let router_id = handle.create_router_session().unwrap();

    handle
        .submit_router_message(&router_id, 10001, "run-failing-work")
        .unwrap();
    gate.wait_for_key("announce-settled", Duration::from_secs(5));
    wait_idle(&handle, &router_id, "announcement settled");

    assert_eq!(
        *announcement_tools.lock().unwrap(),
        vec!["query_work_status".to_owned()],
        "WorkEventBatch announcement must strip dispatch_work, control_work, and ask_user_choice"
    );
    assert_eq!(
        handle.child_works(&router_id).len(),
        1,
        "stripped dispatch_work must block retry storm (still 1 worker)"
    );
}

// =========================================================================
// Test 11: P1-4 Contract-less Router `ask_user_choice` halts batch (0 trailing
// dispatch), survives restart, and resolves strictly once with CardResume.
// =========================================================================
#[test]
fn contractless_router_ask_user_choice_halts_turn_survives_restart_and_resumes_once()
 {
    let root = temp_dir("choice-card");
    let gate = PromptGate::new();
    let resume_prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let rp_c = Arc::clone(&resume_prompts);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user == "edit that deck"
                    && req.last_tool_result.is_none()
                {
                    // Emit ask_user_choice AND a hallucinated dispatch_work in same turn!
                    return MockReply::ToolCalls(vec![
                        (
                            "c-ask".into(),
                            "ask_user_choice".into(),
                            json!({
                                "prompt": "请选择要修改的版本",
                                "impact_boundary": "仅修改所选版本",
                                "options": [
                                    {"object_id": "opt-formal", "label": "正式汇报版.pptx"},
                                    {"object_id": "opt-draft", "label": "内部草稿版.pptx"}
                                ],
                                "allow_multi": false
                            }),
                        ),
                        (
                            "c-bad-disp".into(),
                            "dispatch_work".into(),
                            json!({"brief": "should never execute"}),
                        ),
                    ]);
                }
                if req.last_user.contains("[CardChoiceConfirmed]") {
                    rp_c.lock().unwrap().push(req.last_user.clone());
                    g.record_arrival("choice-resumed");
                    return MockReply::Text("已按确认目标继续".into());
                }
            }
            MockReply::Text("ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();

    handle
        .submit_router_message(&router_id, 11001, "edit that deck")
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || handle.session_status(&router_id) == TurnStatus::WaitingForUser,
        "router suspended in WaitingForUser",
    );
    assert!(
        handle.child_works(&router_id).is_empty(),
        "ask_user_choice must terminate batch with 0 trailing dispatches"
    );
    // Regular chat while card pending is rejected fail-closed.
    assert_eq!(
        handle.submit_router_message(&router_id, 11002, "keep chatting"),
        Err("notice.card_pending")
    );
    let cards = handle.root_pending_cards(&router_id);
    assert_eq!(cards.len(), 1);
    let card_id = cards[0].card.card_id().to_owned();

    // Restart host -> card and WaitingForUser persist on disk!
    drop(handle);
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    assert_eq!(
        reopened.session_status(&router_id),
        TurnStatus::WaitingForUser
    );
    assert_eq!(reopened.root_pending_cards(&router_id).len(), 1);

    // Resolve contract-less Router Disambiguation card!
    let outcome = reopened
        .apply_card_action(CardAction::ResolveCard {
            session_id: router_id.clone(),
            card_id: card_id.clone(),
            selection: CardSelection::Disambiguate {
                object_ids: vec!["opt-formal".into()],
            },
        })
        .expect("resolve contract-less router choice card");
    assert_eq!(
        outcome,
        CardActionOutcome::CardResolved {
            card_id: card_id.clone(),
            resumed: true,
        }
    );
    gate.wait_for_key("choice-resumed", Duration::from_secs(5));
    wait_idle(&reopened, &router_id, "router resumed turn settled");
    assert_eq!(resume_prompts.lock().unwrap().len(), 1);
    assert!(resume_prompts.lock().unwrap()[0].contains("opt-formal"));

    // Second resolution attempt MUST fail cleanly (resumed strictly once!).
    assert_eq!(
        reopened.apply_card_action(CardAction::ResolveCard {
            session_id: router_id.clone(),
            card_id,
            selection: CardSelection::Disambiguate {
                object_ids: vec!["opt-formal".into()],
            },
        }),
        Err("notice.card_not_found")
    );

    // Verify `MessageSource::CardResume` persisted in history.json!
    let disk = flowy_agent_core::SessionIndex::load(&root).unwrap();
    let rec = disk.get(&router_id).unwrap();
    assert!(rec.messages.iter().any(|m| {
        m.source == MessageSource::CardResume
            && m.text().contains("[CardChoiceConfirmed]")
    }));
}

// =========================================================================
// Test 12: P1-5 cancel_work (Queued / Running / WaitingForUser / terminal),
// stop_router_reply isolation, and idempotent revise_work never duplicates.
// =========================================================================
#[test]
fn control_work_cancel_queued_running_finalizing_and_idempotent_revise_never_duplicates()
 {
    let root = temp_dir("control-matrix");
    let gate = PromptGate::new();
    let revised_runs = Arc::new(AtomicUsize::new(0));
    let rev_c = Arc::clone(&revised_runs);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user == "spawn-3" && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![
                        (
                            "c-q".into(),
                            "dispatch_work".into(),
                            json!({"brief": "queued-work", "request_id": "cw-q"}),
                        ),
                        (
                            "c-r".into(),
                            "dispatch_work".into(),
                            json!({"brief": "running-work", "request_id": "cw-r"}),
                        ),
                        (
                            "c-w".into(),
                            "dispatch_work".into(),
                            json!({"brief": "waiting-work", "request_id": "cw-w"}),
                        ),
                    ]);
                }
                if req.last_user == "hold-router-stop" {
                    g.enter_and_wait("hold-router-stop");
                    return MockReply::Text("should-be-stopped".into());
                }
                return MockReply::Text("ok".into());
            }
            if req.last_user == "running-work" {
                g.enter_and_wait("running-work");
                return MockReply::Text("run-done".into());
            }
            if req.last_user == "revised-brief-v2" {
                rev_c.fetch_add(1, Ordering::SeqCst);
                g.record_arrival("revised-v2-ran");
                return MockReply::Text("v2-done".into());
            }
            MockReply::Text("worker-done".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();

    handle
        .submit_router_message(&router_id, 12001, "spawn-3")
        .unwrap();
    wait_idle(&handle, &router_id, "router spawned 3 workers");
    let works = handle.child_works(&router_id);
    assert_eq!(works.len(), 3);
    let w_queued = works
        .iter()
        .find(|w| w.title.contains("queued-work"))
        .unwrap()
        .work_id
        .clone();
    let w_running = works
        .iter()
        .find(|w| w.title.contains("running-work"))
        .unwrap()
        .work_id
        .clone();
    let w_waiting = works
        .iter()
        .find(|w| w.title.contains("waiting-work"))
        .unwrap()
        .work_id
        .clone();

    // 1. Cancel Queued worker -> cancels receipt immediately.
    let c_queued = handle.cancel_work(&router_id, &w_queued).unwrap();
    assert_eq!(c_queued.outcome, "cancelled");
    assert_eq!(c_queued.cancelled_receipt_ids.len(), 1);
    assert_eq!(
        handle.receipt_state(&c_queued.cancelled_receipt_ids[0]),
        Some(ReceiptState::Cancelled)
    );

    // 2. Park w_waiting in WaitingForUser and cancel it -> clears card + receipts.
    lock(&handle.inner.engine)
        .clone()
        .unwrap()
        .park_pending_card(
            &w_waiting,
            PendingCard::ToolApproval {
                card_id: "card-w-wait".into(),
                created_at_ms: 1,
                tool_name: "powershell".into(),
                target_summary: "t".into(),
                args_hash: "h".into(),
                risk_reason: "r".into(),
                command_preview: "cmd".into(),
            },
            None,
        )
        .unwrap();
    let c_wait = handle.cancel_work(&router_id, &w_waiting).unwrap();
    assert_eq!(c_wait.outcome, "cancelled");
    assert_eq!(handle.session_pending_cards(&w_waiting).len(), 0);

    // 3. Start running-work, then test `stop_router_reply` stops ONLY Router!
    handle.set_autonomous_drain(true);
    gate.wait_for_key("running-work", Duration::from_secs(5));
    handle
        .submit_router_message(&router_id, 12002, "hold-router-stop")
        .unwrap();
    gate.wait_for_key("hold-router-stop", Duration::from_secs(5));
    let stop_r = handle.stop_router_reply(&router_id).unwrap();
    assert!(
        matches!(stop_r.outcome.as_str(), "cancelled" | "stopping"),
        "{stop_r:?}"
    );
    gate.release("hold-router-stop");
    wait_idle(&handle, &router_id, "router stopped");
    assert_eq!(
        handle.session_status(&w_running),
        TurnStatus::Running,
        "stop_router_reply must NOT cancel child workers"
    );

    // 4. Revise running-work idempotently -> cancels old run, settles, enqueues strictly ONE receipt!
    let rev1 = handle
        .revise_work(&router_id, &w_running, "revised-brief-v2", "rev-id-1")
        .unwrap();
    gate.release("running-work");
    assert!(!rev1.deduplicated);
    let rev2 = handle
        .revise_work(&router_id, &w_running, "revised-brief-v2", "rev-id-1")
        .unwrap();
    assert!(rev2.deduplicated);
    assert_eq!(rev1.receipt_id, rev2.receipt_id);
    assert_eq!(
        handle.revise_work(
            &router_id,
            &w_running,
            "conflicting-brief",
            "rev-id-1"
        ),
        Err("IDEMPOTENCY_CONFLICT")
    );
    gate.wait_for_key("revised-v2-ran", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            handle.receipt_state(&rev1.receipt_id)
                == Some(ReceiptState::Completed)
        },
        "revised brief v2 completed",
    );
    assert_eq!(
        revised_runs.load(Ordering::SeqCst),
        1,
        "idempotent revise must execute strictly once"
    );

    // 5. Terminal worker cancel -> already_settled.
    let c_term = handle.cancel_work(&router_id, &w_running).unwrap();
    assert_eq!(c_term.outcome, "already_settled");
}

// =========================================================================
// Test 13: Dot Boundary #3 — Project worker preserves workspace binding and
// ReadOnly worker cannot escalate to ReadWrite on resume.
// =========================================================================
#[test]
fn project_worker_preserves_workspace_binding_and_readonly_worker_cannot_escalate_on_resume()
 {
    let root = temp_dir("no-escalation");
    let project = root.join("proj-immutable");
    std::fs::create_dir_all(&project).unwrap();
    let gate = PromptGate::new();
    let worker_id_slot = Arc::new(Mutex::new(String::new()));
    let wid_c = Arc::clone(&worker_id_slot);
    let tool_outputs = Arc::new(Mutex::new(Vec::<String>::new()));
    let to_c = Arc::clone(&tool_outputs);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if let Some(tr) = &req.last_tool_result {
                    to_c.lock().unwrap().push(tr.clone());
                }
                if req.last_user == "spawn-ro" && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-1".into(),
                        "dispatch_work".into(),
                        json!({"brief": "inspect project", "mode": "read_only", "request_id": "ro-init"}),
                    )]);
                }
                let wid = wid_c.lock().unwrap().clone();
                if req.last_user == "try-escalate"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-2".into(),
                        "dispatch_work".into(),
                        json!({"brief": "edit code", "mode": "read_write", "target_work_id": wid, "request_id": "ro-esc"}),
                    )]);
                }
                if req.last_user == "resume-ro"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-3".into(),
                        "dispatch_work".into(),
                        json!({"brief": "inspect again", "target_work_id": wid, "request_id": "ro-resume"}),
                    )]);
                }
                return MockReply::Text("ok".into());
            }
            if req.last_user == "inspect again" {
                g.record_arrival("resumed-ro-ran");
            }
            MockReply::Text("ro-output".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let router_id = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "p-imm".into(),
            root: project.clone(),
        })
        .unwrap();

    handle
        .submit_router_message(&router_id, 13001, "spawn-ro")
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&router_id)
                .first()
                .is_some_and(|w| w.state == "completed")
                && !handle.session_status(&router_id).is_busy()
        },
        "initial ro worker completed",
    );
    let w = handle.child_works(&router_id)[0].clone();
    assert_eq!(w.access_mode, WorkAccessMode::ReadOnly);
    assert_eq!(w.workspace_root.as_deref(), Some(project.as_path()));
    *worker_id_slot.lock().unwrap() = w.work_id;

    // Attempt escalation ReadOnly -> ReadWrite: MUST be denied!
    handle
        .submit_router_message(&router_id, 13002, "try-escalate")
        .unwrap();
    wait_idle(&handle, &router_id, "escalation attempt finished");
    assert!(
        tool_outputs
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains("ACCESS_MODE_ESCALATION_DENIED")),
        "expected ACCESS_MODE_ESCALATION_DENIED, got {:?}",
        *tool_outputs.lock().unwrap()
    );

    // Resume without escalation: preserves ReadOnly & project workspace!
    handle
        .submit_router_message(&router_id, 13003, "resume-ro")
        .unwrap();
    gate.wait_for_key("resumed-ro-ran", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            handle
                .child_works(&router_id)
                .first()
                .is_some_and(|w| w.state == "completed")
        },
        "resumed ro worker completed",
    );
    let resumed_req = server
        .recorded()
        .into_iter()
        .find(|r| r.last_user == "inspect again")
        .unwrap();
    assert_eq!(
        resumed_req.tool_names,
        vec!["glob".to_owned(), "grep".to_owned(), "read_file".to_owned()]
    );
    assert_eq!(
        handle.child_works(&router_id)[0].workspace_root.as_deref(),
        Some(project.as_path())
    );
}

// =========================================================================
// Test 14: P1-5 Home ReadWrite Worker gets full S1-S3 delivery gate while
// Project ReadWrite Worker gets project directory tools without Home publish_file.
// =========================================================================
#[test]
fn home_worker_full_delivery_gate_vs_project_worker_directory_tools() {
    let root = temp_dir("home-vs-project-tools");
    let project = root.join("proj-rw");
    std::fs::create_dir_all(&project).unwrap();
    let gate = PromptGate::new();

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(|req, g| {
            if is_router_req(req) && req.last_tool_result.is_none() {
                if req.last_user == "do-home" {
                    return MockReply::ToolCalls(vec![(
                        "c-h".into(),
                        "dispatch_work".into(),
                        json!({"brief": "home-write-brief", "mode": "read_write", "request_id": "hw-1"}),
                    )]);
                }
                if req.last_user == "do-proj" {
                    return MockReply::ToolCalls(vec![(
                        "c-p".into(),
                        "dispatch_work".into(),
                        json!({"brief": "proj-write-brief", "mode": "read_write", "request_id": "pw-1"}),
                    )]);
                }
            }
            if req.last_user == "home-write-brief" {
                g.record_arrival("home-w-ran");
            }
            if req.last_user == "proj-write-brief" {
                g.record_arrival("proj-w-ran");
            }
            MockReply::Text("done".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let home_router = handle.create_router_session().unwrap();
    let proj_router = handle
        .create_router_project_session(AgentWorkspace {
            project_id: "p-rw".into(),
            root: project,
        })
        .unwrap();

    handle
        .submit_router_message(&home_router, 14001, "do-home")
        .unwrap();
    handle
        .submit_router_message(&proj_router, 14002, "do-proj")
        .unwrap();
    gate.wait_for_key("home-w-ran", Duration::from_secs(5));
    gate.wait_for_key("proj-w-ran", Duration::from_secs(5));

    let recorded = server.recorded();
    let home_req = recorded
        .iter()
        .find(|r| r.last_user == "home-write-brief")
        .expect("home worker request");
    let proj_req = recorded
        .iter()
        .find(|r| r.last_user == "proj-write-brief")
        .expect("proj worker request");

    for required in [
        "publish_file",
        "verify_deck",
        "verify_artifact",
        "write_file",
    ] {
        assert!(
            home_req.tool_names.iter().any(|t| t == required),
            "Home ReadWrite worker must expose `{required}`; got {:?}",
            home_req.tool_names
        );
    }
    for expected in ["write_file", "edit_file", "read_file", "glob", "grep"] {
        assert!(
            proj_req.tool_names.iter().any(|t| t == expected),
            "Project ReadWrite worker must expose `{expected}`; got {:?}",
            proj_req.tool_names
        );
    }
    assert!(
        !proj_req.tool_names.iter().any(|t| t == "publish_file"),
        "Project ReadWrite worker must NOT expose Home-only publish_file; got {:?}",
        proj_req.tool_names
    );
}

// =========================================================================
// Test 15: Unified "Qinghe Project" E2E Scenario (5 steps):
// 1. Quote sheet (parks on ToolApproval) + 3-slide PPT (proceeds in parallel);
// 2. Interject "会上提了 35 万，先记下来暂不改" -> 35万 preserved as Record /
//    PendingProposal while confirmed facts on Quote & PPT stay 30万;
// 3. Ask "现在能发给客户了吗？" -> query_work_status reports exact honest status;
// 4. Ambiguous edit triggers ask_user_choice (0 writes), resolve -> PPT v2,
//    delayed v1 completion event cannot overwrite v2 authoritative state;
// 5. Restart host -> approve Quote ToolApproval card strictly once, all objects
//    and versions accurately queryable!
// =========================================================================
#[test]
fn qinghe_e2e_approval_isolation_record_preservation_disambiguation_revision_gate_and_restart()
 {
    let root = temp_dir("qinghe-e2e");
    let gate = PromptGate::new();
    let ppt_work_slot = Arc::new(Mutex::new(String::new()));
    let ppt_work_c = Arc::clone(&ppt_work_slot);
    let quote_tool_execs = Arc::new(AtomicUsize::new(0));
    let qte_c = Arc::clone(&quote_tool_execs);
    let seen_work_batches = Arc::new(Mutex::new(Vec::<String>::new()));
    let swb_c = Arc::clone(&seen_work_batches);
    let status_snapshot = Arc::new(Mutex::new(String::new()));
    let ss_c = Arc::clone(&status_snapshot);

    let shell_tool_name = if cfg!(windows) { "powershell" } else { "bash" };
    let risky_cmd = if cfg!(windows) {
        "Remove-Item -Recurse -Force ./old_quotes"
    } else {
        "rm -rf ./old_quotes"
    };

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user.starts_with("[WorkEventBatch]") {
                    swb_c.lock().unwrap().push(req.last_user.clone());
                    if req.last_user.contains("stale=true") {
                        g.record_arrival("stale-batch-seen");
                    }
                    return MockReply::Text("收到后台任务进度更新".into());
                }
                // Step 1: Dispatch Quote worker + PPT worker.
                if req.last_user.contains("青禾项目")
                    && req.last_user.contains("30万")
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![
                        (
                            "qh-1".into(),
                            "dispatch_work".into(),
                            json!({
                                "brief": "生成青禾项目报价表 qinghe_quote.xlsx，报价30万",
                                "mode": "read_write",
                                "request_id": "qh-quote-1"
                            }),
                        ),
                        (
                            "qh-2".into(),
                            "dispatch_work".into(),
                            json!({
                                "brief": "生成青禾项目3页汇报PPT qinghe_formal.pptx，报价30万，日期2026-11-20",
                                "mode": "read_write",
                                "request_id": "qh-ppt-1"
                            }),
                        ),
                    ]);
                }
                // Step 2: Interjection recording 35万 proposal without modifying confirmed deliverables.
                if req.last_user.contains("35 万")
                    || req.last_user.contains("35万")
                {
                    return MockReply::Text(
                        "已记录待确认提案：会上提到 35 万，暂不修改正式报价表和汇报 PPT（当前已确认报价仍为 30 万）。"
                            .into(),
                    );
                }
                // Step 3: "现在能发给客户了吗？" -> must query authoritative status.
                if req.last_user.contains("现在能发给客户了吗") {
                    if let Some(tr) = &req.last_tool_result {
                        *ss_c.lock().unwrap() = tr.clone();
                        return MockReply::Text(
                            "还不能全部发送：报价表正在等待审批（0 核验文件），汇报 PPT 已完成。".into(),
                        );
                    }
                    return MockReply::ToolCalls(vec![(
                        "qh-status".into(),
                        "query_work_status".into(),
                        json!({}),
                    )]);
                }
                // Step 4a: Ambiguous edit -> ask_user_choice (0 writes).
                if req.last_user.contains("把那份的日期改成 11 月 30 日")
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "qh-choice".into(),
                        "ask_user_choice".into(),
                        json!({
                            "prompt": "要把哪份文件的日期改成 11 月 30 日？",
                            "impact_boundary": "仅修改所选文件，报价表与30万已确认报价保持原样",
                            "options": [
                                {"object_id": "obj-xlsx-1", "label": "qinghe_quote.xlsx", "extension": "xlsx"},
                                {"object_id": "obj-pptx-1", "label": "qinghe_formal.pptx", "extension": "pptx"}
                            ],
                            "allow_multi": false
                        }),
                    )]);
                }
                // Step 4b: Choice confirmed -> dispatch revision v2 to PPT worker.
                if req.last_user.contains("[CardChoiceConfirmed]") {
                    if req.last_tool_result.is_none() {
                        let ppt_wid = ppt_work_c.lock().unwrap().clone();
                        return MockReply::ToolCalls(vec![(
                            "qh-rev-ppt".into(),
                            "dispatch_work".into(),
                            json!({
                                "brief": "仅将 qinghe_formal.pptx 日期更新为 2026-11-30，保持3页与报价30万不变",
                                "target_work_id": ppt_wid,
                                "request_id": "qh-ppt-v2"
                            }),
                        )]);
                    }
                    g.record_arrival("ppt-v2-dispatched");
                    return MockReply::Text(
                        "已派工更新正式汇报 PPT 日期为 11 月 30 日".into(),
                    );
                }
                return MockReply::Text("已收到".into());
            }

            // Worker 1 (Quote): hits medium-risk shell command -> parks on ToolApproval!
            if req.last_user.contains("qinghe_quote.xlsx") {
                if req.last_tool_result.is_none() {
                    return MockReply::ToolCalls(vec![(
                        "qh-shell".into(),
                        shell_tool_name.to_owned(),
                        json!({"command": risky_cmd}),
                    )]);
                }
                return MockReply::Text("报价表已恢复并完成".into());
            }
            if req.last_user.contains("[ToolApproved]") {
                qte_c.fetch_add(1, Ordering::SeqCst);
                g.record_arrival("quote-approved-resumed");
                return MockReply::Text("报价表审批通过，已完成生成".into());
            }
            // Worker 2 (PPT): v1 and v2 go through real create_deck -> verify_deck -> publish_file!
            if req.last_user.contains("qinghe_formal.pptx")
                && req.last_user.contains("2026-11-20")
            {
                return match req.last_tool_result.as_deref() {
                    None => MockReply::ToolCalls(vec![(
                        "qh-ppt1-create".into(),
                        "create_deck".into(),
                        json!({
                            "path": "qinghe_formal.pptx",
                            "title": "青禾项目汇报",
                            "slides": [
                                "青禾项目汇报 报价30万 日期2026-11-20",
                                "核心方案与报价30万明细",
                                "交付排期与里程碑 2026-11-20"
                            ]
                        }),
                    )]),
                    Some(res) if res.contains("report_id") => {
                        MockReply::ToolCalls(vec![(
                            "qh-ppt1-pub".into(),
                            "publish_file".into(),
                            json!({
                                "path": "qinghe_formal.pptx",
                                "delivery_key": "qinghe_formal.pptx",
                                "display_name": "qinghe_formal.pptx"
                            }),
                        )])
                    }
                    Some(res)
                        if res.contains("artifact_id")
                            || res.contains("staged") =>
                    {
                        g.record_arrival("ppt-v1-done");
                        MockReply::Text("qinghe_formal.pptx v1 已完成".into())
                    }
                    Some(_) => MockReply::ToolCalls(vec![(
                        "qh-ppt1-verify".into(),
                        "verify_deck".into(),
                        json!({
                            "path": "qinghe_formal.pptx",
                            "slides": 3,
                            "anchors": [
                                {"anchor": "S0-title", "value": "青禾项目汇报"},
                                {"anchor": "S0-body", "value": "青禾项目汇报 报价30万 日期2026-11-20"},
                                {"anchor": "S1-title", "value": "青禾项目汇报 · 2"},
                                {"anchor": "S1-body", "value": "核心方案与报价30万明细"},
                                {"anchor": "S2-title", "value": "青禾项目汇报 · 3"},
                                {"anchor": "S2-body", "value": "交付排期与里程碑 2026-11-20"}
                            ]
                        }),
                    )]),
                };
            }
            if req.last_user.contains("2026-11-30") {
                return match req.last_tool_result.as_deref() {
                    None => MockReply::ToolCalls(vec![(
                        "qh-ppt2-create".into(),
                        "create_deck".into(),
                        json!({
                            "path": "qinghe_formal.pptx",
                            "title": "青禾项目汇报",
                            "slides": [
                                "青禾项目汇报 报价30万 日期2026-11-30",
                                "核心方案与报价30万明细",
                                "交付排期与里程碑 2026-11-30"
                            ]
                        }),
                    )]),
                    Some(res) if res.contains("report_id") => {
                        MockReply::ToolCalls(vec![(
                            "qh-ppt2-pub".into(),
                            "publish_file".into(),
                            json!({
                                "path": "qinghe_formal.pptx",
                                "delivery_key": "qinghe_formal.pptx",
                                "display_name": "qinghe_formal.pptx"
                            }),
                        )])
                    }
                    Some(res)
                        if res.contains("artifact_id")
                            || res.contains("staged") =>
                    {
                        g.record_arrival("ppt-v2-done");
                        MockReply::Text(
                            "qinghe_formal.pptx v2 (11月30日) 已完成".into(),
                        )
                    }
                    Some(_) => MockReply::ToolCalls(vec![(
                        "qh-ppt2-verify".into(),
                        "verify_deck".into(),
                        json!({
                            "path": "qinghe_formal.pptx",
                            "slides": 3,
                            "anchors": [
                                {"anchor": "S0-title", "value": "青禾项目汇报"},
                                {"anchor": "S0-body", "value": "青禾项目汇报 报价30万 日期2026-11-30"},
                                {"anchor": "S1-title", "value": "青禾项目汇报 · 2"},
                                {"anchor": "S1-body", "value": "核心方案与报价30万明细"},
                                {"anchor": "S2-title", "value": "青禾项目汇报 · 3"},
                                {"anchor": "S2-body", "value": "交付排期与里程碑 2026-11-30"}
                            ]
                        }),
                    )]),
                };
            }
            MockReply::Text("worker-ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let router_id = handle.create_router_session().unwrap();

    // --- Step 1: Dispatch Quote (blocks on ToolApproval) + PPT (completes) ---
    handle
        .submit_router_message(
            &router_id,
            15001,
            "帮我做青禾项目的30万报价表和三页汇报PPT",
        )
        .unwrap();
    gate.wait_for_key("ppt-v1-done", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            let works = handle.child_works(&router_id);
            works.len() == 2
                && works.iter().any(|w| w.state == "awaiting_approval")
                && works.iter().any(|w| {
                    w.state == "delivered"
                        && !w.delivered_artifact_ids.is_empty()
                })
        },
        "quote waiting for approval while ppt delivered",
    );
    let works = handle.child_works(&router_id);
    let quote_wid = works
        .iter()
        .find(|w| w.state == "awaiting_approval")
        .unwrap()
        .work_id
        .clone();
    let ppt_wid = works
        .iter()
        .find(|w| w.state == "delivered")
        .unwrap()
        .work_id
        .clone();
    *ppt_work_slot.lock().unwrap() = ppt_wid.clone();

    // Wait for any initial announcement to settle before Step 2.
    wait_idle(&handle, &router_id, "router idle after step 1");

    // --- Step 2: Interject "会上提了 35 万，先记下来暂不改" ---
    // Persist the 35万 meeting note as a Record (PendingProposal) on ppt worker
    // alongside the confirmed 30万 deliverable (verifying contract isolation).
    {
        let engine = lock(&handle.inner.engine).clone().unwrap();
        let mut ppt_wc = engine
            .session(&ppt_wid)
            .unwrap()
            .work_contract
            .unwrap_or_default();
        ppt_wc.objects.push(TrackedObject {
            object_id: "record-meeting-35w".into(),
            kind: TrackedObjectKind::Record,
            filename: None,
            extension: "md".into(),
            anchor_summary: "会上提了35万（待确认）".into(),
            overrides: BTreeMap::from([(
                "quote".to_owned(),
                FactEntry {
                    value: "35万".to_owned(),
                    status: FactStatus::PendingProposal,
                    updated_at_ms: now_ms(),
                },
            )]),
            verify_state: ObjectVerifyState::Unverified,
        });
        engine
            .set_session_work_contract(&ppt_wid, ppt_wc, None)
            .unwrap();
    }
    handle
        .submit_router_message(
            &router_id,
            15002,
            "会上提了 35 万，先记下来暂不改",
        )
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&router_id).is_busy()
                && handle.session_lines(&router_id).iter().any(|l| {
                    matches!(l, ChatLine::Assistant { text } if text.contains("35 万"))
                })
        },
        "step 2 interjection answered",
    );
    // Verify confirmed facts on BOTH Quote & PPT remain 30万 while 35万 is preserved in Record!
    {
        let engine = lock(&handle.inner.engine).clone().unwrap();
        let q_wc = engine.session(&quote_wid).unwrap().work_contract.unwrap();
        assert_eq!(
            q_wc.pinned_facts.get("quote").map(String::as_str),
            Some("30万")
        );
        let p_wc = engine.session(&ppt_wid).unwrap().work_contract.unwrap();
        assert_eq!(
            p_wc.confirmed_facts_for("obj-pptx-1")
                .get("quote")
                .map(String::as_str),
            Some("30万")
        );
        let rec_obj = p_wc.find_object("record-meeting-35w").unwrap();
        assert_eq!(rec_obj.kind, TrackedObjectKind::Record);
        assert_eq!(
            rec_obj.overrides.get("quote").unwrap().status,
            FactStatus::PendingProposal
        );
        assert_eq!(rec_obj.overrides.get("quote").unwrap().value, "35万");
    }

    // --- Step 3: Ask "现在能发给客户了吗？" ---
    handle
        .submit_router_message(&router_id, 15003, "现在能发给客户了吗？")
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&router_id).is_busy()
                && !status_snapshot.lock().unwrap().is_empty()
        },
        "step 3 status query completed",
    );
    let status_json: Value =
        serde_json::from_str(&status_snapshot.lock().unwrap()).unwrap();
    let queried_works = status_json["works"].as_array().unwrap();
    assert!(
        queried_works
            .iter()
            .any(|w| w["work_id"] == quote_wid
                && w["state"] == "awaiting_approval"),
        "query_work_status must report quote as awaiting_approval: {status_json}"
    );
    assert!(
        queried_works
            .iter()
            .any(|w| w["work_id"] == ppt_wid && w["state"] == "delivered"),
        "query_work_status must report ppt state honestly: {status_json}"
    );

    // --- Step 4: Ambiguous edit -> choice card -> resolve -> PPT v2 -> delayed v1 event ---
    handle
        .submit_router_message(
            &router_id,
            15004,
            "把那份的日期改成 11 月 30 日",
        )
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || handle.session_status(&router_id) == TurnStatus::WaitingForUser,
        "step 4 choice card parked on router",
    );
    let router_card = handle
        .root_pending_cards(&router_id)
        .into_iter()
        .find(|c| c.owner_session_id == router_id)
        .expect("router choice card");
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: router_id.clone(),
            card_id: router_card.card.card_id().to_owned(),
            selection: CardSelection::Disambiguate {
                object_ids: vec!["obj-pptx-1".into()],
            },
        })
        .expect("resolve qinghe disambiguation");
    gate.wait_for_key("ppt-v2-dispatched", Duration::from_secs(5));
    gate.wait_for_key("ppt-v2-done", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&router_id).is_busy()
                && handle
                    .child_works(&router_id)
                    .iter()
                    .find(|w| w.work_id == ppt_wid)
                    .is_some_and(|w| {
                        w.contract_revision.unwrap_or(0) >= 2
                            && w.state == "delivered"
                    })
        },
        "ppt v2 settled at contract_revision >= 2",
    );

    // Inject delayed old-revision (v1) completion event -> MUST be flagged stale
    // and NEVER overwrite v2 authoritative state!
    handle.inject_work_event_for_test(
        &router_id,
        &ppt_wid,
        Some("submission-delayed-v1".into()),
        Some(1),
    );
    gate.wait_for_key("stale-batch-seen", Duration::from_secs(5));
    wait_idle(&handle, &router_id, "stale batch announcement settled");
    assert!(
        seen_work_batches.lock().unwrap().iter().any(|b| b
            .contains("stale_event_superseded(event_rev=v1,current_rev=v2)")),
        "delayed v1 event must be marked superseded by v2: {:?}",
        *seen_work_batches.lock().unwrap()
    );
    let ppt_after_stale = handle
        .child_works(&router_id)
        .into_iter()
        .find(|w| w.work_id == ppt_wid)
        .unwrap();
    assert_eq!(ppt_after_stale.contract_revision, Some(2));

    // --- Step 5: Restart host, approve Quote ToolApproval card strictly once ---
    drop(handle);
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    reopened.set_autonomous_drain(true);

    let routed_cards = reopened.root_pending_cards(&router_id);
    assert_eq!(routed_cards.len(), 1);
    assert_eq!(routed_cards[0].owner_session_id, quote_wid);
    assert_eq!(routed_cards[0].owner_role, SessionRole::Worker);
    let (approval_card_id, approval_hash) = match &routed_cards[0].card {
        PendingCard::ToolApproval {
            card_id, args_hash, ..
        } => (card_id.clone(), args_hash.clone()),
        other @ PendingCard::Disambiguation { .. } => {
            panic!("expected ToolApproval card on quote worker, got {other:?}")
        }
    };

    reopened
        .apply_card_action(CardAction::ResolveCard {
            session_id: quote_wid.clone(),
            card_id: approval_card_id.clone(),
            selection: CardSelection::ApproveTool {
                args_hash: approval_hash.clone(),
            },
        })
        .expect("approve quote tool card after restart");
    gate.wait_for_key("quote-approved-resumed", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            reopened
                .child_works(&router_id)
                .iter()
                .find(|w| w.work_id == quote_wid)
                .is_some_and(|w| {
                    matches!(
                        w.state.as_str(),
                        "completed" | "partial" | "delivered"
                    ) && w.pending_cards.is_empty()
                })
        },
        "quote worker settled after approval",
    );
    assert_eq!(
        quote_tool_execs.load(Ordering::SeqCst),
        1,
        "approved quote worker must resume strictly once"
    );
    assert_eq!(
        reopened.apply_card_action(CardAction::ResolveCard {
            session_id: quote_wid,
            card_id: approval_card_id,
            selection: CardSelection::ApproveTool {
                args_hash: approval_hash,
            },
        }),
        Err("notice.card_not_found")
    );

    // Final cross-restart check of all objects, versions, and records.
    let final_works = reopened.child_works(&router_id);
    assert_eq!(final_works.len(), 2);
    let final_ppt = final_works.iter().find(|w| w.work_id == ppt_wid).unwrap();
    assert_eq!(final_ppt.contract_revision, Some(2));
    assert!(
        final_ppt
            .objects
            .iter()
            .any(|o| o.object_id == "record-meeting-35w"
                && o.kind == TrackedObjectKind::Record),
        "35万 meeting record must survive restart intact"
    );

    assert!(
        server.recorded().iter().any(|r| {
            is_router_req(r)
                && r.system_prompt.contains("[ChildWorksReadOnlySnapshot]")
        }),
        "Router turns must inject bounded [ChildWorksReadOnlySnapshot] read-only status header"
    );
}

// =========================================================================
// Test 16: PR #495 x PR-2A coupling guards:
// - `search_sessions` hides Workers on empty query, folds redacted Worker hits
//   into parent Router deduplicated by `root_session_id`, and skips
//   `SystemEvent` / `CardResume` control rows;
// - `set_session_archived` fails closed with `notice.agent_busy` while ANY
//   child Worker is running, holds active_claims, or has a queued strict receipt;
// - Direct `rename_session` / `set_session_pinned` / `set_session_archived` on
//   `SessionRole::Worker` are explicitly rejected;
// - `SessionRole::Worker` and `SystemEvent`/`CardResume` never trigger sidecar
//   `generate_title`;
// - `busy_session_ids()` continues reflecting active Workers when Router is idle;
// - `delete_session(router_id)` rejects when `pending_router_followups` is
//   non-empty and cleans `router_inboxes` on delete.
// =========================================================================
#[test]
fn pr495_coupling_search_folds_workers_skips_system_events_archive_checks_children_and_worker_skips_auto_title()
 {
    let root = temp_dir("pr495-coupling");
    let gate = PromptGate::new();

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(|req, g| {
            if is_router_req(req) {
                if req.last_user == "launch-nebula-research"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-neb".into(),
                        "dispatch_work".into(),
                        json!({
                            "brief": "worker-nebula-brief",
                            "mode": "read_only",
                            "request_id": "req-neb-1"
                        }),
                    )]);
                }
                if req.last_user == "launch-queued-only"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![(
                        "c-qonly".into(),
                        "dispatch_work".into(),
                        json!({
                            "brief": "worker-queued-only-brief",
                            "mode": "read_only",
                            "request_id": "req-qonly-1"
                        }),
                    )]);
                }
                if req.last_user == "hold-router-for-del" {
                    g.enter_and_wait("hold-router-for-del");
                    return MockReply::Text("router-del-done".into());
                }
                if req.last_user == "queued-followup-blocks-del" {
                    g.record_arrival("queued-followup-del-ran");
                    return MockReply::Text("followup-del-done".into());
                }
                if req.last_user.starts_with("[WorkEventBatch]") {
                    g.record_arrival("announce-nebula-done");
                    return MockReply::Text("后台研究已完成".into());
                }
                return MockReply::Text("已派工".into());
            }
            if req.last_user == "worker-nebula-brief" {
                g.enter_and_wait("worker-nebula-running");
                return MockReply::Text(
                    "Found nebula-quartz-9942 with api_key=sk-secret-worker-leak"
                        .into(),
                );
            }
            MockReply::Text("worker-ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();

    // Part A: With autonomous drain OFF, dispatch a queued-only strict worker ->
    // archiving parent Router MUST return `notice.agent_busy`!
    handle
        .submit_router_message(&router_id, 16001, "launch-queued-only")
        .unwrap();
    wait_idle(&handle, &router_id, "router queued-only turn settled");
    let queued_wid = handle.child_works(&router_id)[0].work_id.clone();
    assert_eq!(
        handle.set_session_archived(&router_id, true),
        Err("notice.agent_busy"),
        "archiving parent Router must fail closed while a child strict receipt is queued"
    );
    // Cancel the queued-only worker before testing the running worker path.
    handle.cancel_work(&router_id, &queued_wid).unwrap();

    // Part B: Enable autonomous drain and launch running Worker.
    handle.set_autonomous_drain(true);
    handle
        .submit_router_message(&router_id, 16002, "launch-nebula-research")
        .unwrap();
    gate.wait_for_key("worker-nebula-running", Duration::from_secs(5));
    wait_idle(
        &handle,
        &router_id,
        "router turn 2 idle while worker still running",
    );
    let worker_id = handle
        .child_works(&router_id)
        .into_iter()
        .find(|w| w.state == "running")
        .expect("running nebula worker")
        .work_id;

    // P2-2: `busy_session_ids()` still reflects active child Worker even when parent Router is Idle!
    assert!(
        handle.busy_session_ids().contains(&worker_id),
        "busy_session_ids must continue reflecting active child Workers for dock activity signal"
    );
    // P1-3: Archiving parent Router while child Worker is Running MUST fail with `notice.agent_busy`.
    assert_eq!(
        handle.set_session_archived(&router_id, true),
        Err("notice.agent_busy"),
        "archiving parent Router must fail closed while child Worker is running"
    );
    // P1-3: Direct metadata mutations on Worker are strictly rejected.
    assert_eq!(
        handle.rename_session(&worker_id, "Illegal Worker Rename"),
        Err("notice.agent_failed")
    );
    assert_eq!(
        handle.set_session_pinned(&worker_id, true),
        Err("notice.agent_failed")
    );
    assert_eq!(
        handle.set_session_archived(&worker_id, true),
        Err("notice.agent_failed")
    );
    assert_eq!(
        handle.delete_session(&worker_id),
        Err("notice.agent_failed")
    );

    // Release Worker -> completes with searchable keyword + secret -> Router gets WorkEventBatch.
    gate.release("worker-nebula-running");
    gate.wait_for_key("announce-nebula-done", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            !handle.session_status(&router_id).is_busy()
                && handle
                    .child_works(&router_id)
                    .iter()
                    .all(|w| w.state != "running" && w.state != "queued")
        },
        "worker and announcement settled",
    );

    // P1-4: Sidecar `generate_title` NEVER receives Worker prompts or `[WorkEventBatch]`!
    let titles = server.title_prompts();
    assert!(
        !titles.iter().any(|p| p.contains("worker-nebula-brief")
            || p.contains("worker-queued-only-brief")
            || p.contains("[WorkEventBatch]")),
        "Worker sessions and SystemEvent turns must never trigger sidecar generate_title: {titles:?}"
    );

    // P1-2: `search_sessions` hides Workers on empty query, folds redacted Worker hits into parent Router,
    // and skips `[WorkEventBatch]` / secrets!
    let recent = handle.search_sessions("", 20);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].session_id, router_id);

    let folded = handle.search_sessions("nebula-quartz-9942", 20);
    assert_eq!(
        folded.len(),
        1,
        "Worker match must fold deduplicated into parent Router: {folded:?}"
    );
    assert_eq!(folded[0].session_id, router_id);
    let snippet = folded[0].snippet.as_deref().unwrap_or("");
    assert!(
        snippet.contains("nebula-quartz-9942"),
        "folded snippet must include matched keyword: {snippet}"
    );
    assert!(
        !snippet.contains("sk-secret-worker-leak")
            && snippet.contains("[REDACTED]"),
        "folded Worker snippet must redact secrets: {snippet}"
    );
    assert_eq!(
        handle.search_sessions("sk-secret-worker-leak", 20).len(),
        0,
        "redacted secrets inside Worker must never be searchable"
    );
    assert_eq!(
        handle.search_sessions("WorkEventBatch", 20).len(),
        0,
        "SystemEvent announcements must never appear in search hits"
    );

    // Once all children settled, archiving & unarchiving parent Router succeeds cleanly!
    handle
        .set_session_archived(&router_id, true)
        .expect("archive settled router");
    handle
        .set_session_archived(&router_id, false)
        .expect("unarchive settled router");

    // P2-1: `delete_session(router_id)` rejects when `pending_router_followups` is non-empty,
    // and removes `router_inboxes` entry cleanly on deletion.
    handle
        .submit_router_message(&router_id, 16003, "hold-router-for-del")
        .unwrap();
    gate.wait_for_key("hold-router-for-del", Duration::from_secs(5));
    handle
        .submit_router_message(&router_id, 16004, "queued-followup-blocks-del")
        .unwrap();
    assert_eq!(
        handle.delete_session(&router_id),
        Err("notice.agent_busy"),
        "delete_session must refuse Router with active turn / pending_router_followups"
    );
    gate.release("hold-router-for-del");
    gate.wait_for_key("queued-followup-del-ran", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            handle.pending_router_followups(&router_id).is_empty()
                && !handle.inner.is_session_busy_or_active(&router_id)
        },
        "queued followup drained before delete",
    );
    handle.delete_session(&router_id).unwrap();
    assert!(
        !lock(&handle.inner.state.router_inboxes).contains_key(&router_id),
        "delete_session must clean router_inboxes entry"
    );
}

// =========================================================================
// Test 17: Deep code-review hardening guards:
// - `submit_router_message` rejects hidden `Worker` and `Standard` sessions;
// - `dispatch_work` and `control_work(action="revise")` without caller-supplied
//   `request_id` never pollute `SessionRecord::dispatch_requests`.
// =========================================================================
#[test]
fn review_hardening_guards_reject_non_router_submit_and_skip_auto_id_pollution()
{
    let root = temp_dir("review-guards");
    let gate = PromptGate::new();
    let seen_wid = Arc::new(Mutex::new(None::<String>));
    let seen_wid_c = Arc::clone(&seen_wid);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, _| {
            if is_router_req(req) {
                if req.last_user == "dispatch-without-req-id" {
                    if req.last_tool_result.is_none() {
                        return MockReply::ToolCalls(vec![(
                            "c-no-id".into(),
                            "dispatch_work".into(),
                            json!({
                                "brief": "ephemeral worker without request_id",
                                "mode": "read_only"
                            }),
                        )]);
                    }
                    if let Some(res) = &req.last_tool_result
                        && let Ok(val) = serde_json::from_str::<Value>(res)
                        && let Some(wid) = val["work_id"].as_str()
                    {
                        *seen_wid_c.lock().unwrap() = Some(wid.to_owned());
                    }
                    return MockReply::Text("dispatched-no-id".into());
                }
                if req.last_user == "revise-without-req-id" {
                    if req.last_tool_result.is_none() {
                        let wid = seen_wid_c
                            .lock()
                            .unwrap()
                            .clone()
                            .expect("worker id captured");
                        return MockReply::ToolCalls(vec![(
                            "c-rev-no-id".into(),
                            "control_work".into(),
                            json!({
                                "work_id": wid,
                                "action": "revise",
                                "revision_brief": "revised without request_id"
                            }),
                        )]);
                    }
                    return MockReply::Text("revised-no-id".into());
                }
                return MockReply::Text("ok".into());
            }
            MockReply::Text("worker-ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let standard_id = handle.create_home_session().unwrap();
    let router_id = handle.create_router_session().unwrap();

    // `submit_router_message` strictly rejects Standard sessions.
    assert_eq!(
        handle.submit_router_message(&standard_id, 17001, "nope"),
        Err("notice.agent_failed"),
        "submit_router_message must reject Standard sessions"
    );

    // Dispatch and revise WITHOUT `request_id` (spawn_blocking path exercised).
    handle
        .submit_router_message(&router_id, 17002, "dispatch-without-req-id")
        .unwrap();
    wait_idle(&handle, &router_id, "dispatch without request_id settled");
    let worker_id = seen_wid
        .lock()
        .unwrap()
        .clone()
        .expect("worker created without request_id");

    // `submit_router_message` strictly rejects hidden child Worker sessions.
    assert_eq!(
        handle.submit_router_message(&worker_id, 17003, "bypass router"),
        Err("notice.agent_failed"),
        "submit_router_message must reject hidden Worker sessions"
    );

    handle
        .submit_router_message(&router_id, 17004, "revise-without-req-id")
        .unwrap();
    wait_idle(&handle, &router_id, "revise without request_id settled");

    // Neither auto-generated dispatch nor revise may pollute `dispatch_requests`.
    let engine = lock(&handle.inner.engine).clone().unwrap();
    let persisted = engine.session(&router_id).unwrap();
    assert!(
        persisted.dispatch_requests.is_empty(),
        "omitting request_id must not pollute SessionRecord::dispatch_requests: {:?}",
        persisted.dispatch_requests
    );
}

// =========================================================================
// Test 18 (PR-2B Suite 2 / Q5): Identical follow-up texts with distinct
// submission_ids both survive restart projection rebuild and execute strictly
// once; queue accepts 1..=16 and rejects the 17th with `notice.router_queue_full`.
// =========================================================================
#[test]
fn pr2b_identical_followup_texts_survive_restart_and_queue_capacity_enforces_16()
 {
    let root = temp_dir("pr2b-identical-fifo-cap");
    let gate = PromptGate::new();
    let seen_after_restart = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_c = Arc::clone(&seen_after_restart);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if req.last_user == "hold-turn-1" {
                g.enter_and_wait("hold-turn-1");
                return MockReply::Text("turn-1-done".into());
            }
            if req.last_user == "hold-cap-turn" {
                g.enter_and_wait("hold-cap-turn");
                return MockReply::Text("cap-done".into());
            }
            if req.last_user == "identical-followup-cmd" {
                let count = {
                    let mut list = seen_c.lock().unwrap();
                    list.push(req.last_user.clone());
                    list.len()
                };
                if count == 2 {
                    g.record_arrival("both-identical-executed");
                }
                return MockReply::Text(format!("reply-#{count}"));
            }
            MockReply::Text(format!("ok:{}", req.last_user))
        }),
    );

    // Part A: Two identical follow-up texts with distinct submission_ids survive restart.
    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    let router_id = handle.create_router_session().unwrap();
    handle
        .submit_router_message(&router_id, 18001, "hold-turn-1")
        .unwrap();
    gate.wait_for_key("hold-turn-1", Duration::from_secs(5));

    handle
        .submit_router_message(&router_id, 18002, "identical-followup-cmd")
        .unwrap();
    handle
        .submit_router_message(&router_id, 18003, "identical-followup-cmd")
        .unwrap();
    assert_eq!(handle.pending_router_followups(&router_id).len(), 2);

    // Simulate crash while Turn 1 is still active and both identical follow-ups are persisted.
    drop(handle);

    // Reopen fresh handle: boot() must keep BOTH identical user bubbles and run BOTH.
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    gate.wait_for_key("both-identical-executed", Duration::from_secs(5));
    wait_until(
        Duration::from_secs(5),
        || {
            reopened.pending_router_followups(&router_id).is_empty()
                && !reopened.session_status(&router_id).is_busy()
        },
        "both identical follow-ups completed after restart",
    );
    assert_eq!(
        *seen_after_restart.lock().unwrap(),
        vec![
            "identical-followup-cmd".to_owned(),
            "identical-followup-cmd".to_owned(),
        ],
        "two identical follow-up texts with distinct submission_ids must both execute"
    );
    let user_bubble_count = reopened
        .session_lines(&router_id)
        .iter()
        .filter(|line| {
            matches!(line, ChatLine::User { text } if text == "identical-followup-cmd")
        })
        .count();
    assert_eq!(
        user_bubble_count, 2,
        "both identical user bubbles must remain projected after restart"
    );

    // Part B: Capacity limit 1..=16 succeed, 17th fails with `notice.router_queue_full`.
    let cap_router = reopened.create_router_session().unwrap();
    reopened
        .submit_router_message(&cap_router, 18100, "hold-cap-turn")
        .unwrap();
    gate.wait_for_key("hold-cap-turn", Duration::from_secs(5));
    for i in 1..=16_u128 {
        assert_eq!(
            reopened
                .submit_router_message(
                    &cap_router,
                    18100 + i,
                    &format!("queued-{i}"),
                )
                .as_deref(),
            Ok(cap_router.as_str()),
            "follow-up #{i} within 16-item capacity must succeed"
        );
    }
    assert_eq!(reopened.pending_router_followups(&cap_router).len(), 16);
    assert_eq!(
        reopened.submit_router_message(&cap_router, 18117, "overflow-17"),
        Err("notice.router_queue_full"),
        "17th follow-up must fail closed with notice.router_queue_full"
    );
    assert_eq!(reopened.pending_router_followups(&cap_router).len(), 16);

    gate.release_all();
    drop(reopened);
}

// =========================================================================
// Test 19 (PR-2B Suites 2 & 3 / Q3 & Q4): Two child Workers awaiting approval
// stay isolated (resolving W1 leaves W2 pending while Router keeps chatting);
// child Verification + Artifact project into parent Router strictly once across
// refresh & restart; child override notifies parent Router and persists badge.
// =========================================================================
#[test]
fn pr2b_two_workers_approval_isolation_child_card_projection_and_override() {
    let root = temp_dir("pr2b-two-workers-proj");
    let gate = PromptGate::new();
    let shell_tool_name = if cfg!(windows) { "powershell" } else { "bash" };
    let risky_cmd = if cfg!(windows) {
        "Remove-Item -Recurse -Force ./stale_cache"
    } else {
        "rm -rf ./stale_cache"
    };

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, g| {
            if is_router_req(req) {
                if req.last_user == "launch-two-workers"
                    && req.last_tool_result.is_none()
                {
                    return MockReply::ToolCalls(vec![
                        (
                            "c-w1".into(),
                            "dispatch_work".into(),
                            json!({"brief": "worker-one-approval", "request_id": "req-w1"}),
                        ),
                        (
                            "c-w2".into(),
                            "dispatch_work".into(),
                            json!({"brief": "worker-two-approval", "request_id": "req-w2"}),
                        ),
                    ]);
                }
                if req.last_user == "chat-while-both-parked" {
                    g.record_arrival("router-replied-while-workers-parked");
                    return MockReply::Text("router-answered-2nd-msg".into());
                }
                return MockReply::Text("router-ok".into());
            }

            if req.last_user.contains("worker-one-approval") {
                if req.last_tool_result.is_none() {
                    return MockReply::ToolCalls(vec![(
                        "w1-risky".into(),
                        shell_tool_name.to_owned(),
                        json!({"command": risky_cmd}),
                    )]);
                }
                return MockReply::Text("w1-finished".into());
            }
            if req.last_user.contains("worker-two-approval") {
                if req.last_tool_result.is_none() {
                    return MockReply::ToolCalls(vec![(
                        "w2-risky".into(),
                        shell_tool_name.to_owned(),
                        json!({"command": risky_cmd}),
                    )]);
                }
                return MockReply::Text("w2-finished".into());
            }
            if req.last_user.contains("[ToolApproved]")
                && req.last_tool_result.is_none()
            {
                return MockReply::ToolCalls(vec![(
                    "w1-deck".into(),
                    "create_deck".into(),
                    json!({
                        "path": "w1_bad.pptx",
                        "title": "W1 Deck",
                        "slides": ["Only 1 slide"]
                    }),
                )]);
            }
            if let Some(res) = req.last_tool_result.as_deref()
                && res.contains("w1_bad.pptx")
                && !res.contains("report_id")
            {
                return MockReply::ToolCalls(vec![(
                    "w1-verify".into(),
                    "verify_deck".into(),
                    json!({
                        "path": "w1_bad.pptx",
                        "slides": 3,
                        "anchors": [{"anchor": "S2-title", "value": "Missing Slide"}]
                    }),
                )]);
            }
            g.record_arrival("w1-verified-failed");
            MockReply::Text("w1-needs-override".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let router_id = handle.create_router_session().unwrap();
    handle.select_session(&router_id).unwrap();

    handle
        .submit_router_message(&router_id, 19001, "launch-two-workers")
        .unwrap();
    wait_until(
        Duration::from_secs(8),
        || handle.root_pending_cards(&router_id).len() == 2,
        "both child workers parked on ToolApproval",
    );

    let cards = handle.root_pending_cards(&router_id);
    assert_eq!(cards.len(), 2);
    assert!(
        cards.iter().all(|c| c.owner_role == SessionRole::Worker
            && c.owner_session_id != router_id),
        "both pending cards must belong to child Workers: {cards:?}"
    );
    assert!(
        handle.session_pending_cards(&router_id).is_empty(),
        "Router itself has no own pending cards"
    );

    // Q4: While both Workers await approval, Router still accepts and replies to 2nd message!
    handle
        .submit_router_message(&router_id, 19002, "chat-while-both-parked")
        .unwrap();
    gate.wait_for_key(
        "router-replied-while-workers-parked",
        Duration::from_secs(5),
    );
    wait_idle(&handle, &router_id, "router 2nd message settled");

    // Resolve ONLY W1's approval card -> W1 resumes and emits failed Verification card;
    // W2 stays untouched in `awaiting_approval`!
    let w1_card = cards[0].clone();
    let w2_card = cards[1].clone();
    let w1_args_hash = match &w1_card.card {
        PendingCard::ToolApproval { args_hash, .. } => args_hash.clone(),
        other @ PendingCard::Disambiguation { .. } => {
            panic!("expected ToolApproval card on W1, got {other:?}")
        }
    };
    handle
        .apply_card_action(CardAction::ResolveCard {
            session_id: w1_card.owner_session_id.clone(),
            card_id: w1_card.card.card_id().to_owned(),
            selection: CardSelection::ApproveTool {
                args_hash: w1_args_hash,
            },
        })
        .expect("approve W1 tool card");

    gate.wait_for_key("w1-verified-failed", Duration::from_secs(8));
    wait_until(
        Duration::from_secs(8),
        || {
            handle.chat_history().rows.iter().any(|line| {
                matches!(
                    line,
                    ChatLine::Verification {
                        source_session_id: Some(src),
                        ok: false,
                        ..
                    } if src == &w1_card.owner_session_id
                )
            })
        },
        "W1 failed verification card projected onto parent Router timeline",
    );

    // W2's approval card must remain intact and isolated!
    let remaining_cards = handle.root_pending_cards(&router_id);
    assert_eq!(remaining_cards.len(), 1);
    assert_eq!(
        remaining_cards[0].owner_session_id,
        w2_card.owner_session_id
    );

    // Projected verification card appears strictly ONCE in parent Router chat_history()
    // and carries `source_session_id = Some(w1_id)`.
    let projected_v: Vec<_> = handle
        .chat_history()
        .rows
        .into_iter()
        .filter_map(|line| match line {
            ChatLine::Verification {
                report_id,
                artifact_path,
                override_used,
                source_session_id,
                ..
            } => Some((
                report_id,
                artifact_path,
                override_used,
                source_session_id,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        projected_v.len(),
        1,
        "child Verification must appear strictly once on parent Router timeline"
    );
    let (report_id, artifact_path, override_used, source_sid) =
        projected_v[0].clone();
    assert!(!override_used);
    assert_eq!(
        source_sid.as_deref(),
        Some(w1_card.owner_session_id.as_str())
    );

    // Q3: Override targeting explicit `w1_id` commits artifact, emits Session update
    // for parent Router, and updates projected verification + artifact strictly once!
    let _ = handle.drain_updates();
    let artifact_id = handle
        .override_deliver_artifact(
            &w1_card.owner_session_id,
            &report_id,
            &artifact_path,
        )
        .expect("override child W1 verification card");
    let updates = handle.drain_updates();
    assert!(
        updates.iter().any(
            |u| matches!(u, AgentUpdate::Session { id } if id == &router_id)
        ),
        "overriding child Worker must emit AgentUpdate::Session for parent Router: {updates:?}"
    );
    assert_eq!(handle.root_session_id(&w1_card.owner_session_id), router_id);

    // Check once before restart AND once after restart -> strictly 1 verification (overridden)
    // and strictly 1 artifact row on parent Router timeline!
    let assert_projected_once = |h: &AgentHandle| {
        let rows = h.chat_history().rows;
        let v_count = rows
            .iter()
            .filter(|l| {
                matches!(
                    l,
                    ChatLine::Verification {
                        report_id: rid,
                        override_used: true,
                        source_session_id: Some(src),
                        ..
                    } if rid == &report_id && src == &w1_card.owner_session_id
                )
            })
            .count();
        let a_count = rows
            .iter()
            .filter(|l| {
                matches!(l, ChatLine::Artifact { artifact_id: aid } if aid == &artifact_id)
            })
            .count();
        assert_eq!(
            v_count, 1,
            "overridden child Verification must appear strictly once"
        );
        assert_eq!(
            a_count, 1,
            "committed child Artifact must appear strictly once"
        );
    };
    assert_projected_once(&handle);

    drop(handle);
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    reopened.select_session(&router_id).unwrap();
    assert_projected_once(&reopened);
}

#[test]
fn router_cold_start_draft_restarts_into_project_router_and_cancels_deleted_router_target()
 {
    let root = temp_dir("cold-start-restart-router");
    let proj_a_dir = root.join("proj-a");
    std::fs::create_dir_all(&proj_a_dir).unwrap();
    let gate = PromptGate::new();
    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(|req, _| {
            if is_router_req(req) {
                return MockReply::Text("router-cold-reply".into());
            }
            MockReply::Text("worker-ok".into())
        }),
    );

    // Pre-restart: enqueue Project A cold-start draft while offline/unconfigured,
    // plus a strict receipt targeting an existing Router that will be deleted.
    {
        let offline =
            AgentHandle::with_config(root.to_path_buf(), LlmConfig::default());
        while offline.is_initializing() {
            std::thread::sleep(Duration::from_millis(5));
        }
        let to_delete = offline.create_router_session().unwrap();
        let strict_orphan = offline
            .accept_deferred_router(&to_delete, "never resurrect home")
            .unwrap();
        assert!(strict_orphan.strict_target);
        offline.delete_session(&to_delete).unwrap();
        assert_eq!(
            offline.receipt_state(&strict_orphan.id),
            Some(flowy_agent_core::ReceiptState::Cancelled),
            "deleting target Router cancels its strict deferred receipt"
        );

        let keep_router = offline.create_router_session().unwrap();
        let existing_receipt = offline
            .accept_deferred_router(&keep_router, "resume existing router")
            .unwrap();
        assert!(existing_receipt.strict_target);

        let draft = offline
            .accept_deferred_draft(
                "deliver in proj A after restart",
                SessionOrigin::Project {
                    project_id: Some("proj-a".into()),
                },
                Some(AgentWorkspace {
                    project_id: "proj-a".into(),
                    root: proj_a_dir,
                }),
                SessionRole::Router,
            )
            .unwrap();
        assert_eq!(
            draft.draft_target.as_deref().map(|d| d.role),
            Some(SessionRole::Router)
        );
        assert_eq!(offline.pending_acceptances().len(), 2);
    }

    // Reopen with live runnable config and autonomous drain -> deleted Router
    // stayed cancelled, existing Router receipt drains into `keep_router`, and
    // the Project A draft receipt creates strictly one Project A Router.
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    reopened.set_autonomous_drain(true);
    wait_until(
        Duration::from_secs(5),
        || {
            reopened.pending_acceptances().is_empty()
                && reopened.visible_acceptances().is_empty()
                && reopened.list_sessions().len() == 2
        },
        "cold-start Project A Router and existing Router receipts complete",
    );
    let sessions = reopened.list_sessions();
    assert_eq!(sessions.len(), 2);
    let proj_a_router = sessions
        .iter()
        .find(|s| s.project_id.as_deref() == Some("proj-a"))
        .expect("Project A Router materialized");
    assert_eq!(
        reopened.session_role(&proj_a_router.id),
        SessionRole::Router
    );
    assert!(reopened.session_lines(&proj_a_router.id).iter().any(|l| {
        matches!(l, ChatLine::Assistant { text } if text.contains("router-cold-reply"))
    }));
    assert!(
        sessions.iter().any(|s| {
            s.project_id.is_none()
                && reopened.session_role(&s.id) == SessionRole::Router
                && reopened.session_lines(&s.id).iter().any(|l| {
                    matches!(l, ChatLine::User { text } if text == "resume existing router")
                })
        }),
        "kept Router received its persisted cold-start message without spawning extra Home sessions"
    );
}

#[test]
fn router_timeline_anchors_child_cards_by_dispatch_and_revise_turn_across_restart()
 {
    let root = temp_dir("timeline-turn-anchor");
    let gate = PromptGate::new();
    let w1_id_slot: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
    let w1_id_slot_c = Arc::clone(&w1_id_slot);

    let server = RouterMockServer::start(
        Arc::clone(&gate),
        Arc::new(move |req, _g| {
            if is_router_req(req) {
                if req.last_user.contains("deliver alpha") {
                    if req.last_tool_result.is_none() {
                        return MockReply::ToolCalls(vec![(
                            "c-disp-a".into(),
                            "dispatch_work".into(),
                            json!({
                                "brief": "worker-alpha",
                                "mode": "read_write",
                                "request_id": "req-alpha"
                            }),
                        )]);
                    }
                    return MockReply::Text("dispatched-alpha".into());
                }
                if req.last_user.contains("just chatting") {
                    return MockReply::Text("chat-only-reply".into());
                }
                if req.last_user.contains("deliver beta") {
                    if req.last_tool_result.is_none() {
                        return MockReply::ToolCalls(vec![(
                            "c-disp-b".into(),
                            "dispatch_work".into(),
                            json!({
                                "brief": "worker-beta",
                                "mode": "read_write",
                                "request_id": "req-beta"
                            }),
                        )]);
                    }
                    return MockReply::Text("dispatched-beta".into());
                }
                if req.last_user.contains("revise alpha") {
                    if req.last_tool_result.is_none() {
                        let wid = w1_id_slot_c
                            .lock()
                            .unwrap()
                            .clone()
                            .expect("w1_id populated before revise");
                        return MockReply::ToolCalls(vec![(
                            "c-rev-a".into(),
                            "control_work".into(),
                            json!({
                                "work_id": wid,
                                "action": "revise",
                                "revision_brief": "worker-alpha-v2",
                                "request_id": "req-alpha-revise"
                            }),
                        )]);
                    }
                    return MockReply::Text("revised-alpha".into());
                }
                return MockReply::Text("router-ack".into());
            }
            if req.last_user.contains("worker-alpha-v2") {
                return match req.last_tool_result.as_deref() {
                    None => MockReply::ToolCalls(vec![(
                        "c-d-a2".into(),
                        "create_deck".into(),
                        json!({ "path": "alpha_v2.pptx", "title": "A2", "slides": ["S1", "S2", "S3"] }),
                    )]),
                    Some(res) if res.contains("report_id") => {
                        MockReply::Text("alpha-v2-done".into())
                    }
                    Some(_) => MockReply::ToolCalls(vec![(
                        "c-v-a2".into(),
                        "verify_deck".into(),
                        json!({ "path": "alpha_v2.pptx", "slides": 3, "anchors": [{"anchor": "S2-title", "value": "Missing"}] }),
                    )]),
                };
            }
            if req.last_user.contains("worker-alpha") {
                return match req.last_tool_result.as_deref() {
                    None => MockReply::ToolCalls(vec![(
                        "c-d-a1".into(),
                        "create_deck".into(),
                        json!({ "path": "alpha_v1.pptx", "title": "A1", "slides": ["S1", "S2", "S3"] }),
                    )]),
                    Some(res) if res.contains("report_id") => {
                        MockReply::Text("alpha-v1-done".into())
                    }
                    Some(_) => MockReply::ToolCalls(vec![(
                        "c-v-a1".into(),
                        "verify_deck".into(),
                        json!({ "path": "alpha_v1.pptx", "slides": 3, "anchors": [{"anchor": "S2-title", "value": "Missing"}] }),
                    )]),
                };
            }
            if req.last_user.contains("worker-beta") {
                return match req.last_tool_result.as_deref() {
                    None => MockReply::ToolCalls(vec![(
                        "c-d-b1".into(),
                        "create_deck".into(),
                        json!({ "path": "beta_v1.pptx", "title": "B1", "slides": ["S1", "S2", "S3"] }),
                    )]),
                    Some(res) if res.contains("report_id") => {
                        MockReply::Text("beta-v1-done".into())
                    }
                    Some(_) => MockReply::ToolCalls(vec![(
                        "c-v-b1".into(),
                        "verify_deck".into(),
                        json!({ "path": "beta_v1.pptx", "slides": 3, "anchors": [{"anchor": "S2-title", "value": "Missing"}] }),
                    )]),
                };
            }
            MockReply::Text("worker-ok".into())
        }),
    );

    let handle = AgentHandle::with_config(root.to_path_buf(), server.config());
    handle.set_autonomous_drain(true);
    let router_id = handle.create_router_session().unwrap();
    handle.select_session(&router_id).unwrap();

    let wait_children_done = |label: &str| {
        wait_until(
            Duration::from_secs(6),
            || {
                let works = handle.child_works(&router_id);
                !works.is_empty()
                    && works.iter().all(|w| {
                        !matches!(
                            w.state.as_str(),
                            "queued" | "running" | "verifying" | "stopping"
                        )
                    })
            },
            label,
        );
    };

    // Turn 1: deliver alpha -> Worker 1 emits Verification + override commits Artifact.
    handle
        .submit_router_message(&router_id, 7001, "deliver alpha")
        .unwrap();
    wait_until(
        Duration::from_secs(6),
        || {
            handle.chat_history().rows.iter().any(|l| {
                matches!(l, ChatLine::Verification { artifact_path, .. } if artifact_path.ends_with("alpha_v1.pptx"))
            })
        },
        "alpha_v1 verification card projected",
    );
    wait_children_done("w1 turn 1 settled");
    wait_idle(&handle, &router_id, "turn 1 settled");

    let (w1_id, rep_a1, path_a1) = handle
        .chat_history()
        .rows
        .iter()
        .find_map(|l| match l {
            ChatLine::Verification {
                report_id,
                artifact_path,
                source_session_id: Some(src),
                ..
            } if artifact_path.ends_with("alpha_v1.pptx") => {
                Some((src.clone(), report_id.clone(), artifact_path.clone()))
            }
            _ => None,
        })
        .expect("w1 alpha_v1 verification card");
    *w1_id_slot.lock().unwrap() = Some(w1_id.clone());
    let art_a1 = handle
        .override_deliver_artifact(&w1_id, &rep_a1, &path_a1)
        .expect("override deliver alpha_v1");

    // Turn 2: plain conversational follow-up (no dispatch).
    handle
        .submit_router_message(&router_id, 7002, "just chatting")
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        || {
            handle.chat_history().rows.iter().any(|l| {
                matches!(l, ChatLine::Assistant { text } if text.contains("chat-only-reply"))
            })
        },
        "turn 2 chat reply landed",
    );

    let find_user = |rows: &[ChatLine], needle: &str| -> usize {
        rows.iter()
            .position(|l| matches!(l, ChatLine::User { text } if text.contains(needle)))
            .unwrap_or_else(|| panic!("missing user turn {needle}"))
    };
    let find_verif = |rows: &[ChatLine], suffix: &str| -> usize {
        rows.iter()
            .position(|l| {
                matches!(l, ChatLine::Verification { artifact_path, .. } if artifact_path.ends_with(suffix))
            })
            .unwrap_or_else(|| panic!("missing verification {suffix}"))
    };

    // Assert Turn 1 cards stay strictly before Turn 2 User.
    let assert_pre_beta = |h: &AgentHandle| {
        let rows = h.chat_history().rows;
        let t1 = find_user(&rows, "deliver alpha");
        let t2 = find_user(&rows, "just chatting");
        let va = find_verif(&rows, "alpha_v1.pptx");
        let aa = rows
            .iter()
            .position(|l| matches!(l, ChatLine::Artifact { artifact_id } if artifact_id == &art_a1))
            .expect("artifact A present");
        assert!(
            t1 < va && va < t2 && t1 < aa && aa < t2,
            "Turn 1 cards must sit strictly between Turn 1 and Turn 2: t1={t1} va={va} aa={aa} t2={t2}"
        );
    };
    assert_pre_beta(&handle);

    // Turn 3: deliver beta -> Worker 2 emits beta_v1.pptx in Turn 3.
    handle
        .submit_router_message(&router_id, 7003, "deliver beta")
        .unwrap();
    wait_until(
        Duration::from_secs(6),
        || {
            handle.chat_history().rows.iter().any(|l| {
                matches!(l, ChatLine::Verification { artifact_path, .. } if artifact_path.ends_with("beta_v1.pptx"))
            })
        },
        "beta_v1 verification card projected",
    );
    wait_children_done("w2 turn 3 settled");
    wait_idle(&handle, &router_id, "turn 3 settled");

    // Turn 4: revise alpha via control_work(revise) targeting `w1_id`.
    handle
        .submit_router_message(&router_id, 7004, "revise alpha")
        .unwrap();
    wait_until(
        Duration::from_secs(6),
        || {
            handle.chat_history().rows.iter().any(|l| {
                matches!(l, ChatLine::Verification { artifact_path, .. } if artifact_path.ends_with("alpha_v2.pptx"))
            })
        },
        "alpha_v2 verification card projected after revise",
    );
    wait_children_done("w1 turn 4 settled");
    wait_idle(&handle, &router_id, "turn 4 settled");

    let assert_after_revise = |h: &AgentHandle| {
        let rows = h.chat_history().rows;
        let t2 = find_user(&rows, "just chatting");
        let t3 = find_user(&rows, "deliver beta");
        let t4 = find_user(&rows, "revise alpha");
        let vb = find_verif(&rows, "beta_v1.pptx");
        let va2 = find_verif(&rows, "alpha_v2.pptx");
        let va1_count = rows
            .iter()
            .filter(|l| matches!(l, ChatLine::Verification { artifact_path, .. } if artifact_path.ends_with("alpha_v1.pptx")))
            .count();
        let va2_count = rows
            .iter()
            .filter(|l| matches!(l, ChatLine::Verification { artifact_path, .. } if artifact_path.ends_with("alpha_v2.pptx")))
            .count();
        let aa_count = rows
            .iter()
            .filter(|l| matches!(l, ChatLine::Artifact { artifact_id } if artifact_id == &art_a1))
            .count();
        assert_eq!(va1_count, 1);
        assert_eq!(va2_count, 1);
        assert_eq!(aa_count, 1);
        // Revised `w1_id` now anchors to Turn 4 (so its cards move after t4),
        // while `w2` stays anchored to Turn 3 (strictly between t3 and t4).
        assert!(t2 < t3 && t3 < vb && vb < t4 && t4 < va2);
        let va1 = find_verif(&rows, "alpha_v1.pptx");
        assert!(
            t4 < va1,
            "revised worker history belongs to latest Turn 4: t4={t4} va1={va1}"
        );
    };
    assert_after_revise(&handle);

    // Restart and re-verify exact placement and deduplication survive persistence.
    drop(handle);
    let reopened =
        AgentHandle::with_config(root.to_path_buf(), server.config());
    while reopened.is_initializing() {
        std::thread::sleep(Duration::from_millis(5));
    }
    reopened.select_session(&router_id).unwrap();
    assert_after_revise(&reopened);
}

#[test]
fn chat_history_multi_router_and_active_workers_zero_clone_benchmark() {
    use flowy_agent_core::{
        ContentBlock, Message, MessageSource, Role, SessionOrigin,
        SessionRecord, SessionRole, WorkAccessMode,
    };

    let sample_messages = |prefix: &str, count: usize| -> Vec<Message> {
        (0..count)
            .map(|idx| Message {
                id: format!("{prefix}-m-{idx}"),
                role: if idx % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                content: vec![ContentBlock::text(format!(
                    "worker payload chunk {idx}"
                ))],
                source: MessageSource::User,
                display_text: None,
            })
            .collect()
    };

    let root = temp_dir("m1-bench-zero-clone");
    let handle =
        AgentHandle::with_config(root.to_path_buf(), LlmConfig::default());
    while handle.is_initializing() {
        std::thread::sleep(Duration::from_millis(5));
    }
    let engine = lock(&handle.inner.engine)
        .clone()
        .expect("engine initialized");

    for r in 0..50_usize {
        let rid = format!("r-hist-{r:03}");
        engine
            .create_session_with_role(
                rid.clone(),
                1_000 + r as u64,
                SessionOrigin::Home,
                None,
                SessionRole::Router,
                None,
                WorkAccessMode::ReadWrite,
            )
            .unwrap();
        for w in 0..2_usize {
            let wid = format!("w-hist-{r:03}-{w}");
            engine
                .insert_session(SessionRecord {
                    id: wid.clone(),
                    updated_at: 1_000 + r as u64,
                    origin: SessionOrigin::Home,
                    role: SessionRole::Worker,
                    root_session_id: Some(rid.clone()),
                    access_mode: WorkAccessMode::ReadOnly,
                    messages: sample_messages(&wid, 20),
                    ..SessionRecord::default()
                })
                .unwrap();
        }
    }

    let active_router = "r-active-bench".to_owned();
    engine
        .create_session_with_role(
            active_router.clone(),
            5_000,
            SessionOrigin::Home,
            None,
            SessionRole::Router,
            None,
            WorkAccessMode::ReadWrite,
        )
        .unwrap();
    for w in 1..=2_usize {
        let wid = format!("w-active-{w}");
        engine
            .insert_session(SessionRecord {
                id: wid.clone(),
                updated_at: 5_000 + w as u64,
                origin: SessionOrigin::Home,
                role: SessionRole::Worker,
                root_session_id: Some(active_router.clone()),
                access_mode: WorkAccessMode::ReadOnly,
                messages: sample_messages(&wid, 20),
                ..SessionRecord::default()
            })
            .unwrap();
    }
    drop(engine);
    drop(handle);

    let reopened =
        AgentHandle::with_config(root.to_path_buf(), LlmConfig::default());
    while reopened.is_initializing() {
        std::thread::sleep(Duration::from_millis(5));
    }
    reopened.select_session(&active_router).unwrap();
    let mut router_lines = vec![
        ChatLine::User {
            text: "Run parallel tasks".into(),
        },
        ChatLine::Assistant {
            text: "Dispatched two active workers.".into(),
        },
    ];
    for step in 0..20_usize {
        router_lines.push(ChatLine::ToolCall {
            call_id: format!("step-{step}"),
            name: "query_work_status".into(),
            target: "w-active-1".into(),
            running: false,
        });
    }
    let router_proj = ChatProjection::from_lines(router_lines);
    let worker_projs: Vec<(String, ChatProjection)> = (1..=2_usize)
        .map(|w| {
            let wid = format!("w-active-{w}");
            let mut w_lines = Vec::with_capacity(24);
            for s in 0..20_usize {
                w_lines.push(ChatLine::Assistant {
                    text: format!("stream chunk {s} for {wid}"),
                });
            }
            w_lines.push(ChatLine::Verification {
                report_id: format!("rep-bench-{w}"),
                artifact_path: format!(r"C:\bench\out-{w}.md"),
                artifact_hash: format!("hash-{w}"),
                ok: true,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: Some(wid.clone()),
            });
            (wid, ChatProjection::from_lines(w_lines))
        })
        .collect();
    {
        let mut live = lock(&reopened.inner.state.live);
        live.get_mut(&active_router)
            .expect("active router hydrated into live")
            .projection = router_proj;
        for (wid, proj) in worker_projs {
            live.get_mut(&wid)
                .expect("active child worker hydrated into live")
                .projection = proj;
        }
        drop(live);
    }

    // Warm up once, then sample 100 chat_history() projections across 50 routers.
    let warmup = reopened.chat_history();
    assert_eq!(
        reopened.active_session_id().as_deref(),
        Some(active_router.as_str())
    );
    let has_verifications = |rows: &[ChatLine]| {
        let has_w1 = rows.iter().any(|l| {
            matches!(
                l,
                ChatLine::Verification {
                    report_id,
                    source_session_id: Some(src),
                    ..
                } if report_id == "rep-bench-1" && src == "w-active-1"
            )
        });
        let has_w2 = rows.iter().any(|l| {
            matches!(
                l,
                ChatLine::Verification {
                    report_id,
                    source_session_id: Some(src),
                    ..
                } if report_id == "rep-bench-2" && src == "w-active-2"
            )
        });
        has_w1 && has_w2
    };
    assert!(
        has_verifications(&warmup.rows),
        "projected chat history must surface both active child Worker cards"
    );

    let start = std::time::Instant::now();
    let mut last_rows = Vec::new();
    for _ in 0..100 {
        last_rows = reopened.chat_history().rows;
    }
    let elapsed_us = start.elapsed().as_micros();
    println!(
        "[M1-BENCH] 50 historical Routers + 2 active streaming Workers: chat_history() x100 = {elapsed_us}us ({:.2}us/call), visible_rows={}",
        elapsed_us as f64 / 100.0,
        last_rows.len()
    );
    assert!(has_verifications(&last_rows));
}

// =========================================================================
// Test 20 (PR-2B Suite 4): Live-cloud Router 2-message non-blocking E2E.
// Same Router conversation dispatches a real workspace Worker task, verifies
// Worker 1 is active & unfinished, submits a 2nd user message on the same
// conversation, verifies Router answers the 2nd message independently while
// Worker 1 continues, and logs full IDs/timestamps for release evidence.
// =========================================================================
#[test]
#[ignore = "hits live cloud gateway server.flowyaipc.com using local Flowy auth session"]
fn pr2b_live_cloud_router_two_messages_non_blocking_e2e() {
    let appdata =
        std::env::var("APPDATA").expect("APPDATA env var must be set");
    let session_path = std::path::PathBuf::from(appdata)
        .join("Flowy")
        .join("Flowy")
        .join("data")
        .join("auth")
        .join("session.json");
    let raw = std::fs::read_to_string(&session_path)
        .expect("read local Flowy auth session.json");
    let session: Value =
        serde_json::from_str(&raw).expect("parse auth session.json");
    let api_key = session
        .get("access_token")
        .and_then(Value::as_str)
        .expect("access_token in session.json")
        .to_owned();

    let root = temp_dir("pr2b-live-cloud-e2e");
    let llm = LlmConfig {
        provider: "flowy".to_owned(),
        model: "AIPC-qwen3.8-flash".to_owned(),
        base_url: "https://server.flowyaipc.com/claw/v1".to_owned(),
        api_key: Some(api_key),
        ..LlmConfig::default()
    };
    let handle = AgentHandle::with_config(root.to_path_buf(), llm);
    handle.set_autonomous_drain(true);

    let t0 = Instant::now();
    let sub1: u128 = 20_001;
    let sub2: u128 = 20_002;
    let router_id = handle.create_router_session().expect("create live router");
    handle.select_session(&router_id).unwrap();
    println!(
        "[LIVE-E2E +{:>5}ms] created router_id={router_id} role={:?}",
        t0.elapsed().as_millis(),
        handle.session_role(&router_id)
    );

    // Message 1: ask Router to dispatch a real multi-step workspace deliverable (PPTX).
    let msg1 = "请帮我在后台派工做一份 3 页的《青禾季度复盘》PPT，文件名叫 qinghe_q3_live.pptx，包含封面、核心指标和下一步计划三页，并完成校验与交付。";
    let res1 = handle
        .submit_router_message(&router_id, sub1, msg1)
        .expect("submit msg1");
    assert_eq!(res1, router_id);
    println!(
        "[LIVE-E2E +{:>5}ms] submitted msg1 submission_id={sub1} router_id={router_id}",
        t0.elapsed().as_millis()
    );

    // Wait until Worker 1 is created AND actively running/queued (not yet finished).
    let mut worker_id = String::new();
    let mut worker_state_when_seen = String::new();
    let spawn_deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < spawn_deadline {
        let works = handle.child_works(&router_id);
        if let Some(first) = works.first() {
            worker_id = first.work_id.clone();
            worker_state_when_seen = first.state.clone();
            if matches!(
                first.state.as_str(),
                "running" | "queued" | "verifying" | "awaiting_approval"
            ) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    assert!(
        !worker_id.is_empty(),
        "Router must spawn at least one child Worker for PPTX brief"
    );
    assert!(
        matches!(
            worker_state_when_seen.as_str(),
            "running" | "queued" | "verifying" | "awaiting_approval"
        ),
        "expected Worker 1 to be active/unfinished when submitting msg2, got state={worker_state_when_seen}"
    );
    println!(
        "[LIVE-E2E +{:>5}ms] confirmed child worker_id={worker_id} root={} state={worker_state_when_seen} (UNFINISHED)",
        t0.elapsed().as_millis(),
        handle.root_session_id(&worker_id)
    );

    // Message 2 in the SAME conversation while Worker 1 is still unfinished!
    let msg2 = "趁后台任务还在跑，顺便直接回答我一个问题：17 乘以 23 等于多少？只回答数字和一句话即可，不要取消刚才的任务。";
    let res2 = handle
        .submit_router_message(&router_id, sub2, msg2)
        .expect("submit msg2 while worker 1 runs");
    assert_eq!(res2, router_id);
    println!(
        "[LIVE-E2E +{:>5}ms] submitted msg2 submission_id={sub2} while worker_id={worker_id} active",
        t0.elapsed().as_millis()
    );

    // Wait for Router to answer Message 2 (`391`).
    let msg2_deadline = Instant::now() + Duration::from_secs(60);
    let mut msg2_reply = None;
    let mut worker_state_at_msg2_reply = String::new();
    while Instant::now() < msg2_deadline {
        let rows = handle.session_lines(&router_id);
        if let Some(reply) = rows.iter().rev().find_map(|line| match line {
            ChatLine::Assistant { text } if text.contains("391") => {
                Some(text.clone())
            }
            _ => None,
        }) {
            msg2_reply = Some(reply);
            worker_state_at_msg2_reply = handle
                .child_works(&router_id)
                .into_iter()
                .find(|w| w.work_id == worker_id)
                .map_or_else(|| "missing".into(), |w| w.state);
            break;
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    let reply = msg2_reply.expect("Router must answer msg2 with 391");
    println!(
        "[LIVE-E2E +{:>5}ms] Router answered msg2 reply={:?} worker_state_at_reply={worker_state_at_msg2_reply}",
        t0.elapsed().as_millis(),
        reply
    );

    // Wait for Worker 1 to finish or reach terminal/approval state (bounded 120s).
    let settle_deadline = Instant::now() + Duration::from_secs(120);
    let mut final_worker = None;
    while Instant::now() < settle_deadline {
        if let Some(w) = handle
            .child_works(&router_id)
            .into_iter()
            .find(|w| w.work_id == worker_id)
        {
            let settled = !matches!(
                w.state.as_str(),
                "running" | "queued" | "verifying" | "stopping"
            );
            final_worker = Some(w);
            if settled {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let final_w = final_worker.expect("worker summary present");
    println!(
        "[LIVE-E2E +{:>5}ms] final worker_id={} state={} artifacts={:?} top_level_sessions={}",
        t0.elapsed().as_millis(),
        final_w.work_id,
        final_w.state,
        final_w.delivered_artifact_ids,
        handle.list_sessions().len()
    );
    assert_eq!(handle.list_sessions().len(), 1);
    assert_eq!(handle.root_session_id(&worker_id), router_id);
    assert_ne!(
        final_w.state, "stopped",
        "Message 2 must NOT cancel Worker 1"
    );
}

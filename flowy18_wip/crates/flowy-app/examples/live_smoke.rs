//! Manual live smoke: real cloud inference through the host agent stack.
//!
//! Usage:
//!   cargo run -p flowy-app --example `live_smoke` -- [prompt] [legacy]
//!
//! `legacy` seeds a history where an assistant tool call is paired with a
//! legacy tool result that has no `MessageSource::Tool`, to check the repair
//! path end-to-end against the real provider.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use flowy_agent::{AgentHandle, ChatLine, LlmConfig, TurnStatus};
use flowy_app::prefs::Preferences;
use flowy_app::session::SessionStore;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let legacy = mode == "legacy";
    let prompt = if legacy {
        "继续：请只回复两个字：OK".to_owned()
    } else {
        "请只回复两个字：OK".to_owned()
    };

    let data_root = flowy_app::app::default_store_root();
    println!("data_root = {}", data_root.display());
    let prefs = Preferences::load(&data_root);
    let store = SessionStore::new(data_root);
    let session = store.load().expect("no signed-in session in app data");
    println!("signed in as {}", session.email);

    let client = flowy_auth::AuthClient::from_env();
    let catalog = client
        .available_models(&session.access_token)
        .expect("model catalog request failed");
    println!("catalog cloud models = {}", catalog.cloud.len());
    let model = catalog
        .cloud
        .iter()
        .find(|model| model.id == prefs.model_id)
        .or_else(|| catalog.cloud.first())
        .expect("catalog is empty");
    println!(
        "model = {} endpoint = {} ctx = {} max = {}",
        model.id, model.endpoint, model.context_window, model.max_tokens
    );

    let agent_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../build-test/live-smoke-agent");
    if legacy {
        seed_legacy_history(&agent_root);
        println!("seeded legacy history at {}", agent_root.display());
    } else {
        let _ = std::fs::remove_dir_all(&agent_root);
    }

    let config = LlmConfig {
        api_key: Some(session.access_token),
        base_url: model.endpoint.clone(),
        model: model.id.clone(),
        provider: "flowy".into(),
        context_window: model.context_window,
        max_tokens: model.max_tokens,
    };
    let handle = AgentHandle::with_config(agent_root, config);
    handle.send(&prompt).expect("send failed");

    let started = Instant::now();
    let mut seen_running = false;
    loop {
        std::thread::sleep(Duration::from_millis(120));
        let status = handle.turn_status();
        if matches!(status, TurnStatus::Running) {
            seen_running = true;
        } else if seen_running {
            break;
        }
        if started.elapsed() > Duration::from_secs(180) {
            println!(
                "TIMEOUT after {:?}, status = {status:?}",
                started.elapsed()
            );
            std::process::exit(2);
        }
    }
    let elapsed = started.elapsed();
    let status = handle.turn_status();
    println!("elapsed = {elapsed:?}");
    println!("status = {status:?}");
    for (index, line) in handle.lines().iter().enumerate() {
        match line {
            ChatLine::User { text } => println!("[{index}] user: {text}"),
            ChatLine::Assistant { text } => {
                println!("[{index}] assistant: {text}");
            }
            ChatLine::Reasoning { text, .. } => {
                println!("[{index}] reasoning: {text}");
            }
            ChatLine::Tool { text } => println!("[{index}] tool: {text}"),
            ChatLine::ToolCall {
                name,
                target,
                running,
                ..
            } => println!(
                "[{index}] tool_call: {name} target={target} running={running}"
            ),
            ChatLine::ToolResult { text, is_error } => {
                println!("[{index}] tool_result: error={is_error} {text}");
            }
            ChatLine::Status { text } => println!("[{index}] status: {text}"),
            ChatLine::TaskReceipt { task_id, .. } => {
                println!("[{index}] task receipt: {task_id}");
            }
        }
    }
    match status {
        TurnStatus::Completed => println!("LIVE_SMOKE_OK"),
        other => {
            println!("LIVE_SMOKE_FAILED status={other:?}");
            std::process::exit(1);
        }
    }
}

fn seed_legacy_history(root: &std::path::Path) {
    let _ = std::fs::remove_dir_all(root);
    let sessions = root.join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions dir");
    std::fs::write(
        sessions.join("history.json"),
        serde_json::json!({
            "active": "legacy-smoke",
            "sessions": [{
                "id": "legacy-smoke",
                "title": "legacy smoke",
                "titled": true,
                "updated_at": 1,
                "status": {"kind": "completed"},
                "messages": [
                    {
                        "id": "u1",
                        "role": "user",
                        "content": [{"type": "text", "text": "读取 a.txt"}],
                        "source": {"kind": "user"}
                    },
                    {
                        "id": "a1",
                        "role": "assistant",
                        "content": [{
                            "type": "tool-call",
                            "id": "call-1",
                            "name": "read_file",
                            "arguments": "{\"path\":\"a.txt\"}"
                        }],
                        "source": {"kind": "model", "provider": "flowy", "model": ""}
                    },
                    {
                        "id": "t1",
                        "role": "user",
                        "content": [{
                            "type": "tool-result",
                            "tool_call_id": "call-1",
                            "content": [{"type": "text", "text": "file data"}],
                            "is_error": false
                        }],
                        "source": {"kind": "user"}
                    },
                    {
                        "id": "u2",
                        "role": "user",
                        "content": [{"type": "text", "text": "结果是什么？"}],
                        "source": {"kind": "user"}
                    }
                ]
            }]
        })
        .to_string(),
    )
    .expect("write history");
    std::fs::write(
        sessions.join("index.json"),
        serde_json::json!({
            "active": "legacy-smoke",
            "sessions": [{
                "id": "legacy-smoke",
                "title": "legacy smoke",
                "updated_at": 1,
                "lines": [
                    {"kind": "user", "text": "读取 a.txt"},
                    {"kind": "user", "text": "结果是什么？"}
                ],
                "status": {"kind": "completed"},
                "titled": true
            }]
        })
        .to_string(),
    )
    .expect("write index");
}

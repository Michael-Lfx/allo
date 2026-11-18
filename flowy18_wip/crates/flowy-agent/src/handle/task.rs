use std::path::{Path, PathBuf};

use super::{
    AgentHandle, AgentUpdate, Arc, CancelToken, ChatLine, ChatProjection,
    Engine, EngineConfig, Live, LoopOutcome, Mutex, TITLE_MAX_CHARS,
    TurnStatus, UiState, lock, model_route, now_ms, persist_index, title,
};
use crate::inventory::{
    Inventory, InventoryStatus, InventorySummary, inventory_tool, task_id,
};

impl AgentHandle {
    /// Create a local task, or return the original receipt for this submission.
    pub fn submit_inventory(
        &self,
        submission: u128,
        goal: &str,
        folder: Option<&Path>,
        origin: Option<String>,
    ) -> Result<String, &'static str> {
        let state = &self.inner.state;
        let _dispatch = lock(&state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let id = task_id(submission);
        if let Some(existing) =
            Inventory::load(&state.root, &id).map_err(storage)?
        {
            ensure_task_session(&engine, state, &existing).map_err(storage)?;
            persist_index(state)?;
            return Ok(id);
        }
        let mut task = Inventory::new(id.clone(), origin, goal.to_owned());
        if let Some(folder) = folder {
            task.set_folder(folder)
                .map_err(|_| "inventory.invalid_folder")?;
        }
        task.save(&state.root).map_err(storage)?;
        ensure_task_session(&engine, state, &task).map_err(storage)?;
        persist_index(state)?;
        if task.folder.is_some() {
            self.run_inventory(&engine, task)?;
        } else {
            state.push_update(AgentUpdate::Sessions);
        }
        Ok(id)
    }

    pub fn resume_inventory(
        &self,
        folder: Option<&Path>,
    ) -> Result<(), &'static str> {
        let state = &self.inner.state;
        let _dispatch = lock(&state.dispatch);
        let id = self.active_session_id().ok_or("notice.agent_failed")?;
        let mut task = Inventory::load(&state.root, &id)
            .map_err(storage)?
            .ok_or("notice.agent_failed")?;
        if !matches!(
            task.status,
            InventoryStatus::WaitingForFolder
                | InventoryStatus::Paused
                | InventoryStatus::Failed
        ) {
            return Err("inventory.use_task_controls");
        }
        if let Some(folder) = folder {
            task.set_folder(folder)
                .map_err(|_| "inventory.invalid_folder")?;
        }
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        self.run_inventory(&engine, task)
    }

    #[must_use]
    pub fn active_inventory(&self) -> Option<InventorySummary> {
        let id = self.active_session_id()?;
        let root = &self.inner.state.root;
        if let Ok(task) = Inventory::load(root, &id) {
            task.map(|mut task| {
                if task.status == InventoryStatus::Delivered
                    && !matches!(task.artifact(root), Ok(Some(_)))
                {
                    task.status = InventoryStatus::NeedsReview;
                }
                InventorySummary::from(task)
            })
        } else {
            Some(InventorySummary {
                goal: self.session_title(),
                folder: None,
                status: InventoryStatus::NeedsReview,
            })
        }
    }

    pub fn inventory_artifact(&self) -> Result<PathBuf, &'static str> {
        let id = self
            .active_session_id()
            .ok_or("inventory.output_unavailable")?;
        let root = &self.inner.state.root;
        let task = Inventory::load(root, &id)
            .map_err(storage)?
            .ok_or("inventory.output_unavailable")?;
        task.artifact(root)
            .ok()
            .flatten()
            .ok_or("inventory.output_unavailable")
    }

    fn run_inventory(
        &self,
        engine: &Arc<Engine>,
        mut task: Inventory,
    ) -> Result<(), &'static str> {
        let state = Arc::clone(&self.inner.state);
        let folder = task.folder.clone().ok_or("inventory.waiting")?;
        let config = lock(&self.inner.config).clone();
        if config.api_key.is_none() {
            task.status = InventoryStatus::Paused;
            task.save(&state.root).map_err(storage)?;
            state.push_update(AgentUpdate::Sessions);
            return Ok(());
        }
        task.attempt = task.attempt.saturating_add(1);
        task.status = InventoryStatus::Running;
        task.save(&state.root).map_err(storage)?;
        let listing = Arc::new(Mutex::new(None));
        let config = EngineConfig {
            route: model_route(&config),
            api_key: config.api_key,
            system: "This task delivers a directory inventory. Call inventory_selected_folder with no arguments to collect it. The host verifies and saves the actual file. Do not claim a particular file was delivered, and keep your reply brief.".into(),
            registry: inventory_tool(folder, Arc::clone(&listing)),
            cwd: None,
            todos: None,
        };
        let id = task.id;
        let goal = task.goal;
        let attempt = task.attempt;
        let turn = engine.start_with_config(
            id.clone(),
            goal.clone(),
            config,
            None,
            None,
        );
        let Ok(turn) = turn else {
            if let Some(mut task) =
                Inventory::load(&state.root, &id).map_err(storage)?
            {
                task.finish(&state.root, None).map_err(storage)?;
            }
            state.push_update(AgentUpdate::Session { id });
            return Err("notice.agent_failed");
        };
        if let Some(session) = lock(&state.live).get_mut(&id) {
            session.status = TurnStatus::Running;
            session.turn_started_at = Some(now_ms());
            session.updated_at = now_ms();
            session.projection.push_user(goal);
        }
        if persist_index(&state).is_err() {
            state.report_storage_error();
        }
        state.push_update(AgentUpdate::Sessions);
        self.inner.runtime.spawn(async move {
            let outcome = turn.await;
            let _dispatch = lock(&state.dispatch);
            let result = (|| {
                let Some(mut task) = Inventory::load(&state.root, &id)? else {
                    return Ok(());
                };
                if task.attempt != attempt
                    || task.status != InventoryStatus::Running
                {
                    return Ok(());
                }
                let output = if matches!(outcome, Ok(LoopOutcome::Completed)) {
                    lock(&listing).take()
                } else {
                    None
                };
                task.finish(&state.root, output)
            })();
            if result.is_err() {
                state.report_storage_error();
            }
            state.push_update(AgentUpdate::Session { id });
        });
        Ok(())
    }
}

pub(super) fn restore_inventories(engine: &Engine, state: &UiState) {
    let result = (|| {
        for id in Inventory::ids(&state.root)? {
            match Inventory::load(&state.root, &id) {
                Ok(Some(mut task)) => {
                    if task
                        .reconcile(&state.root)
                        .and_then(|()| {
                            ensure_task_session(engine, state, &task)
                        })
                        .is_err()
                    {
                        state.report_storage_error();
                    }
                }
                Ok(None) => (),
                Err(_) => state.report_storage_error(),
            }
        }
        Ok::<(), std::io::Error>(())
    })();
    if result.is_err() {
        state.report_storage_error();
    }
}

pub(super) fn cancel_inventory(state: &UiState, id: &str) {
    match Inventory::load(&state.root, id) {
        Ok(Some(mut task)) if task.status == InventoryStatus::Running => {
            task.status = InventoryStatus::Cancelled;
            if task.save(&state.root).is_err() {
                state.report_storage_error();
            }
        }
        Ok(_) => (),
        Err(_) => state.report_storage_error(),
    }
}

fn ensure_task_session(
    engine: &Engine,
    state: &UiState,
    task: &Inventory,
) -> std::io::Result<()> {
    let title = title::clamp_title(&task.goal, TITLE_MAX_CHARS);
    if engine.session(&task.id).is_none() {
        engine.insert_session(flowy_agent_core::SessionRecord {
            id: task.id.clone(),
            title: title.clone(),
            titled: true,
            updated_at: now_ms(),
            ..Default::default()
        })?;
    }
    let mut live = lock(&state.live);
    live.entry(task.id.clone()).or_insert_with(|| Live {
        projection: ChatProjection::new(),
        title,
        titled: true,
        auto_title_pending: false,
        title_signal: CancelToken::new(),
        status: TurnStatus::Idle,
        updated_at: now_ms(),
        turn_started_at: None,
    });
    if let Some(origin) = task.origin.as_ref().and_then(|id| live.get_mut(id))
        && !origin.projection.lines.iter().any(|line| matches!(line, ChatLine::TaskReceipt { task_id, .. } if *task_id == task.id))
    {
        origin.projection.lines.push(ChatLine::TaskReceipt {
            task_id: task.id.clone(),
            goal: task.goal.clone(),
        });
    }
    drop(live);
    Ok(())
}

fn storage(_: std::io::Error) -> &'static str {
    "notice.storage_error"
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use super::{AgentHandle, Inventory, InventoryStatus};
    use crate::LlmConfig;

    #[test]
    fn separate_tasks_resume_and_deliver_real_files_through_pi() {
        let root = std::env::temp_dir().join(format!(
            "flowy-inventory-loop-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        let first_folder = root.join("first");
        let second_folder = root.join("second");
        std::fs::create_dir_all(&first_folder).unwrap();
        std::fs::create_dir_all(&second_folder).unwrap();
        std::fs::write(first_folder.join("清单.txt"), b"keep original")
            .unwrap();
        std::fs::write(second_folder.join("second.txt"), b"keep too").unwrap();
        let store = root.join("state");
        let first;
        let ordinary;
        {
            let handle =
                AgentHandle::with_config(store.clone(), LlmConfig::default());
            ordinary = handle.submit_chat(100, "ordinary chat").unwrap();
            assert_eq!(
                handle.submit_chat(100, "ordinary chat").unwrap(),
                ordinary
            );
            assert_eq!(handle.list_sessions().len(), 1);
            assert!(handle.active_inventory().is_none());
            first = handle
                .submit_inventory(101, "first inventory", None, None)
                .unwrap();
            handle.select_session(&first).unwrap();
            assert_eq!(
                handle.active_inventory().unwrap().status,
                InventoryStatus::WaitingForFolder
            );
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let config = LlmConfig {
            base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
            provider: "openai".into(),
            model: "inventory-test".into(),
            api_key: Some("local-test".into()),
            ..Default::default()
        };
        listener.set_nonblocking(true).unwrap();
        let fixture = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut calls = 0;
            while calls < 4 && Instant::now() < deadline {
                match listener.accept() {
                    Ok((socket, _)) => {
                        serve_inventory_turn(socket);
                        calls += 1;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
            assert_eq!(calls, 4);
        });
        let handle = AgentHandle::with_config(store.clone(), config);
        assert_eq!(handle.active_session_id().as_deref(), Some(first.as_str()));
        assert_eq!(handle.submit_chat(100, "ordinary chat").unwrap(), ordinary);
        handle.resume_inventory(Some(&first_folder)).unwrap();
        let second = handle
            .submit_inventory(
                102,
                "second inventory",
                Some(&second_folder),
                None,
            )
            .unwrap();
        assert_eq!(
            handle
                .submit_inventory(
                    102,
                    "second inventory",
                    Some(&second_folder),
                    None
                )
                .unwrap(),
            second
        );
        fixture.join().unwrap();
        for (id, contains, excludes) in
            [(&first, "清单", "second"), (&second, "second", "清单")]
        {
            let deadline = Instant::now() + Duration::from_secs(5);
            let artifact = loop {
                let task = Inventory::load(&store, id).unwrap().unwrap();
                if let Some(artifact) = task.artifact(&store).unwrap() {
                    assert_eq!(task.attempt, 1);
                    break artifact;
                }
                assert!(
                    task.status == InventoryStatus::Running
                        && Instant::now() < deadline,
                    "task {id} ended in {:?}",
                    task.status
                );
                std::thread::sleep(Duration::from_millis(10));
            };
            let text = std::fs::read_to_string(artifact).unwrap();
            assert!(text.contains(contains) && !text.contains(excludes));
        }
        assert_eq!(
            std::fs::read(first_folder.join("清单.txt")).unwrap(),
            b"keep original"
        );
        assert_eq!(
            std::fs::read(second_folder.join("second.txt")).unwrap(),
            b"keep too"
        );
        drop(handle);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn serve_inventory_turn(mut socket: TcpStream) {
        socket
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut reader = BufReader::new(&mut socket);
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) =
                line.to_ascii_lowercase().strip_prefix("content-length:")
            {
                length = value.trim().parse().unwrap();
            }
        }
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        let tools = request["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "inventory_selected_folder");
        let tool_finished = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool");
        let (delta, finish) = if tool_finished {
            (
                json!({"role": "assistant", "content": "The inventory was collected."}),
                "stop",
            )
        } else {
            (
                json!({"role": "assistant", "tool_calls": [{
                    "index": 0, "id": "inventory_fixture", "type": "function",
                    "function": {"name": "inventory_selected_folder", "arguments": "{}"}
                }]}),
                "tool_calls",
            )
        };
        let event = json!({
            "id": "inventory-fixture", "object": "chat.completion.chunk",
            "created": 1, "model": "inventory-test",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]
        });
        let body = format!("data: {event}\n\ndata: [DONE]\n\n");
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    }
}

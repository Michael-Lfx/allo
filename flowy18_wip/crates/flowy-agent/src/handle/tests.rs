#![allow(clippy::wildcard_imports)]
use super::guards::*;
use super::observer::*;
use super::*;

#[cfg(test)]
mod background_init_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn accepting_with_no_live_session_creates_none_and_leaves_no_hint() {
        let root = std::env::temp_dir()
            .join(format!("flowy-accept-nosession-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        assert_eq!(handle.active_session_id(), None);

        let receipt =
            handle.accept_deferred("orphan prompt").expect("accepted");

        // The receipt must not claim a session it never had. Before this fix
        // `accept_deferred` called `ensure_active_session()`, which creates one,
        // so the receipt was pinned to an empty conversation the user never had.
        assert!(
            receipt.session_hint.is_none(),
            "no session was live, so there is nothing to hint at"
        );
        assert_eq!(
            handle.active_session_id(),
            None,
            "accepting must not switch the active session"
        );
        assert!(
            handle.list_sessions().is_empty(),
            "accepting must not create a session"
        );

        // Still durable, still readable.
        let pending = handle.pending_acceptances();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].text, "orphan prompt");

        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepting_while_a_session_is_live_keeps_it_only_as_a_hint() {
        let root = std::env::temp_dir()
            .join(format!("flowy-accept-hint-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let id = handle.create_session().expect("session");

        let receipt =
            handle.accept_deferred("hinted prompt").expect("accepted");
        assert_eq!(receipt.session_hint.as_deref(), Some(id.as_str()));
        // The hint did not change which session is active.
        assert_eq!(handle.active_session_id().as_deref(), Some(id.as_str()));

        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_receipt_for_a_busy_session_waits_instead_of_forking() {
        let root = std::env::temp_dir()
            .join(format!("flowy-accept-busy-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let hinted = handle.create_session().expect("hinted session");
        let receipt = handle
            .accept_deferred("wait for the hinted session")
            .expect("accepted");
        assert_eq!(receipt.session_hint.as_deref(), Some(hinted.as_str()));

        // The user moved on to another session; the hinted one is still busy.
        handle.create_session().expect("other session");
        lock(&handle.inner.state.live)
            .get_mut(&hinted)
            .expect("hinted session is live")
            .status = TurnStatus::Running;

        assert!(
            handle.run_accepted_once().expect("pump").is_none(),
            "the busy session's receipt must wait"
        );
        assert_eq!(
            handle.list_sessions().len(),
            2,
            "waiting must not fork a new conversation"
        );
        assert!(!handle.has_active_claim(), "nothing was claimed");
        let pending = handle.pending_acceptances();
        assert_eq!(pending.len(), 1);
        assert!(
            pending[0].taken_at_ms.is_none(),
            "waiting must not inflate the attempt count"
        );

        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_accepted_once_submits_the_receipt_prompt() {
        let root = std::env::temp_dir()
            .join(format!("flowy-exec-prompt-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        handle.create_session().expect("session");
        handle.accept_deferred("from the input").expect("accepted");

        let run = handle.run_accepted_once().expect("pump").expect("claimed");
        assert_ne!(run.receipt_id, "");
        assert!(
            handle.chat_history().rows.iter().any(|row| matches!(
                row,
                crate::projection::ChatLine::User { text }
                    if text == "from the input"
            )),
            "the submitted prompt must match the accepted receipt"
        );

        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_tool_registry_advertises_the_deck_capability() {
        let registry = tool_registry(std::path::Path::new("."));
        assert!(
            registry.list().iter().any(|name| name == "create_deck"),
            "the deck tool is part of the capability registry"
        );
    }

    #[test]
    fn accept_deferred_hands_back_a_receipt_a_fresh_handle_still_finds() {
        let root =
            std::env::temp_dir().join(format!("flowy-accept-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);

        let receipt = handle
            .accept_deferred("second prompt while the first turn runs")
            .expect("accepted while busy");
        assert_eq!(receipt.text, "second prompt while the first turn runs");
        assert!(receipt.taken_at_ms.is_none());

        // A separate handle over the same root is the "restart the client"
        // shape: no in-memory state is shared, only the receipt file.
        let reopened = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        let pending = reopened.pending_acceptances();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, receipt.id);
        assert_eq!(pending[0].text, receipt.text);

        drop(handle);
        drop(reopened);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn empty_selection_revision_prevents_sending_home_draft_into_a_new_project()
    {
        let root = std::env::temp_dir()
            .join(format!("flowy-context-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let captured = handle.context_generation();
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        handle.set_workspace_for_new_sessions(Some(AgentWorkspace {
            project_id: "project-1".into(),
            root: project,
        }));
        handle.set_origin_for_new_sessions(SessionOrigin::Project {
            project_id: Some("project-1".into()),
        });
        assert_eq!(handle.active_session_id(), None);
        assert_eq!(
            handle.submit_chat_if_active(1, "Home draft", None, captured),
            Err("notice.agent_busy")
        );
        assert!(handle.list_sessions().is_empty());
        assert_eq!(handle.lines().as_slice(), []);
        drop(handle);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retry_delivery_commits_the_durable_group_and_clears_failure() {
        use crate::artifact_store::PrepareArtifact;

        let root = std::env::temp_dir()
            .join(format!("flowy-retry-delivery-{}", next_stamp()));
        let config = LlmConfig::default();
        let handle = AgentHandle::uninitialized(root.clone(), config.clone());
        let state = Arc::clone(&handle.inner.state);
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::clone(&state),
        });
        let engine = Arc::new(
            Engine::open(root.clone(), engine_config(&config, &root), observer)
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: root.join("workspace"),
                }),
            )
            .unwrap();
        let output = root.join("outputs").join("submission-1");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("report.md"), b"# report\n").unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "report.md".into(),
                source_relative_path: "report.md".into(),
                conversation_import: false,
                session_id: "home".into(),
                submission_id: "submission-1".into(),
                tool_call_id: "call-1".into(),
                delivery_key: "report".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        let mut record = engine.session("home").unwrap();
        record.delivery_commits.push(DeliveryCommit {
            session_id: "home".into(),
            submission_id: "submission-1".into(),
            artifact_ids: vec![receipt.artifact_id.clone()],
            completed_successfully: true,
            committed_at_ms: 1,
            origin: SessionOrigin::Home,
            origin_title_snapshot: "Home".into(),
        });
        engine.insert_session(record).unwrap();
        *lock(&handle.inner.engine) = Some(engine);
        let _ = state.artifact_store.set(store.clone());
        *lock(&state.active) = Some("home".into());
        lock(&state.live).insert(
            "home".into(),
            Live {
                projection: ChatProjection::from_lines(vec![
                    ChatLine::Status {
                        text: "chat.status_delivery_failed".into(),
                    },
                ]),
                title: "Home".into(),
                titled: true,
                auto_title_pending: false,
                title_signal: CancelToken::new(),
                status: TurnStatus::DeliveryFailed,
                updated_at: 1,
                turn_started_at: None,
                workspace: None,
                origin: SessionOrigin::Home,
                session_workspace: None,
            },
        );

        if let Some(live) = lock(&state.live).get_mut("home") {
            live.status = TurnStatus::Running;
        }
        assert_eq!(
            handle.retry_delivery(Some("home")),
            Err("notice.agent_busy")
        );
        if let Some(live) = lock(&state.live).get_mut("home") {
            live.status = TurnStatus::DeliveryFailed;
        }
        handle.retry_delivery(Some("home")).unwrap();
        // Read under the guard, then release it: the lock is a significant
        // drop, and holding it across the assertions serves nothing.
        let live_state = lock(&state.live);
        let live = live_state.get("home").unwrap();
        let is_completed = live.status == TurnStatus::Completed;
        let has_failure_line = live.projection.lines.iter().any(|line| {
            matches!(line, ChatLine::Status { text } if text == "chat.status_delivery_failed")
        });
        let has_artifact_line = live.projection.lines.iter().any(|line| {
            matches!(line, ChatLine::Artifact { artifact_id } if artifact_id == &receipt.artifact_id)
        });
        drop(live_state);
        assert!(is_completed);
        assert!(!has_failure_line);
        assert!(has_artifact_line);
        assert_eq!(
            store.read_payload(&receipt.artifact_id).unwrap(),
            b"# report\n"
        );
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_completes_a_prepared_session_deletion_after_crash() {
        use crate::artifact_store::PrepareArtifact;

        let root = std::env::temp_dir()
            .join(format!("flowy-session-delete-recovery-{}", next_stamp()));
        let config = LlmConfig::default();
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::new(UiState::new(root.clone(), None)),
        });
        let engine =
            Engine::open(root.clone(), engine_config(&config, &root), observer)
                .unwrap();
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: root.join("workspace"),
                }),
            )
            .unwrap();
        let output = root.join("outputs").join("submission-1");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            output.join("report.md"),
            b"# report
",
        )
        .unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "report.md".into(),
                source_relative_path: "report.md".into(),
                conversation_import: false,
                session_id: "home".into(),
                submission_id: "submission-1".into(),
                tool_call_id: "call-1".into(),
                delivery_key: "report".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        let delivery = DeliveryCommit {
            session_id: "home".into(),
            submission_id: "submission-1".into(),
            artifact_ids: vec![receipt.artifact_id.clone()],
            completed_successfully: true,
            committed_at_ms: 1,
            origin: SessionOrigin::Home,
            origin_title_snapshot: "Home".into(),
        };
        store
            .recover_commits(std::slice::from_ref(&delivery))
            .unwrap();
        store
            .prepare_session_deletion(
                "home",
                &delivery.artifact_ids,
                std::slice::from_ref(&delivery.submission_id),
            )
            .unwrap();
        engine.remove_session("home").unwrap();
        crate::index::SessionIndex {
            active: Some("home".into()),
            sessions: vec![IndexRecord {
                id: "home".into(),
                title: "stale".into(),
                updated_at: 1,
                lines: vec![ChatLine::User {
                    text: "stale row".into(),
                }],
                status: TurnStatus::Completed,
                titled: true,
                workspace: None,
                origin: SessionOrigin::Home,
                session_workspace: None,
            }],
        }
        .save(&root)
        .unwrap();
        drop(store);
        drop(engine);

        let handle = AgentHandle::with_config(root.clone(), config);
        let deadline = Instant::now() + Duration::from_secs(5);
        while handle.is_initializing() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!handle.is_initializing());
        assert!(handle.list_sessions().is_empty());
        assert!(
            crate::index::SessionIndex::load(&root)
                .unwrap()
                .sessions
                .is_empty()
        );
        let store = handle.artifact_store().unwrap();
        assert_eq!(store.list().unwrap().as_slice(), []);
        assert!(store.get(&receipt.artifact_id).unwrap().is_none());
        assert!(store.session_deletions().unwrap().is_empty());
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_source_deletion_marker_is_migrated_to_cascade_cleanup() {
        use crate::artifact_store::PrepareArtifact;

        let root = std::env::temp_dir()
            .join(format!("flowy-session-delete-legacy-{}", next_stamp()));
        let config = LlmConfig::default();
        let state = Arc::new(UiState::new(root.clone(), None));
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::clone(&state),
        });
        let engine =
            Engine::open(root.clone(), engine_config(&config, &root), observer)
                .unwrap();
        let output = root.join("outputs").join("submission-legacy");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            output.join("report.md"),
            b"# report
",
        )
        .unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "report.md".into(),
                source_relative_path: "report.md".into(),
                conversation_import: false,
                session_id: "legacy".into(),
                submission_id: "submission-legacy".into(),
                tool_call_id: "call-legacy".into(),
                delivery_key: "report".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        store
            .recover_commits(std::slice::from_ref(&DeliveryCommit {
                session_id: "legacy".into(),
                submission_id: "submission-legacy".into(),
                artifact_ids: vec![receipt.artifact_id.clone()],
                completed_successfully: true,
                committed_at_ms: 1,
                origin: SessionOrigin::Home,
                origin_title_snapshot: "Legacy".into(),
            }))
            .unwrap();
        std::fs::write(
            store.root().join("sources-deleted/legacy.json"),
            br#"{"schema_version":1,"session_id":"legacy","deleted_at_ms":1}"#,
        )
        .unwrap();

        let deleted = migrate_legacy_source_deletions(&store, &engine, &state);
        assert!(deleted.contains("legacy"));
        assert_eq!(store.list().unwrap().as_slice(), []);
        assert!(store.get(&receipt.artifact_id).unwrap().is_none());
        assert_eq!(store.session_deletions().unwrap().len(), 1);
        store.complete_session_deletion("legacy").unwrap();
        assert!(!store.root().join("sources-deleted/legacy.json").exists());
        assert!(store.session_deletions().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn background_initialization_restores_sessions_and_announces_readiness() {
        let root = std::env::temp_dir()
            .join(format!("flowy-background-{}", next_stamp()));
        let config = LlmConfig {
            api_key: None,
            ..LlmConfig::default()
        };
        let seed = AgentHandle::with_config(root.clone(), config.clone());
        let id = seed.create_home_session().unwrap();
        drop(seed);
        let handle = AgentHandle::with_config_background(root.clone(), config);
        handle
            .apply_config(LlmConfig {
                api_key: None,
                model: "latest-model".into(),
                ..LlmConfig::default()
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while handle.is_initializing() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            !handle.is_initializing(),
            "storage initialization did not finish"
        );
        assert_eq!(handle.active_session_id().as_deref(), Some(id.as_str()));
        assert!(handle.source_available(&id));
        assert_eq!(lock(&handle.inner.config).model, "latest-model");
        let updates = handle.drain_updates();
        assert_eq!(
            updates
                .iter()
                .filter(|update| matches!(update, AgentUpdate::Initialized))
                .count(),
            1
        );
        assert!(
            !updates
                .iter()
                .any(|update| matches!(update, AgentUpdate::StorageError))
        );
        drop(handle);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn disconnected_cloud_config_does_not_drain_receipts_into_echo() {
        let root = std::env::temp_dir()
            .join(format!("flowy-cloud-gate-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: "flowy".into(),
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let receipt = handle
            .accept_deferred("do not echo me before cloud catalog arrives")
            .expect("accepted");
        assert!(handle.requires_cloud_ready());
        assert!(!handle.can_run_turns());
        assert!(
            handle.run_accepted_once().expect("pump").is_none(),
            "disconnected cloud handle must keep receipts queued"
        );
        assert_eq!(handle.pending_acceptances().len(), 1);
        assert_eq!(handle.pending_acceptances()[0].id, receipt.id);
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn create_verify_and_publish_deck_succeed_in_one_home_turn() {
        let root = std::env::temp_dir()
            .join(format!("flowy-e2e-deck-{}", next_stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let delivery =
            crate::delivery::DeliveryStore::new(root.join("delivery"));
        let create_tool = crate::pptx::definition();
        let verify_tool = crate::verify::definition(Some(delivery.clone()));
        let publish_tool =
            crate::publish_tool::definition(store.clone(), Some(delivery));

        let workspace = root.join("sessions/workspaces/s-home");
        let submission_id = format!("submission-{:032x}", 7);
        let output_root = workspace.join("outputs").join(&submission_id);
        std::fs::create_dir_all(&output_root).unwrap();
        let context = flowy_agent_core::ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("s-home".into()),
            cwd: Some(workspace.display().to_string()),
            todos: None,
            submission_id: Some(submission_id),
            session_origin: SessionOrigin::Home,
            output_root: Some(output_root.clone()),
            artifact_delivery_available: true,
        };

        let created = (create_tool.execute)(
            ToolCallArgs {
                call_id: "c1".into(),
                name: "create_deck".into(),
                arguments: serde_json::json!({
                    "path": "deck.pptx",
                    "title": "Q4 Review",
                    "slides": ["Summary", "Next steps"],
                }),
            },
            context.clone(),
        )
        .await;
        assert!(
            matches!(
                created,
                flowy_agent_core::ToolExecutionResult::Success { .. }
            ),
            "create_deck failed: {created:?}"
        );
        assert!(output_root.join("deck.pptx").exists());

        let verified = (verify_tool.execute)(
            ToolCallArgs {
                call_id: "c2".into(),
                name: "verify_deck".into(),
                arguments: serde_json::json!({
                    "path": "deck.pptx",
                    "slides": 2,
                    "anchors": [
                        {"anchor": "S0-title", "value": "Q4 Review"},
                        {"anchor": "S0-body", "value": "Summary"},
                        {"anchor": "S1-title", "value": "Q4 Review · 2"},
                        {"anchor": "S1-body", "value": "Next steps"}
                    ],
                }),
            },
            context.clone(),
        )
        .await;
        let flowy_agent_core::ToolExecutionResult::Success { value, .. } =
            verified
        else {
            panic!("verify_deck failed: {verified:?}");
        };
        assert_eq!(value["ok"], true);

        let published = (publish_tool.execute)(
            ToolCallArgs {
                call_id: "c3".into(),
                name: "publish_file".into(),
                arguments: serde_json::json!({
                    "path": "deck.pptx",
                    "delivery_key": "deck",
                }),
            },
            context,
        )
        .await;
        assert!(
            matches!(
                published,
                flowy_agent_core::ToolExecutionResult::Success {
                    delivery: Some(_),
                    ..
                }
            ),
            "publish_file must succeed after create_deck + verify_deck, got {published:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod guard_tests {
    use super::{
        AcceptanceQueue, AgentHandle, AgentWorkspace, CancelToken,
        ContentBlock, LlmConfig, Message, MessageSource, ReceiptState, Role,
        SessionOrigin, SessionRecord, TurnState, confined_to, lock,
        model_route, next_stamp, path_guard, promote_latest_turn_verifications,
        session_working_directory, take_file_change, tool_arg_key,
        tool_registry, tool_result_display_text, upsert_verification_line,
    };
    use crate::projection::{ChatLine, FileChangeKind, VerificationIssue};
    use flowy_agent_core::types::ToolCallArgs;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    fn args(name: &str, arguments: serde_json::Value) -> ToolCallArgs {
        ToolCallArgs {
            call_id: "call-1".into(),
            name: name.into(),
            arguments,
        }
    }

    #[test]
    fn file_tools_stay_inside_the_tool_working_directory() {
        let root = Path::new("/work");
        assert!(confined_to(root, "a/b"));
        assert!(confined_to(root, "./a/../b"));
        assert!(!confined_to(root, "../outside"));
        assert!(!confined_to(root, "/etc/passwd"));
    }

    fn shell_name() -> &'static str {
        if cfg!(windows) { "powershell" } else { "bash" }
    }

    #[test]
    fn shell_is_not_treated_as_a_file_path() {
        let root = Path::new("/work");
        for name in ["bash", "powershell"] {
            let shell = args(name, json!({ "command": "cd .. && pwd" }));
            assert_eq!(path_guard(root, &shell), None);
        }
        let read = args("read_file", json!({ "path": "../outside" }));
        assert!(path_guard(root, &read).is_some());
    }

    #[test]
    fn production_registry_exposes_file_and_shell_tools_without_todos() {
        let registry = tool_registry(Path::new("/work"));
        let names = registry.list();
        assert!(names.iter().any(|name| name == "read_file"));
        assert!(names.iter().any(|name| name == shell_name()));
        let other = if cfg!(windows) { "bash" } else { "powershell" };
        assert!(!names.iter().any(|name| name == other));
        assert!(!names.iter().any(|name| name == "todo_write"));
    }

    #[test]
    fn session_workspace_overrides_default_tool_directory() {
        let root = Path::new("/flowy-data");
        let workspace = AgentWorkspace {
            project_id: "project-a".into(),
            root: Path::new("/work/project-a").to_path_buf(),
        };
        assert_eq!(
            session_working_directory(root, Some(&workspace)),
            workspace.root,
        );
        assert_eq!(
            session_working_directory(root, None),
            root.join("sessions").join("cwd"),
        );
    }

    #[test]
    fn publish_tool_result_uses_ui_key_without_changing_other_tools() {
        let payload = r#"{"status":"staged","message":"File staged.","ui_key":"chat.publish.staged"}"#;
        assert_eq!(
            tool_result_display_text("publish_file", payload),
            "chat.publish.staged"
        );
        assert_eq!(tool_result_display_text("write_file", payload), payload);
    }

    #[test]
    fn file_change_counts_changed_lines_for_edits() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c1"),
            json!({
                "path": "src/a.rs",
                "old_string": "a\nb\nc",
                "new_string": "a\nB\nc",
            }),
        );
        let edit = take_file_change(&args, "s", "c1", Path::new("/work"))
            .expect("edit change");
        assert!(edit.path.ends_with("a.rs"), "path={}", edit.path);
        assert_eq!(edit.kind, FileChangeKind::Update);
        assert_eq!((edit.added, edit.removed), (1, 1));
        assert!(!edit.binary);
        // The cached arguments are dropped once the row is built.
        assert!(
            take_file_change(&args, "s", "c1", Path::new("/work")).is_none()
        );
    }

    #[test]
    fn file_change_reports_only_additions_for_whole_file_writes() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c2"),
            json!({ "path": "new.md", "content": "one\ntwo\n" }),
        );
        let write = take_file_change(&args, "s", "c2", Path::new("/work"))
            .expect("write change");
        assert_eq!(write.kind, FileChangeKind::Add);
        assert_eq!((write.added, write.removed), (2, 0));
    }

    #[test]
    fn relative_tool_paths_resolve_against_the_session_directory() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c3"),
            json!({ "path": "notes/a.md", "content": "x\n" }),
        );
        let change = take_file_change(&args, "s", "c3", Path::new("/work"))
            .expect("change");
        assert!(change.path.contains("notes"), "path={}", change.path);
        assert!(change.path.ends_with("a.md"));
    }

    #[test]
    fn blank_tool_paths_produce_no_row() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c4"),
            json!({ "path": "   ", "content": "x" }),
        );
        assert!(
            take_file_change(&args, "s", "c4", Path::new("/work")).is_none()
        );
    }

    #[test]
    fn deck_tools_cannot_escape_the_working_directory() {
        let root = Path::new("/work");
        for tool in ["create_deck", "verify_deck", "verify_artifact"] {
            let escaped = args(tool, json!({ "path": "../outside.pptx" }));
            assert!(
                path_guard(root, &escaped).is_some(),
                "{tool} must reject parent traversal"
            );
            let inside = args(tool, json!({ "path": "outputs/sub/deck.pptx" }));
            assert_eq!(path_guard(root, &inside), None);
        }
    }

    #[test]
    fn verification_cards_supersede_older_failures_and_pin_above_artifacts() {
        let mut lines = vec![
            ChatLine::User {
                text: "Build Q3 deck".into(),
            },
            ChatLine::Verification {
                report_id: "report-fail".into(),
                artifact_path: "outputs/sub/q3-east.pptx".into(),
                artifact_hash: "aaaa1111".into(),
                ok: false,
                issues: vec![VerificationIssue {
                    code: "ANCHOR_MISSING".into(),
                    field: "S0-title".into(),
                }],
                semantic_hint: None,
                override_used: false,
            },
        ];
        upsert_verification_line(
            &mut lines,
            ChatLine::Verification {
                report_id: "report-pass".into(),
                artifact_path: "outputs\\sub\\q3-east.pptx".into(),
                artifact_hash: "bbbb2222".into(),
                ok: true,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
            },
        );
        lines.push(ChatLine::Assistant {
            text: "Summary table".into(),
        });
        lines.push(ChatLine::Artifact {
            artifact_id: "art-1".into(),
        });

        promote_latest_turn_verifications(&mut lines);

        let verification_count = lines
            .iter()
            .filter(|line| matches!(line, ChatLine::Verification { .. }))
            .count();
        assert_eq!(
            verification_count, 1,
            "older failed verification must be superseded by the latest pass"
        );
        assert!(matches!(lines[1], ChatLine::Assistant { .. }));
        assert!(matches!(
            &lines[2],
            ChatLine::Verification {
                report_id,
                ok: true,
                ..
            } if report_id == "report-pass"
        ));
        assert!(matches!(
            &lines[3],
            ChatLine::Artifact { artifact_id } if artifact_id == "art-1"
        ));
    }

    #[test]
    fn crash_after_engine_commit_reconciles_receipt_without_rerunning_while_user_only_stays_retryable()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-crash-reconcile-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let receipt = handle
            .accept_deferred("complete once then crash before journal")
            .expect("accept");
        let run = handle.run_accepted_once().expect("claim").expect("claimed");
        for _ in 0..200 {
            if !handle.has_busy_session() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!handle.has_busy_session());
        // Simulate process crash AFTER engine committed history, BEFORE
        // `finish_accepted(true)` wrote `Completed` to acceptance/journal.jsonl.
        drop(handle);

        // Also simulate a second receipt whose engine session crashed right
        // after `Engine::prepare` wrote only the User message (status=Running).
        let queue = AcceptanceQueue::new(root.clone());
        let user_only = queue
            .accept(None, "crashed after prepare", 1_000)
            .expect("accept second");
        let _ = queue
            .take(&user_only.id, "attempt-prep-only", 1_100, 1)
            .expect("take second");
        let _ = queue
            .bind(
                &user_only.id,
                "attempt-prep-only",
                "s-prep-only",
                "submission-prep-only",
                1_110,
            )
            .expect("bind second");
        let mut index =
            flowy_agent_core::SessionIndex::load(&root).expect("load history");
        index.upsert(SessionRecord {
            id: "s-prep-only".into(),
            status: TurnState::Running,
            messages: vec![Message {
                id: "submission-prep-only".into(),
                role: Role::User,
                content: vec![ContentBlock::text("crashed after prepare")],
                source: MessageSource::User,
            }],
            origin: SessionOrigin::Home,
            ..SessionRecord::default()
        });
        index.save(&root).expect("save history");

        // Reopen a fresh handle: `boot()` must reconcile receipt 1 to Completed
        // without re-running it, and must NOT mark user-only receipt 2 done.
        let reopened = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        assert_eq!(
            reopened.receipt_state(&receipt.id),
            Some(ReceiptState::Completed),
            "committed engine turn must reconcile to Completed on restart"
        );
        assert_eq!(
            reopened.receipt_state(&user_only.id),
            Some(ReceiptState::Retrying),
            "user-only pre-run prompt must never be mistaken for a committed turn"
        );
        let pending_ids: Vec<String> = reopened
            .pending_acceptances()
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(pending_ids, vec![user_only.id]);

        // Verify original session history was not duplicated by a rerun.
        let restored_history =
            flowy_agent_core::SessionIndex::load(&root).expect("reload");
        let committed_session = restored_history
            .get(&run.session_id)
            .expect("original session");
        let user_msgs = committed_session
            .messages
            .iter()
            .filter(|msg| msg.role == Role::User)
            .count();
        assert_eq!(user_msgs, 1, "committed receipt must execute exactly once");

        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn override_deliver_artifact_rejects_swapped_file_after_failed_report() {
        let root = std::env::temp_dir()
            .join(format!("flowy-override-tamper-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let session_id = handle.create_home_session().expect("home session");
        let workspace_root = lock(&handle.inner.engine)
            .as_ref()
            .and_then(|engine| engine.session(&session_id))
            .and_then(|record| record.workspace.map(|ws| ws.root))
            .expect("home workspace");
        let deck = workspace_root
            .join("outputs")
            .join("sub-1")
            .join("deck.pptx");
        std::fs::create_dir_all(deck.parent().unwrap()).unwrap();
        std::fs::write(&deck, b"PK\x03\x04original-failed-deck").unwrap();

        let gate = crate::delivery::DeliveryStore::new(root.join("delivery"));
        let report = gate
            .record_report(
                &deck,
                vec![crate::delivery::Issue {
                    code: "STRUCTURE".to_owned(),
                    field: "brief_min_slides".to_owned(),
                }],
                1_000,
            )
            .expect("record failed report");

        // Swap the bytes on disk after the failed verification card opened.
        std::fs::write(&deck, b"PK\x03\x04tampered-after-card-opened").unwrap();

        let err = handle
            .override_deliver_artifact(
                &session_id,
                &report.id,
                &deck.display().to_string(),
            )
            .expect_err(
                "override must reject bytes changed after the failed report",
            );
        assert_eq!(err, "chat.publish.not_verified");
        let artifacts = handle
            .artifact_store()
            .expect("store")
            .list()
            .expect("list");
        assert_eq!(artifacts.len(), 0);
        let events =
            std::fs::read_to_string(root.join("delivery").join("events.jsonl"))
                .unwrap_or_default();
        assert!(!events.contains("deliver_override"));
        assert!(!events.contains("\"deliver\""));

        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_semantic_verify_makes_zero_cloud_requests() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
        let addr = listener.local_addr().expect("addr");
        listener.set_nonblocking(true).expect("nonblocking");

        let root = std::env::temp_dir()
            .join(format!("flowy-l3-disclosure-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: Some("test-key".to_owned()),
                base_url: format!("http://{addr}/v1"),
                model: "test-model".to_owned(),
                provider: "openai".to_owned(),
                ..LlmConfig::default()
            },
        );
        handle.set_semantic_verify_enabled(false);
        assert!(!handle.semantic_verify_enabled());

        let deck = root.join("deck.pptx");
        crate::pptx::write_deck(&deck, "T", &["one".to_owned()]).unwrap();
        let store = crate::delivery::DeliveryStore::new(root.join("delivery"));
        let semantic = handle.semantic_verify_enabled().then(|| {
            crate::semantic_verify::SemanticContext {
                brief: "T".to_owned(),
                route: model_route(&lock(&handle.inner.config)),
                api_key: "test-key".to_owned(),
            }
        });
        let tool = crate::verify::definition_with_semantic_and_contract(
            Some(store),
            semantic,
            None,
        );
        let context = flowy_agent_core::ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("s-l3".to_owned()),
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: Some("sub-l3".to_owned()),
            session_origin: SessionOrigin::Home,
            output_root: None,
            artifact_delivery_available: true,
        };
        let args = ToolCallArgs {
            call_id: "call-l3-off".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: serde_json::json!({
                "path": "deck.pptx",
                "slides": 1,
                "anchors": [
                    {"anchor": "S0-title", "value": "T"},
                    {"anchor": "S0-body", "value": "one"},
                ],
            }),
        };
        let rt = tokio::runtime::Runtime::new().expect("rt");
        let result = rt.block_on((tool.execute)(args, context));
        assert!(matches!(
            result,
            flowy_agent_core::ToolExecutionResult::Success { .. }
        ));
        assert!(
            matches!(
                listener.accept(),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock
            ),
            "disabling host semantic verification must make zero outbound requests"
        );

        drop(rt);
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn confined_to_blocks_nonexistent_child_under_escaping_link() {
        let root = std::env::temp_dir()
            .join(format!("flowy-confined-escape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let link = workspace.join("escape_link");
        #[cfg(windows)]
        let linked = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.display().to_string(),
                &outside.display().to_string(),
            ])
            .status()
            .is_ok_and(|status| status.success());
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &link).is_ok();

        assert!(linked, "test fixture must create directory link");
        // Multi-level non-existent path inside the link must be rejected!
        assert!(
            !confined_to(&workspace, "escape_link/new_sub/evil.txt"),
            "escaping link with non-existent descendant must not pass confined_to"
        );
        // Normal non-existent multi-level path inside workspace must still pass.
        assert!(confined_to(&workspace, "outputs/new_sub/good.txt"));

        #[cfg(windows)]
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&root);
    }
}

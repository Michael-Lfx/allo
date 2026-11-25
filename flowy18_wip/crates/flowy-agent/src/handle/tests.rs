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
        let registry =
            tool_registry(std::path::Path::new("."), &[], Vec::new());
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
                heartbeat: None,
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
        SessionOrigin, SessionRecord, TurnState, append_skill_listing,
        confined_to, lock, model_route, next_stamp, path_guard,
        prepare_skill_prompt, promote_latest_turn_verifications,
        session_working_directory, shell_guard, skill_scope, take_file_change,
        tool_arg_key, tool_registry, tool_result_display_text,
        upsert_verification_line,
    };
    use crate::projection::{ChatLine, FileChangeKind, VerificationIssue};
    use flowy_agent_core::types::ToolCallArgs;
    use flowy_skills::{InstallOrigin, InstalledSkill, parse_skill_id};
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use std::sync::{Arc, Mutex};

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
        let active = Arc::new(Mutex::new(HashSet::new()));
        for name in ["bash", "powershell"] {
            let shell = args(name, json!({ "command": "cd .. && pwd" }));
            assert_eq!(path_guard(root, &[], &active, &shell), None);
        }
        let read = args("read_file", json!({ "path": "../outside" }));
        assert!(path_guard(root, &[], &active, &read).is_some());
    }

    #[test]
    fn production_registry_exposes_file_and_shell_tools_without_todos() {
        let registry = tool_registry(Path::new("/work"), &[], Vec::new());
        let names = registry.list();
        assert!(names.iter().any(|name| name == "read_file"));
        assert!(names.iter().any(|name| name == shell_name()));
        let other = if cfg!(windows) { "bash" } else { "powershell" };
        assert!(!names.iter().any(|name| name == other));
        assert!(!names.iter().any(|name| name == "todo_write"));
    }

    fn installed_skill(scripts_enabled: bool) -> InstalledSkill {
        InstalledSkill {
            id: parse_skill_id("owner", "demo", "1.0.0").unwrap(),
            name: "demo".into(),
            description: "Demo <skill> & helpers".into(),
            root: Path::new("/work/skills/demo").to_path_buf(),
            skill_path: Path::new("/work/skills/demo/SKILL.md").to_path_buf(),
            content: "---\nname: demo\n---\nUse the demo.".into(),
            auto_invoke: true,
            scripts_enabled,
            origin: InstallOrigin::Community,
            latest_version: Some("1.0.0".into()),
        }
    }

    #[test]
    fn slash_skill_injects_full_body_and_keeps_display_text() {
        let skill = installed_skill(false);
        let (prompt, active) =
            prepare_skill_prompt("/skill:demo summarize this", &[skill])
                .unwrap();
        assert!(prompt.model.contains("<skill name=\"demo\""));
        assert!(prompt.model.contains("Use the demo."));
        assert_eq!(
            prompt.display.as_deref(),
            Some("/skill:demo summarize this")
        );
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn available_skill_listing_is_xml_escaped() {
        let listing = append_skill_listing("base", &[installed_skill(false)]);
        assert!(listing.starts_with("base\n\n"));
        assert!(listing.contains("<available_skills>"));
        assert!(listing.contains("Demo &lt;skill&gt; &amp; helpers"));
        assert!(listing.contains("/work/skills/demo/SKILL.md"));
    }

    #[test]
    fn shell_is_blocked_until_active_skill_scripts_are_enabled() {
        let skill = installed_skill(false);
        let active = Arc::new(Mutex::new(HashSet::from([skill.id.clone()])));
        let shell = args(shell_name(), json!({ "command": "echo hi" }));
        assert!(
            shell_guard(std::slice::from_ref(&skill), &active, &shell)
                .is_some()
        );
        let mut enabled = skill;
        enabled.scripts_enabled = true;
        assert!(shell_guard(&[enabled], &active, &shell).is_none());
    }

    #[test]
    fn auto_listed_skills_are_in_shell_scope() {
        let skill = installed_skill(false);
        let active = Arc::new(Mutex::new(skill_scope(
            std::slice::from_ref(&skill),
            Vec::new(),
        )));
        let shell = args(shell_name(), json!({ "command": "echo hi" }));
        assert!(active.lock().unwrap().contains(&skill.id));
        assert!(shell_guard(&[skill], &active, &shell).is_some());
    }

    #[test]
    fn reading_or_grepping_a_skill_marks_it_active_and_allows_its_root() {
        let base = std::env::temp_dir().join(format!(
            "flowy-agent-skill-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let cwd = base.join("cwd");
        let skill_root = base.join("skill");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&skill_root).unwrap();
        std::fs::write(skill_root.join("SKILL.md"), "body").unwrap();
        let mut skill = installed_skill(false);
        skill.root = skill_root.clone();
        skill.skill_path = skill_root.join("SKILL.md");
        let active = Arc::new(Mutex::new(HashSet::new()));
        let read = args(
            "read_file",
            json!({ "path": skill.skill_path.display().to_string() }),
        );
        assert_eq!(path_guard(&cwd, &[skill.clone()], &active, &read), None);
        assert!(active.lock().unwrap().contains(&skill.id));

        let grep_active = Arc::new(Mutex::new(HashSet::new()));
        let grep = args(
            "grep",
            json!({
                "path": skill_root.display().to_string(),
                "pattern": "body"
            }),
        );
        assert_eq!(
            path_guard(&cwd, &[skill.clone()], &grep_active, &grep),
            None
        );
        assert!(grep_active.lock().unwrap().contains(&skill.id));
        let _ = std::fs::remove_dir_all(base);
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
        let active = Arc::new(Mutex::new(HashSet::new()));
        for tool in ["create_deck", "verify_deck", "verify_artifact"] {
            let escaped = args(tool, json!({ "path": "../outside.pptx" }));
            assert!(
                path_guard(root, &[], &active, &escaped).is_some(),
                "{tool} must reject parent traversal"
            );
            let inside = args(tool, json!({ "path": "outputs/sub/deck.pptx" }));
            assert_eq!(path_guard(root, &[], &active, &inside), None);
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
                provenance_warnings: Vec::new(),
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
                provenance_warnings: Vec::new(),
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
                display_text: None,
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
        // Symlink + `..` traversal must resolve link before folding `..` so
        // `escape_link/../anything` never collapses back inside `workspace`.
        assert!(
            !confined_to(&workspace, "escape_link/../escaped_sibling.txt"),
            "symlink followed by parent traversal must not collapse inside workspace"
        );
        // Windows reserved device names must always be refused.
        assert!(!confined_to(&workspace, "CON"));
        assert!(!confined_to(&workspace, "subdir/NUL.txt"));
        assert!(!confined_to(&workspace, "COM1"));
        // Normal non-existent multi-level path inside workspace must still pass.
        assert!(confined_to(&workspace, "outputs/new_sub/good.txt"));

        #[cfg(windows)]
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shell_policy_guard_blocks_dangerous_commands_and_allows_workspace_work()
    {
        let cwd = Path::new("workspace");
        for blocked in [
            ("bash", "tccutil reset All com.flowy.app"),
            ("bash", "rm -rf /"),
            ("powershell", "Remove-Item -Recurse -Force C:\\Windows"),
            (
                "powershell",
                "Get-Content $env:APPDATA\\Flowy\\Flowy\\data\\auth\\session.json",
            ),
        ] {
            let args = ToolCallArgs {
                call_id: "c-block".to_owned(),
                name: blocked.0.to_owned(),
                arguments: serde_json::json!({ "command": blocked.1 }),
            };
            assert!(
                super::shell_policy_guard(cwd, &args).is_some(),
                "expected shell_policy_guard to block {blocked:?}"
            );
        }
        for allowed in [
            ("powershell", "Write-Output 'ok'"),
            ("bash", "rm -rf ./tmp_out"),
        ] {
            let args = ToolCallArgs {
                call_id: "c-allow".to_owned(),
                name: allowed.0.to_owned(),
                arguments: serde_json::json!({ "command": allowed.1 }),
            };
            assert_eq!(
                super::shell_policy_guard(cwd, &args),
                None,
                "expected shell_policy_guard to allow {allowed:?}"
            );
        }
    }

    #[test]
    fn dropping_handle_while_autonomous_drain_runs_never_panics_runtime() {
        let root = std::env::temp_dir()
            .join(format!("flowy-rt-drop-safety-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_autonomous_drain(true);
        handle
            .accept_deferred("queued right before immediate handle drop")
            .expect("accept");
        // Drop immediately while autonomous drain worker task may still hold a
        // strong `Arc<Harness>` upgrade inside Tokio worker threads.
        drop(handle);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn session_needs_attention_clears_after_turn_2_succeeds_and_flags_partial()
    {
        let root = std::env::temp_dir()
            .join(format!("flowy-attention-turns-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        let session = handle.create_home_session().expect("session");
        {
            let lines = vec![
                ChatLine::User {
                    text: "Turn 1".into(),
                },
                ChatLine::Verification {
                    report_id: "r1".into(),
                    artifact_path: "outputs/sub-1/deck.pptx".into(),
                    artifact_hash: "aaaa".into(),
                    ok: false,
                    issues: vec![VerificationIssue {
                        code: "STRUCTURE".into(),
                        field: "slides".into(),
                    }],
                    semantic_hint: None,
                    override_used: false,
                    provenance_warnings: Vec::new(),
                },
            ];
            lock(&handle.inner.state.live)
                .get_mut(&session)
                .expect("live session")
                .projection
                .lines = lines;
        }
        assert!(handle.session_needs_attention(&session));

        // Turn 2 succeeds cleanly -> latest turn has no failed card.
        {
            let turn2 = [
                ChatLine::User {
                    text: "Turn 2 fix".into(),
                },
                ChatLine::Verification {
                    report_id: "r2".into(),
                    artifact_path: "outputs/sub-2/deck.pptx".into(),
                    artifact_hash: "bbbb".into(),
                    ok: true,
                    issues: Vec::new(),
                    semantic_hint: None,
                    override_used: false,
                    provenance_warnings: Vec::new(),
                },
            ];
            lock(&handle.inner.state.live)
                .get_mut(&session)
                .expect("live session")
                .projection
                .lines
                .extend(turn2);
        }
        assert!(
            !handle.session_needs_attention(&session),
            "Turn 1 failure must not keep attention badge stuck after Turn 2 passes"
        );

        // Latest submission Partial without failed card still needs attention.
        let engine = lock(&handle.inner.engine).clone().expect("engine");
        engine
            .set_submission_outcome(
                &session,
                "submission-partial-1",
                flowy_agent_core::SubmissionOutcome::Partial {
                    missing: vec!["budget.xlsx".into()],
                },
            )
            .expect("set outcome");
        assert!(handle.session_needs_attention(&session));

        drop(handle);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn boot_emits_receipt_updates_after_initialized_and_releases_stale_lease() {
        let root = std::env::temp_dir()
            .join(format!("flowy-boot-order-{}", next_stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let queue = AcceptanceQueue::new(root.clone());
        let now = super::now_ms();
        let stale = queue.accept(None, "crashed in flight", now).unwrap();
        queue
            .take(&stale.id, "dead-proc-att", now, 60_000)
            .unwrap()
            .unwrap();

        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        let updates = handle.drain_updates();
        let init_pos = updates
            .iter()
            .position(|u| matches!(u, super::AgentUpdate::Initialized))
            .expect("Initialized emitted");
        let receipt_pos = updates
            .iter()
            .position(|u| {
                matches!(
                    u,
                    super::AgentUpdate::Receipt {
                        id,
                        state: ReceiptState::Retrying
                    } if id == &stale.id
                )
            })
            .expect("stale lease released to Retrying on boot");
        assert!(
            receipt_pos > init_pos,
            "boot receipt updates must arrive strictly after AgentUpdate::Initialized"
        );
        assert_eq!(
            handle.receipt_state(&stale.id),
            Some(ReceiptState::Retrying)
        );
        drop(handle);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn disambiguation_card_survives_restart_and_resolves_only_selected_object()
    {
        let root = std::env::temp_dir()
            .join(format!("flowy-s1-disambig-{}", next_stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let wait_idle = |handle: &AgentHandle, sid: &str| {
            for _ in 0..200 {
                if !handle.session_status(sid).is_busy() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("session {sid} remained busy");
        };

        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        let sid = handle.create_home_session().unwrap();
        // Turn 1: BASE contract (xlsx + pptx, 30万, 2026-11-27)
        handle
            .submit_chat(
                101,
                "请帮青禾项目做一份报价表和三页青禾项目报价汇报.pptx，报价 30 万元，汇报日期是 2026 年 11 月 27 日",
            )
            .unwrap();
        wait_idle(&handle, &sid);
        // Turn 2: Pending proposal recorded as Record (must not mutate 30万)
        handle
            .submit_chat(
                102,
                "会议记录：客户口头提到报价可考虑 35 万元，尚待确认，先别改正式文件。",
            )
            .unwrap();
        wait_idle(&handle, &sid);
        // Turn 3: Twin PPT added
        handle
            .submit_chat(
                103,
                "保留青禾项目报价汇报.pptx，另做一份三页的青禾项目内部讨论稿.pptx",
            )
            .unwrap();
        wait_idle(&handle, &sid);
        // Turn 4: Ambiguous edit -> must park in WaitingForUser WITHOUT running a worker
        handle
            .submit_chat(104, "把刚才那份的报价改成 32 万元。")
            .unwrap();
        assert_eq!(
            handle.session_status(&sid),
            super::TurnStatus::WaitingForUser
        );
        assert!(!handle.has_busy_session());
        assert!(handle.session_needs_attention(&sid));
        let cards = handle.session_pending_cards(&sid);
        assert_eq!(cards.len(), 1);
        let (card_id, ppt1_id, ppt2_id, xlsx_id) = match &cards[0] {
            super::PendingCard::Disambiguation {
                card_id,
                options,
                impact_boundary,
                ..
            } => {
                assert_eq!(
                    options.len(),
                    3,
                    "2 PPTs + 1 XLSX, Record excluded"
                );
                assert!(impact_boundary.contains("只改所选"));
                let p1 = options
                    .iter()
                    .find(|o| o.label == "青禾项目报价汇报.pptx")
                    .unwrap()
                    .object_id
                    .clone();
                let p2 = options
                    .iter()
                    .find(|o| o.label == "青禾项目内部讨论稿.pptx")
                    .unwrap()
                    .object_id
                    .clone();
                let x = options
                    .iter()
                    .find(|o| o.extension == "xlsx")
                    .unwrap()
                    .object_id
                    .clone();
                (card_id.clone(), p1, p2, x)
            }
            other @ super::PendingCard::ToolApproval { .. } => {
                panic!("expected Disambiguation card, got {other:?}")
            }
        };

        // Bind a queued receipt to this WaitingForUser session to prove boot
        // protects its lease from being forcibly released/requeued.
        let queue = AcceptanceQueue::new(root.clone());
        let now = super::now_ms();
        let pinned_receipt = queue
            .accept(Some(&sid), "deferred while waiting", now)
            .unwrap();
        queue
            .take(&pinned_receipt.id, "wait-att", now, 120_000)
            .unwrap()
            .unwrap();
        queue
            .bind(&pinned_receipt.id, "wait-att", &sid, "sub-wait", now)
            .unwrap();

        // Graceful shutdown must NOT cancel a WaitingForUser session.
        handle.prepare_shutdown_with_timeout(std::time::Duration::from_millis(
            200,
        ));
        assert_eq!(
            handle.session_status(&sid),
            super::TurnStatus::WaitingForUser
        );
        drop(handle);

        // Cold restart must restore WaitingForUser + PendingCard + protect lease.
        let reopened = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        assert_eq!(
            reopened.session_status(&sid),
            super::TurnStatus::WaitingForUser
        );
        assert_eq!(reopened.session_pending_cards(&sid).len(), 1);
        assert_eq!(
            reopened.receipt_state(&pinned_receipt.id),
            Some(ReceiptState::Running),
            "WaitingForUser session's lease must not be forcibly released at boot"
        );

        // Invalid object_id is rejected fail-closed.
        assert_eq!(
            reopened.apply_card_action(super::CardAction::ResolveCard {
                session_id: sid.clone(),
                card_id: card_id.clone(),
                selection: super::CardSelection::Disambiguate {
                    object_ids: vec!["record-nonexistent".to_owned()],
                },
            }),
            Err("notice.card_selection_invalid")
        );

        // Resolve card selecting ONLY PPT #1 -> resumes and isolates sibling objects.
        let outcome = reopened
            .apply_card_action(super::CardAction::ResolveCard {
                session_id: sid.clone(),
                card_id: card_id.clone(),
                selection: super::CardSelection::Disambiguate {
                    object_ids: vec![ppt1_id.clone()],
                },
            })
            .expect("resolve disambiguation card");
        assert_eq!(
            outcome,
            super::CardActionOutcome::CardResolved {
                card_id,
                resumed: true,
            }
        );
        wait_idle(&reopened, &sid);
        assert_eq!(reopened.session_pending_cards(&sid), Vec::new());

        let engine = lock(&reopened.inner.engine).clone().expect("engine");
        let rec = engine.session(&sid).expect("session record");
        let wc = rec.work_contract.expect("work contract");
        assert_eq!(
            wc.confirmed_facts_for(&ppt1_id)
                .get("quote")
                .map(String::as_str),
            Some("32万")
        );
        assert_eq!(
            wc.confirmed_facts_for(&ppt2_id)
                .get("quote")
                .map(String::as_str),
            Some("30万")
        );
        assert_eq!(
            wc.confirmed_facts_for(&xlsx_id)
                .get("quote")
                .map(String::as_str),
            Some("30万")
        );
        drop(reopened);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn canonical_tool_args_hash_is_order_independent_sha256() {
        let a = serde_json::json!({
            "b": 2,
            "a": { "z": true, "y": "hello" },
            "command": "rm -rf ./outputs"
        });
        let b = serde_json::json!({
            "command": "rm -rf ./outputs",
            "a": { "y": "hello", "z": true },
            "b": 2
        });
        let c = serde_json::json!({
            "command": "rm -rf ./other",
            "a": { "y": "hello", "z": true },
            "b": 2
        });
        let hash_a = super::canonical_tool_args_hash("bash", &a);
        let hash_b = super::canonical_tool_args_hash("bash", &b);
        let hash_c = super::canonical_tool_args_hash("bash", &c);
        assert_eq!(hash_a.len(), 64, "must be full SHA-256 hex digest");
        assert_eq!(hash_a, hash_b);
        assert_ne!(hash_a, hash_c);
        assert_ne!(hash_a, super::canonical_tool_args_hash("powershell", &a));
    }

    #[test]
    fn branch_heartbeat_updates_strictly_on_real_events_and_never_fabricates_progress()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-s3-heartbeat-{}", next_stamp()));
        std::fs::create_dir_all(&root).unwrap();
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        let sid = handle.create_home_session().unwrap();
        assert_eq!(handle.session_heartbeat(&sid), None);

        use flowy_agent_core::TurnObserver as _;
        let observer = super::UiObserver {
            state: Arc::clone(&handle.inner.state),
        };
        observer.tool_start(
            &sid,
            "c1",
            "read_file",
            &serde_json::json!({"path": "brief.md"}),
        );
        let snap1 = handle
            .session_heartbeat(&sid)
            .expect("heartbeat after tool_start");
        assert_eq!(snap1.last_completed, None);
        assert!(snap1.awaiting_evidence);
        assert!(snap1.current_action.contains("read_file"));
        // Repeated reads with zero new events must return the exact same snapshot.
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(handle.session_heartbeat(&sid), Some(snap1));

        observer.tool_end(&sid, "c1", "read_file", "ok", false);
        let snap2 = handle
            .session_heartbeat(&sid)
            .expect("heartbeat after tool_end");
        assert_eq!(snap2.last_completed.as_deref(), Some("read_file"));
        assert!(snap2.last_completed_at_ms.is_some());
        assert!(!snap2.awaiting_evidence);

        observer.finished(&sid, flowy_agent_core::LoopOutcome::Completed);
        let snap3 = handle
            .session_heartbeat(&sid)
            .expect("heartbeat preserved after finish");
        assert_eq!(snap3.last_completed.as_deref(), Some("read_file"));
        assert_eq!(snap3.current_action, "");
        assert_eq!(snap3.next_check_hint, None);
        assert!(!snap3.awaiting_evidence);

        drop(handle);
        let _ = std::fs::remove_dir_all(&root);
    }
}

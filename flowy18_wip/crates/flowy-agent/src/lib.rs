//! Agent host port. UI depends on this crate only — never on an agent kernel.
//!
//! Turns run through `flowy-agent-core` (pi loop + provider transport) with
//! config from env (`FLOWY_LLM_API_KEY` / `FLOWY_LLM_BASE_URL` /
//! `FLOWY_LLM_MODEL`), Flex BYOK prefs, or local echo when no key is set.
//! Loop events are projected into chat lines for Studio.

mod artifact_source;
mod artifact_store;
mod catalog;
mod config;
mod conversation_artifacts;
mod flex;
mod handle;
mod index;
mod model;
mod model_directory;
mod projection;
mod publish_tool;
mod title;

pub use artifact_store::{
    ArtifactManifest, ArtifactPreference, ArtifactRecord, ArtifactStore,
    ArtifactUsage, MAX_ARTIFACT_BYTES, MAX_DELIVERY_BYTES, MAX_DELIVERY_FILES,
    MAX_STORE_BYTES, MAX_STORED_ITEMS, PrepareArtifact, PublicationState,
    ViewCopy, can_open_with_default_app,
};
pub use catalog::{CatalogModel, ProviderCatalog};
pub use config::LlmConfig;
pub use flex::FlexProvider;
pub use flowy_agent_core::{DeliveryCommit, SessionOrigin, SessionWorkspace};
pub use handle::{AgentHandle, AgentUpdate, AgentWorkspace, SessionSummary};
pub use model::{Model, Plan};
pub use model_directory::{
    CLOUD_TIERS, TierResolution, TierSpec, is_locked, required_level,
    resolve_tier,
};
pub use projection::{
    ChatHistory, ChatLine, FileChangeKind, FileEntry, TurnStatus,
};
pub use title::{TITLE_MAX_CHARS, clamp_title, fallback_title};

#[cfg(test)]
mod perf_tests;

#[cfg(test)]
mod artifact_integration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path =
            std::env::temp_dir().join(format!("flowy-agent-{tag}-{stamp}"));
        let _ = std::fs::create_dir_all(&path);
        path
    }

    fn mock_handle(root: &std::path::Path) -> AgentHandle {
        AgentHandle::with_config(
            root.to_path_buf(),
            LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: String::new(),
                ..LlmConfig::default()
            },
        )
    }

    #[test]
    fn project_workspace_follows_sessions_and_restores_after_restart() {
        let root = ScratchRoot::new("project-workspaces");
        let project_a = AgentWorkspace {
            project_id: "project-a".into(),
            root: root.0.join("projects/a"),
        };
        let project_b = AgentWorkspace {
            project_id: "project-b".into(),
            root: root.0.join("projects/b"),
        };
        std::fs::create_dir_all(&project_a.root).unwrap();
        std::fs::create_dir_all(&project_b.root).unwrap();

        let handle = mock_handle(&root.0);
        handle.set_workspace_for_new_sessions(Some(project_a.clone()));
        let session_a = handle.create_session().unwrap();
        handle.clear_active().unwrap();
        handle.set_workspace_for_new_sessions(Some(project_b.clone()));
        let session_b = handle.create_session().unwrap();

        assert!(handle.list_sessions().iter().any(|session| {
            session.id == session_a
                && session.project_id.as_deref() == Some("project-a")
        }));

        handle.select_session(&session_a).unwrap();
        assert_eq!(handle.active_workspace(), Some(project_a.clone()));
        handle.select_session(&session_b).unwrap();
        assert_eq!(handle.active_workspace(), Some(project_b.clone()));
        let index = crate::index::SessionIndex::load(&root.0).unwrap();
        assert_eq!(
            index
                .sessions
                .iter()
                .find(|session| session.id == session_a)
                .unwrap()
                .workspace,
            Some(project_a.clone()),
        );
        assert_eq!(
            index
                .sessions
                .iter()
                .find(|session| session.id == session_b)
                .unwrap()
                .workspace,
            Some(project_b.clone()),
        );
        drop(handle);

        let reopened = mock_handle(&root.0);
        assert_eq!(reopened.active_workspace(), Some(project_b));
        reopened.select_session(&session_a).unwrap();
        assert_eq!(reopened.active_workspace(), Some(project_a));
    }

    struct ScratchRoot(std::path::PathBuf);

    impl ScratchRoot {
        fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            loop {
                let id =
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let root = std::env::temp_dir().join(format!(
                    "flowy-agent-test-{}-{tag}-{id}",
                    std::process::id()
                ));
                match std::fs::create_dir(&root) {
                    Ok(()) => return Self(root),
                    Err(error)
                        if error.kind()
                            == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        panic!("could not create test directory: {error}")
                    }
                }
            }
        }
    }

    impl Drop for ScratchRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn send_echoes_through_mock_when_no_key() {
        let root = temp_root("echo");
        let handle = AgentHandle::new(root);
        handle.create_session().unwrap();
        handle.send("hello harness").unwrap();
        // Wait for idle completion.
        for _ in 0..50 {
            if matches!(handle.turn_status(), TurnStatus::Completed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let lines = handle.lines();
        assert!(
            lines.iter().any(|line| matches!(line, ChatLine::User { text } if text == "hello harness")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| {
                matches!(line, ChatLine::Assistant { text } if text == "hello harness")
            }),
            "{lines:?}"
        );
    }

    #[test]
    fn empty_prompt_is_rejected() {
        let handle = AgentHandle::new(temp_root("empty"));
        assert_eq!(handle.send("  "), Err("notice.prompt_empty"));
    }

    #[test]
    fn failed_source_deletion_restores_full_core_history() {
        let root = ScratchRoot::new("delete-source-index-failure");
        let handle = mock_handle(&root.0);
        let id = handle.create_home_session().unwrap();
        handle.send("keep the full history").unwrap();
        for _ in 0..100 {
            if !handle.turn_status().is_busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!handle.turn_status().is_busy());
        let before = flowy_agent_core::SessionIndex::load(&root.0)
            .unwrap()
            .get(&id)
            .unwrap()
            .clone();
        let index_path = crate::index::SessionIndex::path(&root.0);
        std::fs::remove_file(&index_path).unwrap();
        std::fs::create_dir(&index_path).unwrap();
        assert_eq!(handle.delete_session(&id), Err("notice.storage_error"));
        let restored = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
        assert_eq!(restored.get(&id), Some(&before));
        assert!(!handle.source_deleted(&id));
        assert!(
            handle
                .list_sessions()
                .iter()
                .any(|session| session.id == id)
        );
    }

    #[test]
    fn new_home_origin_and_workspace_survive_ui_index_rebuild() {
        let root = ScratchRoot::new("home-source-rebuild");
        let handle = mock_handle(&root.0);
        let id = handle.create_home_session().unwrap();
        let original = flowy_agent_core::SessionIndex::load(&root.0)
            .unwrap()
            .get(&id)
            .unwrap()
            .clone();
        assert_eq!(original.origin, SessionOrigin::Home);
        assert!(original.workspace.as_ref().unwrap().root.ends_with(&id));
        drop(handle);
        std::fs::write(crate::index::SessionIndex::path(&root.0), b"{broken")
            .unwrap();
        let restored = mock_handle(&root.0);
        let history = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
        assert_eq!(history.get(&id).unwrap().origin, SessionOrigin::Home);
        assert_eq!(history.get(&id).unwrap().workspace, original.workspace);
        assert!(restored.source_available(&id));
    }

    #[test]
    fn title_fallback_used_without_live_key() {
        let root = temp_root("title");
        let handle = mock_handle(&root);
        handle.create_session().unwrap();
        handle.send("build a slide deck about cats").unwrap();
        for _ in 0..50 {
            if !handle.session_title().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let title = handle.session_title();
        assert!(
            title.contains("build")
                || title.contains("slide")
                || title.contains("cats"),
            "title={title}"
        );
        let history = flowy_agent_core::SessionIndex::load(&root).unwrap();
        let active = handle.active_session_id().expect("active session");
        let record = history
            .sessions
            .iter()
            .find(|record| record.id == active)
            .expect("session history");
        assert_eq!(record.title, title);
        assert!(record.titled);
    }

    #[test]
    fn restoring_history_does_not_project_replayed_events_twice() {
        let root = ScratchRoot::new("restore-duplicates");
        let lines: Vec<_> = (0..1000)
            .map(|i| {
                if i % 2 == 0 {
                    ChatLine::User {
                        text: format!("user {i}"),
                    }
                } else {
                    ChatLine::Assistant {
                        text: format!("assistant {i}"),
                    }
                }
            })
            .collect();
        crate::index::SessionIndex {
            active: Some("restored-session".into()),
            sessions: vec![crate::index::SessionRecord {
                id: "restored-session".into(),
                title: "Existing".into(),
                updated_at: 1,
                lines: lines.clone(),
                status: TurnStatus::Completed,
                titled: true,
                workspace: None,
                origin: SessionOrigin::default(),
                session_workspace: None,
            }],
        }
        .save(&root.0)
        .unwrap();
        let handle = mock_handle(&root.0);
        assert_eq!(handle.lines(), lines);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(handle.lines(), lines);
    }

    #[test]
    fn restoring_rich_history_preserves_rows_without_replaying_them_twice() {
        let root = ScratchRoot::new("restore-rich-history");
        let mut lines = Vec::new();
        for i in 0..200 {
            lines.push(ChatLine::User {
                text: format!("question {i}"),
            });
            lines.push(ChatLine::Reasoning {
                text: format!("reasoning {i}"),
                duration_secs: 2,
                live: false,
            });
            lines.push(ChatLine::ToolCall {
                name: "read_file".into(),
                target: format!("file-{i}"),
                call_id: format!("call-{i}"),
                running: false,
            });
            lines.push(ChatLine::ToolResult {
                text: format!("result {i}"),
                is_error: i % 2 == 0,
            });
            lines.push(ChatLine::Assistant {
                text: format!("answer {i}"),
            });
        }
        crate::index::SessionIndex {
            active: Some("rich-session".into()),
            sessions: vec![crate::index::SessionRecord {
                id: "rich-session".into(),
                title: "Rich history".into(),
                updated_at: 1,
                lines: lines.clone(),
                status: TurnStatus::Completed,
                titled: true,
                workspace: None,
                origin: SessionOrigin::default(),
                session_workspace: None,
            }],
        }
        .save(&root.0)
        .unwrap();

        let handle = mock_handle(&root.0);
        assert_eq!(handle.lines(), lines);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(handle.lines(), lines);
    }

    #[test]
    fn restoring_completed_turn_repairs_a_stale_index() {
        use flowy_agent_core::{
            SessionIndex as History, SessionRecord, TurnState,
            types::{ContentBlock, Message, MessageSource, Role},
        };

        let root = ScratchRoot::new("restore-completed-turn");
        let id = "restored-session";
        let prompt = ChatLine::User {
            text: "repeated question".into(),
        };
        let earlier = ChatLine::Assistant {
            text: "earlier answer".into(),
        };
        let reasoning = ChatLine::Reasoning {
            text: "verified details".into(),
            duration_secs: 3,
            live: false,
        };
        let receipt = ChatLine::TaskReceipt {
            task_id: "inventory-test".into(),
            goal: "saved receipt".into(),
        };
        let mut index = crate::index::SessionIndex {
            active: Some(id.into()),
            sessions: vec![crate::index::SessionRecord {
                id: id.into(),
                title: "Existing".into(),
                updated_at: 1,
                lines: vec![
                    prompt.clone(),
                    earlier.clone(),
                    prompt.clone(),
                    reasoning.clone(),
                    receipt.clone(),
                ],
                status: TurnStatus::Running,
                titled: true,
                workspace: None,
                origin: SessionOrigin::default(),
                session_workspace: None,
            }],
        };
        index.save(&root.0).unwrap();

        let user_message = |id: &str| Message {
            id: id.into(),
            role: Role::User,
            content: vec![ContentBlock::text("repeated question")],
            source: MessageSource::User,
        };
        let assistant_message = |id: &str, content| Message {
            id: id.into(),
            role: Role::Assistant,
            content,
            source: MessageSource::Model {
                provider: "openai".into(),
                model: "test".into(),
            },
        };
        History {
            active: None,
            sessions: vec![SessionRecord {
                id: id.into(),
                updated_at: 2,
                status: TurnState::Completed,
                messages: vec![
                    user_message("u-1"),
                    assistant_message(
                        "a-1",
                        vec![ContentBlock::text("earlier answer")],
                    ),
                    user_message("submission-00000000000000000000000000000001"),
                    assistant_message(
                        "a-2",
                        vec![
                            ContentBlock::Reasoning {
                                text: "verified details".into(),
                            },
                            ContentBlock::text("durably saved answer"),
                        ],
                    ),
                ],
                ..SessionRecord::default()
            }],
        }
        .save(&root.0)
        .unwrap();

        let expected = [
            prompt.clone(),
            earlier,
            prompt.clone(),
            reasoning,
            ChatLine::Assistant {
                text: "durably saved answer".into(),
            },
            receipt,
        ];
        for _ in 0..2 {
            let handle = mock_handle(&root.0);
            assert_eq!(handle.turn_status(), TurnStatus::Completed);
            assert_eq!(handle.lines(), expected);
        }

        // The same prompt in an additional interrupted turn is a different run.
        index.sessions[0].lines = expected.to_vec();
        index.sessions[0].lines.push(prompt);
        index.sessions[0].updated_at = 2;
        index.save(&root.0).unwrap();
        let handle = mock_handle(&root.0);
        assert_eq!(handle.turn_status(), TurnStatus::Stopped);
        assert_eq!(handle.lines(), index.sessions[0].lines);
    }

    #[test]
    fn interrupted_session_can_be_used_after_restart() {
        let root = ScratchRoot::new("interrupted");
        let index = crate::index::SessionIndex {
            active: Some("interrupted-session".into()),
            sessions: vec![crate::index::SessionRecord {
                id: "interrupted-session".into(),
                title: "Existing task".into(),
                updated_at: 1,
                lines: vec![
                    ChatLine::User {
                        text: "Earlier prompt".into(),
                    },
                    ChatLine::Reasoning {
                        text: "thinking".into(),
                        duration_secs: 2,
                        live: true,
                    },
                    ChatLine::ToolCall {
                        name: "read_file".into(),
                        target: "file".into(),
                        call_id: "call-1".into(),
                        running: true,
                    },
                ],
                status: TurnStatus::Running,
                titled: true,
                workspace: None,
                origin: SessionOrigin::default(),
                session_workspace: None,
            }],
        };
        std::fs::create_dir_all(root.0.join("sessions")).unwrap();
        std::fs::write(
            crate::index::SessionIndex::path(&root.0),
            serde_json::to_vec(&index).unwrap(),
        )
        .unwrap();

        let handle = mock_handle(&root.0);
        assert_eq!(handle.turn_status(), TurnStatus::Stopped);
        let saved = crate::index::SessionIndex::load(&root.0).unwrap();
        assert_eq!(saved.sessions[0].status, TurnStatus::Stopped);
        assert!(saved.sessions[0].lines.iter().any(|line| matches!(
            line,
            ChatLine::Reasoning { live: false, .. }
        )));
        assert!(saved.sessions[0].lines.iter().any(|line| matches!(
            line,
            ChatLine::ToolCall { running: false, .. }
        )));
        assert!(handle.lines().iter().any(|line| matches!(line, ChatLine::User { text } if text == "Earlier prompt")));
        assert!(handle.send("Continue").is_ok());
        for _ in 0..50 {
            if !matches!(handle.turn_status(), TurnStatus::Running) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn corrupt_index_is_quarantined_and_app_still_opens() {
        let root = ScratchRoot::new("corrupt");
        let path = crate::index::SessionIndex::path(&root.0);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{broken").unwrap();

        let handle = mock_handle(&root.0);
        assert!(handle.create_session().is_ok());
        assert!(
            handle
                .drain_updates()
                .iter()
                .any(|update| matches!(update, AgentUpdate::StorageError))
        );
        let backup = std::fs::read_dir(root.0.join("sessions"))
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("index.corrupt-")
            })
            .expect("corrupt index backup");
        assert_eq!(std::fs::read(backup.path()).unwrap(), b"{broken");
    }

    #[test]
    fn corrupt_index_is_quarantined_and_history_sessions_are_restored() {
        let root = ScratchRoot::new("corrupt-index-history");
        let history = flowy_agent_core::SessionIndex {
            active: None,
            sessions: vec![flowy_agent_core::SessionRecord {
                id: "history-only".into(),
                title: "From history".into(),
                titled: true,
                updated_at: 1,
                status: flowy_agent_core::TurnState::Completed,
                messages: vec![
                    flowy_agent_core::Message {
                        id: "m1".into(),
                        role: flowy_agent_core::Role::User,
                        content: vec![flowy_agent_core::ContentBlock::text(
                            "hello",
                        )],
                        source: flowy_agent_core::MessageSource::User,
                    },
                    flowy_agent_core::Message {
                        id: "m2".into(),
                        role: flowy_agent_core::Role::Assistant,
                        content: vec![flowy_agent_core::ContentBlock::text(
                            "hi",
                        )],
                        source: flowy_agent_core::MessageSource::Model {
                            provider: "flowy".into(),
                            model: "m".into(),
                        },
                    },
                ],
                todos: Vec::new(),
                ..flowy_agent_core::SessionRecord::default()
            }],
        };
        history.save(&root.0).unwrap();
        std::fs::write(crate::index::SessionIndex::path(&root.0), b"{broken")
            .unwrap();

        let handle = mock_handle(&root.0);
        let sessions = handle.list_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "history-only");
        handle.select_session("history-only").unwrap();
        assert_eq!(
            handle.lines(),
            vec![
                ChatLine::User {
                    text: "hello".into()
                },
                ChatLine::Assistant { text: "hi".into() }
            ]
        );
    }

    #[test]
    fn failed_session_storage_rejects_new_sessions_and_submissions() {
        let root = ScratchRoot::new("save-failure");
        let handle = mock_handle(&root.0);
        let active = handle.create_session().unwrap();
        handle.drain_updates();
        std::fs::remove_dir_all(root.0.join("sessions")).unwrap();
        std::fs::write(root.0.join("sessions"), b"not a directory").unwrap();

        assert_eq!(handle.create_session(), Err("notice.storage_error"));
        assert_eq!(
            handle.send("cannot durably submit"),
            Err("notice.storage_error")
        );
        assert_eq!(
            handle.active_session_id().as_deref(),
            Some(active.as_str())
        );
        assert_eq!(handle.list_sessions().len(), 1);
        assert_eq!(handle.lines().as_slice(), []);
        assert!(!handle.turn_status().is_busy());
    }

    #[test]
    fn corrupt_history_is_quarantined_and_projection_still_loads() {
        let root = ScratchRoot::new("corrupt-history");
        let lines = vec![
            ChatLine::User {
                text: "Earlier prompt".into(),
            },
            ChatLine::Assistant {
                text: "Earlier answer".into(),
            },
        ];
        crate::index::SessionIndex {
            active: Some("history-session".into()),
            sessions: vec![crate::index::SessionRecord {
                id: "history-session".into(),
                title: "Existing".into(),
                updated_at: 1,
                lines: lines.clone(),
                status: TurnStatus::Completed,
                titled: true,
                workspace: None,
                origin: SessionOrigin::default(),
                session_workspace: None,
            }],
        }
        .save(&root.0)
        .unwrap();
        std::fs::write(
            flowy_agent_core::SessionIndex::path(&root.0),
            b"{broken",
        )
        .unwrap();

        let handle = mock_handle(&root.0);
        assert_eq!(handle.lines(), lines);
        assert!(handle.create_session().is_ok());
        assert!(
            std::fs::read_dir(root.0.join("sessions"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("history.corrupt-"))
        );
    }
}

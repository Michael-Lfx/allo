//! Agent host port. UI depends on this crate only — never on `dsh_rs::llm`.
//!
//! Boots the dsh base bundle with OpenAI-compatible config from env
//! (`FLOWY_LLM_API_KEY` / `FLOWY_LLM_BASE_URL` / `FLOWY_LLM_MODEL`), Flex
//! BYOK prefs, or mock when no key is set. Projects session events into chat
//! lines for Studio.

mod catalog;
mod config;
mod flex;
mod handle;
mod index;
mod model;
mod projection;
mod title;

pub use catalog::{CatalogModel, ProviderCatalog};
pub use config::LlmConfig;
pub use flex::FlexProvider;
pub use handle::{AgentHandle, AgentUpdate, SessionSummary};
pub use model::{Model, Plan};
pub use projection::{ChatLine, TurnStatus};
pub use title::{TITLE_MAX_CHARS, clamp_title, fallback_title};

#[cfg(test)]
mod perf_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
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
            },
        )
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
    fn title_fallback_used_without_live_key() {
        let root = temp_root("title");
        let handle = AgentHandle::new(root);
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
    fn corrupt_index_is_preserved_and_cannot_be_overwritten() {
        let root = ScratchRoot::new("corrupt");
        let path = crate::index::SessionIndex::path(&root.0);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{broken").unwrap();

        let handle = mock_handle(&root.0);
        assert!(handle.create_session().is_err());
        assert!(matches!(
            handle.drain_updates().as_slice(),
            [AgentUpdate::StorageError]
        ));
        assert!(handle.create_session().is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"{broken");
    }

    #[test]
    fn failed_index_save_emits_a_storage_error() {
        let root = ScratchRoot::new("save-failure");
        let handle = mock_handle(&root.0);
        handle.create_session().unwrap();
        handle.drain_updates();
        std::fs::remove_dir_all(root.0.join("sessions")).unwrap();
        std::fs::write(root.0.join("sessions"), b"not a directory").unwrap();

        handle.create_session().unwrap();
        assert!(
            handle
                .drain_updates()
                .iter()
                .any(|update| matches!(update, AgentUpdate::StorageError))
        );
    }
}

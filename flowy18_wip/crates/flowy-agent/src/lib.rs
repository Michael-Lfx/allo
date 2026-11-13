//! Agent host port. UI depends on this crate only — never on `dsh_rs::llm`.
//!
//! Boots the dsh base bundle with OpenAI-compatible config from env
//! (`FLOWY_LLM_API_KEY` / `FLOWY_LLM_BASE_URL` / `FLOWY_LLM_MODEL`), Flex
//! BYOK prefs, or mock when no key is set. Projects session events into chat
//! lines for Studio.

mod config;
mod flex;
mod handle;
mod index;
mod model;
mod projection;
mod title;

pub use config::LlmConfig;
pub use flex::FlexProvider;
pub use handle::{AgentHandle, AgentUpdate, SessionSummary};
pub use model::{Model, Plan};
pub use projection::{ChatLine, TurnStatus};
pub use title::{TITLE_MAX_CHARS, clamp_title, fallback_title};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("flowy-agent-{tag}-{stamp}"));
        let _ = std::fs::create_dir_all(&path);
        path
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
            title.contains("build") || title.contains("slide") || title.contains("cats"),
            "title={title}"
        );
    }
}

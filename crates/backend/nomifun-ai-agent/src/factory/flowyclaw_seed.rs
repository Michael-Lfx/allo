//! Materialize a one-shot import seed the first time a migrated chat is sent.
//!
//! The import writes this file outside `nomi-sessions`. Putting it in that
//! directory at import time would let `SessionManager::create` delete it:
//! create prunes the oldest session files once the index passes its cap, and
//! a migrated chat the user never opens should not occupy that cap.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use nomi_agent::session::{Session, SessionManager};
use nomi_types::message::{Message, TokenUsage};
use serde::Deserialize;
use tracing::warn;

#[derive(Debug, Deserialize)]
struct ImportSeed {
    version: u32,
    #[serde(default)]
    messages: Vec<Message>,
}

pub(crate) struct ImportSeedContext<'a> {
    pub data_dir: &'a Path,
    pub conversation_id: &'a str,
    pub cwd: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub owner_token: &'a str,
    pub conv_created_ms: i64,
}

pub(crate) fn materialize_import_seed(
    session_mgr: &SessionManager,
    context: &ImportSeedContext<'_>,
) -> Option<Session> {
    let path = seed_path(context.data_dir, context.conversation_id)?;
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let seed: ImportSeed = match serde_json::from_slice::<ImportSeed>(&bytes) {
        Ok(seed) if seed.version == 1 && !seed.messages.is_empty() => seed,
        _ => {
            warn!("legacy import seed was unusable; the chat will start without it");
            let _ = std::fs::rename(&path, path.with_extension("unusable.json"));
            return None;
        }
    };
    if context.owner_token.is_empty() {
        return None;
    }
    let now = Utc::now();
    let created = if now.timestamp_millis() < context.conv_created_ms {
        DateTime::<Utc>::from_timestamp_millis(context.conv_created_ms).unwrap_or(now)
    } else {
        now
    };
    let session = Session {
        id: context.conversation_id.to_owned(),
        created_at: created,
        updated_at: created,
        provider: context.provider.to_owned(),
        model: context.model.to_owned(),
        cwd: context.cwd.to_owned(),
        total_usage: TokenUsage::default(),
        messages: seed.messages,
        owner_token: Some(context.owner_token.to_owned()),
        activated_deferred_tools: Vec::new(),
        editable_turn: None,
        last_turn_ended_at: Some(created),
    };
    if session_mgr.save(&session).is_err() || session_mgr.update_index_for(&session).is_err() {
        warn!("legacy import seed could not be saved into the engine session");
        return None;
    }
    let _ = std::fs::remove_file(&path);
    Some(session)
}

pub(crate) fn seed_path(data_dir: &Path, conversation_id: &str) -> Option<PathBuf> {
    if !is_safe_conversation_id(conversation_id) {
        return None;
    }
    Some(
        data_dir
            .join("imports")
            .join("flowyclaw")
            .join("seeds")
            .join(format!("{conversation_id}.json")),
    )
}

fn is_safe_conversation_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (32..=36).contains(&bytes.len())
        && bytes.iter().all(|byte| byte.is_ascii_hexdigit() || *byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_agent::session::session_belongs_to;

    const CONVERSATION_ID: &str = "0190f5fe-7c00-7000-8000-0000000000ab";
    const OWNER: &str = "1700000000000";

    #[test]
    fn materialize_accepts_the_conversation_owner_and_removes_the_seed() {
        let root = tempfile::tempdir().unwrap();
        let path = seed_path(root.path(), CONVERSATION_ID).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"version":1,"messages":[{"role":"user","content":[{"type":"text","text":"hello"}]}]}"#,
        )
        .unwrap();
        let manager = SessionManager::new(root.path().join("nomi-sessions"), 100);
        let session = materialize_import_seed(
            &manager,
            &ImportSeedContext {
                data_dir: root.path(),
                conversation_id: CONVERSATION_ID,
                cwd: "/tmp/work",
                provider: "openai",
                model: "m",
                owner_token: OWNER,
                conv_created_ms: 1_700_000_000_000,
            },
        )
        .expect("seed should materialize");

        assert!(session_belongs_to(
            session.owner_token.as_deref(),
            session.created_at.timestamp_millis(),
            OWNER,
            1_700_000_000_000,
        ));
        assert!(!path.exists());
        let loaded = manager.load(CONVERSATION_ID).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert!(materialize_import_seed(
            &manager,
            &ImportSeedContext {
                data_dir: root.path(),
                conversation_id: CONVERSATION_ID,
                cwd: "/tmp/work",
                provider: "openai",
                model: "m",
                owner_token: OWNER,
                conv_created_ms: 1_700_000_000_000,
            },
        )
        .is_none());
    }

    #[test]
    fn unsafe_conversation_id_does_not_resolve_a_seed_path() {
        let root = tempfile::tempdir().unwrap();
        assert!(seed_path(root.path(), "../secret").is_none());
        assert!(seed_path(root.path(), "not-a-uuid").is_none());
    }
}

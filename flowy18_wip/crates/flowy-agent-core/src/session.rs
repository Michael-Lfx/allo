//! Structured session history owned by the host.
//!
//! The previous index stored only display lines, so tool arguments were lost on
//! restore (`{"target": ...}` was synthesized). This store keeps whole
//! [`Message`] values, including raw tool-call argument strings and tool-result
//! pairing, so a restored session can be sent to a model unchanged.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::types::{Message, TodoItem};

/// Durable origin label. Older sessions default to `Unknown`; callers must
/// opt a newly created global-home session into artifact delivery explicitly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionOrigin {
    Home,
    Project {
        project_id: Option<String>,
    },
    #[default]
    Unknown,
}

/// Host-assigned workspace for one session, separate from the ambient process
/// working directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionWorkspace {
    pub root: PathBuf,
}

/// The durable result of a completed turn that delivered one or more files.
/// The independent artifact-store marker remains the visibility authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryCommit {
    pub session_id: String,
    pub submission_id: String,
    /// Complete, ordered set of artifacts published by this turn.
    pub artifact_ids: Vec<String>,
    pub completed_successfully: bool,
    pub committed_at_ms: u64,
    #[serde(default)]
    pub origin: SessionOrigin,
    #[serde(default)]
    pub origin_title_snapshot: String,
}

/// Host callback that makes a prepared delivery group visible in its
/// independent artifact store. Implementations must be idempotent.
#[async_trait::async_trait]
pub trait DeliveryCommitter: Send + Sync {
    async fn commit(&self, commit: &DeliveryCommit) -> Result<(), String>;
}

/// Lifecycle state of the last turn in a session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnState {
    #[default]
    Idle,
    Running,
    Completed,
    Stopped,
    Error {
        message: String,
    },
}

impl TurnState {
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// One durable session: metadata plus the full structured message history.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub titled: bool,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub status: TurnState,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub todos: Vec<TodoItem>,
    #[serde(default)]
    pub origin: SessionOrigin,
    #[serde(default)]
    pub workspace: Option<SessionWorkspace>,
    #[serde(default)]
    pub delivery_commits: Vec<DeliveryCommit>,
}

/// The whole session file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionIndex {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub sessions: Vec<SessionRecord>,
}

fn log_storage_error(operation: &'static str, path: &Path, error: &io::Error) {
    tracing::error!(
        target: "flowy.storage",
        operation,
        path = %path.display(),
        kind = ?error.kind(),
        error = %error,
        "storage operation failed"
    );
}

impl SessionIndex {
    /// Stored next to (not instead of) the legacy `index.json` until the switch.
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join("sessions").join("history.json")
    }

    pub fn load(root: &Path) -> io::Result<Self> {
        let path = Self::path(root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                log_storage_error("load structured history", &path, &error);
                return Err(error);
            }
        };
        serde_json::from_slice(&bytes).map_err(|error| {
            let error = io::Error::new(io::ErrorKind::InvalidData, error);
            log_storage_error("parse structured history", &path, &error);
            error
        })
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        let path = Self::path(root);
        let result = self.save_to(&path);
        if let Err(error) = &result {
            log_storage_error("save structured history", &path, error);
        }
        result
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("missing sessions directory"))?;
        fs::create_dir_all(dir)?;
        let bytes =
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let (tmp, mut file) = loop {
            let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let tmp = dir.join(format!(
                ".history.json.{}.{}.tmp",
                std::process::id(),
                number
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
            {
                Ok(file) => break (tmp, file),
                Err(error) => {
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
            }
        };
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    pub fn upsert(&mut self, record: SessionRecord) {
        if let Some(slot) =
            self.sessions.iter_mut().find(|row| row.id == record.id)
        {
            *slot = record;
        } else {
            self.sessions.push(record);
        }
        self.sessions
            .sort_by_key(|row| std::cmp::Reverse(row.updated_at));
    }

    pub fn get(&self, id: &str) -> Option<&SessionRecord> {
        self.sessions.iter().find(|row| row.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SessionRecord> {
        self.sessions.iter_mut().find(|row| row.id == id)
    }

    pub fn remove(&mut self, id: &str) -> Option<SessionRecord> {
        let index = self.sessions.iter().position(|row| row.id == id)?;
        let record = self.sessions.remove(index);
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        Some(record)
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionIndex, SessionOrigin, SessionRecord, TurnState};
    use crate::types::{ContentBlock, Message, MessageSource, Role};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("build-test")
                .join(format!(
                    "{tag}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tool_history() -> Vec<Message> {
        vec![
            Message {
                id: "u1".into(),
                role: Role::User,
                content: vec![ContentBlock::text("read the config")],
                source: MessageSource::User,
            },
            Message {
                id: "a1".into(),
                role: Role::Assistant,
                content: vec![
                    ContentBlock::text("I will read it."),
                    ContentBlock::ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        // Exact raw argument string must survive a round trip.
                        arguments: r#"{"path":"config/app.json","limit":10}"#
                            .into(),
                    },
                ],
                source: MessageSource::Model {
                    provider: "flowy".into(),
                    model: "AIPC-deepseek-v4-flash".into(),
                },
            },
            Message {
                id: "t1".into(),
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_call_id: "call-1".into(),
                    content: vec![ContentBlock::text("{\"port\":8080}")],
                    is_error: Some(false),
                    artifact: None,
                }],
                source: MessageSource::Tool {
                    tool: "read_file".into(),
                },
            },
            Message {
                id: "a2".into(),
                role: Role::Assistant,
                content: vec![ContentBlock::text("The port is 8080.")],
                source: MessageSource::Model {
                    provider: "flowy".into(),
                    model: "AIPC-deepseek-v4-flash".into(),
                },
            },
        ]
    }

    fn record() -> SessionRecord {
        SessionRecord {
            id: "session-1".into(),
            title: "config".into(),
            titled: true,
            updated_at: 42,
            status: TurnState::Completed,
            messages: tool_history(),
            todos: Vec::new(),
            origin: SessionOrigin::Unknown,
            workspace: None,
            delivery_commits: Vec::new(),
        }
    }

    #[test]
    fn tool_arguments_and_pairing_survive_a_round_trip() {
        let scratch = Scratch::new("roundtrip");
        let mut index = SessionIndex::default();
        index.active = Some("session-1".into());
        index.upsert(record());
        index.save(&scratch.0).unwrap();

        let restored = SessionIndex::load(&scratch.0).unwrap();
        assert_eq!(restored.active.as_deref(), Some("session-1"));
        let session = restored.get("session-1").unwrap();
        assert_eq!(session, &record());
        let Message { content, .. } = &session.messages[1];
        let ContentBlock::ToolCall { arguments, .. } = &content[1] else {
            panic!("tool call block missing after restore");
        };
        assert_eq!(arguments, r#"{"path":"config/app.json","limit":10}"#);
        let ContentBlock::ToolResult {
            tool_call_id,
            is_error,
            ..
        } = &session.messages[2].content[0]
        else {
            panic!("tool result block missing after restore");
        };
        assert_eq!(tool_call_id, "call-1");
        assert_eq!(*is_error, Some(false));
    }

    #[test]
    fn missing_file_is_an_empty_index_not_an_error() {
        let scratch = Scratch::new("missing");
        let index = SessionIndex::load(&scratch.0).unwrap();
        assert!(index.sessions.is_empty());
        assert!(index.active.is_none());
    }

    #[test]
    fn upsert_replaces_and_orders_by_updated_at() {
        let mut index = SessionIndex::default();
        let mut older = record();
        older.id = "older".into();
        older.updated_at = 1;
        let mut newer = record();
        newer.id = "newer".into();
        newer.updated_at = 2;
        index.upsert(older);
        index.upsert(newer);
        assert_eq!(index.sessions[0].id, "newer");

        let mut updated = record();
        updated.id = "older".into();
        updated.title = "renamed".into();
        updated.updated_at = 3;
        index.upsert(updated);
        assert_eq!(index.sessions.len(), 2);
        assert_eq!(index.get("older").unwrap().title, "renamed");
    }

    #[test]
    fn corrupted_history_is_reported_not_silently_reset() {
        let scratch = Scratch::new("corrupt");
        let dir = scratch.0.join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("history.json"), b"{broken").unwrap();
        assert!(SessionIndex::load(&scratch.0).is_err());
    }

    #[test]
    fn legacy_sessions_have_unknown_origin_and_no_delivery_authority() {
        let record: SessionRecord =
            serde_json::from_str(r#"{"id":"old-session","messages":[]}"#)
                .unwrap();
        assert_eq!(record.origin, SessionOrigin::Unknown);
        assert!(record.workspace.is_none());
        assert!(record.delivery_commits.is_empty());
        let result: ContentBlock = serde_json::from_str(
            r#"{"type":"tool-result","tool_call_id":"old-call","content":[],"is_error":false}"#,
        ).unwrap();
        assert!(matches!(
            result,
            ContentBlock::ToolResult { artifact: None, .. }
        ));
    }
}

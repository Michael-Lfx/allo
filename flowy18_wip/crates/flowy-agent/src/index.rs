use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::projection::{ChatLine, TurnStatus};
use flowy_agent_core::{
    SessionOrigin, SessionRole, SessionWorkspace, WorkAccessMode,
};

/// A project workspace attached to a chat session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspace {
    pub project_id: String,
    pub root: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    #[serde(default)]
    pub lines: Vec<ChatLine>,
    #[serde(default)]
    pub status: TurnStatus,
    #[serde(default)]
    pub titled: bool,
    #[serde(default)]
    pub workspace: Option<AgentWorkspace>,
    #[serde(default)]
    pub origin: SessionOrigin,
    #[serde(default)]
    pub session_workspace: Option<SessionWorkspace>,
    #[serde(default)]
    pub execution: flowy_agent_core::ExecutionMetadata,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "SessionRole::is_default")]
    pub role: SessionRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "WorkAccessMode::is_default")]
    pub access_mode: WorkAccessMode,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionIndex {
    pub active: Option<String>,
    pub sessions: Vec<SessionRecord>,
}

impl SessionIndex {
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join("sessions").join("index.json")
    }

    pub fn load(root: &Path) -> io::Result<Self> {
        let path = Self::path(root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                log_storage_error("load session index", &path, &error);
                return Err(error);
            }
        };
        serde_json::from_slice(&bytes).map_err(|error| {
            let error = io::Error::new(io::ErrorKind::InvalidData, error);
            log_storage_error("parse session index", &path, &error);
            error
        })
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        let path = Self::path(root);
        let result = save_json(&path, self);
        if let Err(error) = &result {
            log_storage_error("save session index", &path, error);
        }
        result
    }
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

pub fn save_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    static REPLACE: Mutex<()> = Mutex::new(());
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    fs::create_dir_all(dir).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("create {}: {error}", dir.display()),
        )
    })?;
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let (tmp, mut file) = loop {
        let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let tmp =
            dir.join(format!(".flowy.{}.{}.tmp", std::process::id(), number));
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
        // Windows can deny two simultaneous replacements of the same file.
        let _replace = REPLACE.lock().unwrap();
        fs::rename(&tmp, path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("replace {}: {error}", path.display()),
            )
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_saves_do_not_share_temporary_files() {
        let root = std::env::temp_dir().join(format!(
            "flowy-index-concurrent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        const WRITERS: usize = 8;
        let barrier = Arc::new(Barrier::new(WRITERS));
        let writers: Vec<_> = (0..WRITERS)
            .map(|id| {
                let root = root.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let index = SessionIndex {
                        active: Some(format!("writer-{id}")),
                        sessions: Vec::new(),
                    };
                    barrier.wait();
                    index.save(&root)
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        let saved = SessionIndex::load(&root).unwrap();
        assert!(saved.active.unwrap().starts_with("writer-"));
        assert!(
            fs::read_dir(root.join("sessions"))
                .unwrap()
                .all(|entry| entry.unwrap().file_name() == "index.json")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_sessions_without_workspace_remain_unassigned() {
        let json = br#"{
            "active": "s-legacy",
            "sessions": [{
                "id": "s-legacy",
                "title": "old session",
                "updated_at": 1,
                "lines": [],
                "titled": false
            }]
        }"#;
        let index: SessionIndex = serde_json::from_slice(json).unwrap();
        assert!(index.sessions[0].workspace.is_none());
    }

    #[test]
    fn workspace_roundtrips_with_session_index() {
        let root = std::env::temp_dir().join(format!(
            "flowy-index-workspace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = AgentWorkspace {
            project_id: "project-1".into(),
            root: root.join("project-root"),
        };
        SessionIndex {
            active: Some("s-1".into()),
            sessions: vec![SessionRecord {
                execution: flowy_agent_core::ExecutionMetadata::default(),
                id: "s-1".into(),
                title: "task".into(),
                updated_at: 1,
                lines: Vec::new(),
                status: TurnStatus::Idle,
                titled: false,
                workspace: Some(workspace.clone()),
                origin: SessionOrigin::default(),
                session_workspace: None,
                pinned: true,
                archived: false,
                ..SessionRecord::default()
            }],
        }
        .save(&root)
        .unwrap();

        let loaded = SessionIndex::load(&root).unwrap();
        assert_eq!(loaded.sessions[0].workspace.as_ref(), Some(&workspace));
        assert!(loaded.sessions[0].pinned);
        assert!(!loaded.sessions[0].archived);
        fs::remove_dir_all(root).unwrap();
    }
}

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::projection::{ChatLine, TurnStatus};

#[derive(Clone, Debug, Serialize, Deserialize)]
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

    #[must_use]
    pub fn load(root: &Path) -> Self {
        let path = Self::path(root);
        let Ok(bytes) = fs::read(&path) else {
            return Self::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub fn save(&self, root: &Path) {
        let dir = root.join("sessions");
        let _ = fs::create_dir_all(&dir);
        let path = Self::path(root);
        let Ok(bytes) = serde_json::to_vec_pretty(self) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, &bytes).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }

    pub fn upsert(&mut self, record: SessionRecord) {
        if let Some(slot) = self.sessions.iter_mut().find(|row| row.id == record.id) {
            *slot = record;
        } else {
            self.sessions.push(record);
        }
        self.sessions
            .sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SessionRecord> {
        self.sessions.iter_mut().find(|row| row.id == id)
    }

    pub fn get(&self, id: &str) -> Option<&SessionRecord> {
        self.sessions.iter().find(|row| row.id == id)
    }
}

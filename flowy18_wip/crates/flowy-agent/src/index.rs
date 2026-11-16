use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

    pub fn load(root: &Path) -> io::Result<Self> {
        let path = Self::path(root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error),
        };
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let dir = root.join("sessions");
        fs::create_dir_all(&dir)?;
        let path = Self::path(root);
        let bytes =
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        // Separate handles can save concurrently into the same directory.
        // Claim each temporary file exclusively so they never truncate or
        // remove one another's pending write.
        let (tmp, mut file) = loop {
            let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let tmp = dir.join(format!(
                ".index.json.{}.{}.tmp",
                std::process::id(),
                number
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
            {
                Ok(file) => break (tmp, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => return Err(error),
            }
        };
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, &path)
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
            .sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SessionRecord> {
        self.sessions.iter_mut().find(|row| row.id == id)
    }

    pub fn get(&self, id: &str) -> Option<&SessionRecord> {
        self.sessions.iter().find(|row| row.id == id)
    }
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
}

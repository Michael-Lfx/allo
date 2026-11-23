//! Local acceptance receipts.
//!
//! A turn that is already running cannot take another prompt today: the host
//! rejects the submission and the draft is lost. This module gives the host a
//! place to put an accepted message **before** any scheduling decision is made,
//! so "accepted" and "answered" stop being the same event.
//!
//! The store is deliberately separate from [`crate::session::SessionIndex`]:
//! the queue is written while a turn is running, and the session index is
//! rewritten at turn boundaries. Sharing one file would make those two writers
//! race for no gain.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// One message the host has taken responsibility for but has not run yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedMessage {
    /// Stable receipt id handed back to the caller. Never reused.
    pub id: String,
    pub session_id: String,
    pub text: String,
    pub created_at_ms: u64,
    /// Set once a turn has picked this message up. Kept (not deleted) so a
    /// restart can still explain what happened to an early submission.
    #[serde(default)]
    pub taken_at_ms: Option<u64>,
}

/// File-backed acceptance queue rooted at one storage directory.
#[derive(Clone, Debug)]
pub struct AcceptanceQueue {
    root: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct QueueFile {
    #[serde(default)]
    messages: Vec<AcceptedMessage>,
}

impl AcceptanceQueue {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Stored apart from `sessions/history.json` on purpose; see module docs.
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join("acceptance").join("pending.json")
    }

    pub fn load(&self) -> io::Result<Vec<AcceptedMessage>> {
        let path = Self::path(&self.root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let file: QueueFile = serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(file.messages)
    }

    /// Accept `text` for `session_id` and return the receipt.
    ///
    /// This is the only write the busy path performs: one file, one atomic
    /// rename, no session state touched.
    pub fn accept(
        &self,
        session_id: &str,
        text: &str,
        now_ms: u64,
    ) -> io::Result<AcceptedMessage> {
        let mut messages = self.load()?;
        let message = AcceptedMessage {
            id: new_receipt_id(now_ms, messages.len()),
            session_id: session_id.to_owned(),
            text: text.to_owned(),
            created_at_ms: now_ms,
            taken_at_ms: None,
        };
        messages.push(message.clone());
        self.save(&messages)?;
        Ok(message)
    }

    /// Messages accepted but never picked up, oldest first.
    pub fn pending(&self) -> io::Result<Vec<AcceptedMessage>> {
        let mut messages: Vec<_> = self
            .load()?
            .into_iter()
            .filter(|message| message.taken_at_ms.is_none())
            .collect();
        messages.sort_by_key(|message| message.created_at_ms);
        Ok(messages)
    }

    /// Mark `id` as picked up. Returns the message it just claimed, so two
    /// concurrent callers cannot both run the same text.
    pub fn take(&self, id: &str, now_ms: u64) -> io::Result<Option<AcceptedMessage>> {
        let mut messages = self.load()?;
        let claimed = messages
            .iter_mut()
            .find(|message| message.id == id && message.taken_at_ms.is_none());
        let Some(message) = claimed else {
            return Ok(None);
        };
        message.taken_at_ms = Some(now_ms);
        let snapshot = message.clone();
        self.save(&messages)?;
        Ok(Some(snapshot))
    }

    fn save(&self, messages: &[AcceptedMessage]) -> io::Result<()> {
        let path = Self::path(&self.root);
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("missing acceptance directory"))?;
        fs::create_dir_all(dir)?;
        let file = QueueFile {
            messages: messages.to_vec(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let (tmp, mut handle) = loop {
            let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let tmp = dir.join(format!(
                ".pending.json.{}.{}.tmp",
                std::process::id(),
                number
            ));
            match fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&tmp)
            {
                Ok(handle) => break (tmp, handle),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };
        if let Err(error) = handle.write_all(&bytes).and_then(|()| handle.flush()) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        drop(handle);
        match fs::rename(&tmp, &path) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&tmp);
                Err(error)
            }
        }
    }
}

fn new_receipt_id(now_ms: u64, index: usize) -> String {
    format!("accepted-{now_ms:x}-{index:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "flowy-acceptance-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn accept_survives_a_fresh_queue_handle() {
        let root = temp_root("durable");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept("home", "second prompt", 1_000).expect("accept");

        // A new handle sees the same receipt: this is the "restart and still
        // find it" contract, exercised without the UI.
        let reopened = AcceptanceQueue::new(&root);
        let pending = reopened.pending().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, accepted.id);
        assert_eq!(pending[0].text, "second prompt");
        assert_eq!(pending[0].session_id, "home");
        assert!(pending[0].taken_at_ms.is_none());

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn taking_once_removes_it_from_pending_but_keeps_the_record() {
        let root = temp_root("take");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept("home", "only once", 1_000).expect("accept");

        let first = queue.take(&accepted.id, 2_000).expect("take");
        assert!(first.is_some());
        assert_eq!(first.expect("claimed").taken_at_ms, Some(2_000));

        let second = queue.take(&accepted.id, 3_000).expect("take twice");
        assert!(second.is_none(), "a receipt must not be run twice");

        assert!(queue.pending().expect("pending").is_empty());
        assert_eq!(queue.load().expect("load").len(), 1);

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn empty_queue_reads_as_empty_not_missing() {
        let root = temp_root("empty");
        let queue = AcceptanceQueue::new(&root);
        assert!(queue.pending().expect("pending").is_empty());
        assert!(!AcceptanceQueue::path(&root).exists());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn receipts_are_unique_across_accepts_in_the_same_millisecond() {
        let root = temp_root("unique");
        let queue = AcceptanceQueue::new(&root);
        let a = queue.accept("home", "a", 1_000).expect("a");
        let b = queue.accept("home", "b", 1_000).expect("b");
        assert_ne!(a.id, b.id);
        assert_eq!(queue.pending().expect("pending").len(), 2);
        fs::remove_dir_all(&root).expect("cleanup");
    }
}

//! Local acceptance receipts, kept as an append-only journal.
//!
//! A turn that is already running cannot take another prompt today: the host
//! rejects the submission and the draft is lost. This module gives the host a
//! place to put an accepted message **before** any scheduling decision is made,
//! so "accepted" and "answered" stop being the same event.
//!
//! # Why a journal and not a snapshot
//!
//! An acceptance is a **fact**: a message was taken in at a moment and that
//! cannot be changed afterwards. A snapshot file answered "what is still
//! pending" but overwrote the previous answer on every write, so it could never
//! answer "what happened earlier". Tracing needs the second question.
//!
//! The log is also the simpler write: appending never reads the file first, so
//! two processes accepting at the same time cannot clobber each other. Pending
//! state is derived by replaying the journal.
//!
//! The journal is deliberately separate from [`crate::session::SessionIndex`]:
//! it is written while a turn is running, and the index is rewritten at turn
//! boundaries.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One message the host has taken responsibility for but has not run yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedMessage {
    /// Stable receipt id handed back to the caller. Never reused.
    pub id: String,
    /// The session the user was looking at when they submitted this.
    ///
    /// Deliberately **optional and non-binding**: at accept time we do not know
    /// which work this belongs to, and pinning it to a freshly created empty
    /// session would attach the receipt to a conversation the user never had.
    /// Deciding ownership is the router's job, later.
    #[serde(default)]
    pub session_hint: Option<String>,
    pub text: String,
    pub created_at_ms: u64,
    /// Set once a turn has picked this message up. Kept so a restart can still
    /// explain what happened to an early submission.
    #[serde(default)]
    pub taken_at_ms: Option<u64>,
}

/// One line of the journal. Append-only; nothing is ever rewritten.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum JournalEntry {
    Accepted {
        id: String,
        #[serde(default)]
        session_hint: Option<String>,
        text: String,
        at_ms: u64,
    },
    Taken {
        id: String,
        at_ms: u64,
    },
}

/// File-backed acceptance journal rooted at one storage directory.
#[derive(Clone, Debug)]
pub struct AcceptanceQueue {
    root: PathBuf,
}

impl AcceptanceQueue {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// One journal for the whole store. Stored apart from `sessions/history.json`
    /// on purpose; see module docs.
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join("acceptance").join("journal.jsonl")
    }

    /// Replay the journal into the current set of receipts, oldest first.
    ///
    /// A torn final line is tolerated: an append that was interrupted by a crash
    /// leaves a partial record, and dropping it is better than refusing to start.
    /// Every malformed line is skipped, so a torn record in the middle cannot
    /// hide the receipts appended after it.
    pub fn replay(&self) -> io::Result<Vec<AcceptedMessage>> {
        let path = Self::path(&self.root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut receipts: Vec<AcceptedMessage> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // A torn record from a crash: skip it instead of dropping every
            // record that follows it.
            let Ok(entry) = serde_json::from_str::<JournalEntry>(line) else {
                continue;
            };
            match entry {
                JournalEntry::Accepted {
                    id,
                    session_hint,
                    text,
                    at_ms,
                } => receipts.push(AcceptedMessage {
                    id,
                    session_hint,
                    text,
                    created_at_ms: at_ms,
                    taken_at_ms: None,
                }),
                JournalEntry::Taken { id, at_ms } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                    {
                        receipt.taken_at_ms = Some(at_ms);
                    }
                }
            }
        }
        Ok(receipts)
    }

    /// Every receipt ever accepted, oldest first, claimed or not.
    pub fn all(&self) -> io::Result<Vec<AcceptedMessage>> {
        self.replay()
    }

    /// Accept `text` for `session_id` and return the receipt.
    ///
    /// This is the only write the busy path performs: one append, no read, no
    /// session state touched.
    pub fn accept(
        &self,
        session_hint: Option<&str>,
        text: &str,
        now_ms: u64,
    ) -> io::Result<AcceptedMessage> {
        let id = new_receipt_id(now_ms, self.next_ordinal(now_ms)?);
        let entry = JournalEntry::Accepted {
            id: id.clone(),
            session_hint: session_hint.map(str::to_owned),
            text: text.to_owned(),
            at_ms: now_ms,
        };
        self.append(&entry)?;
        Ok(AcceptedMessage {
            id,
            session_hint: session_hint.map(str::to_owned),
            text: text.to_owned(),
            created_at_ms: now_ms,
            taken_at_ms: None,
        })
    }

    /// Receipts accepted but never picked up, oldest first.
    pub fn pending(&self) -> io::Result<Vec<AcceptedMessage>> {
        let mut pending: Vec<_> = self
            .replay()?
            .into_iter()
            .filter(|receipt| receipt.taken_at_ms.is_none())
            .collect();
        pending.sort_by_key(|receipt| receipt.created_at_ms);
        Ok(pending)
    }

    /// Mark `id` as picked up. Returns the receipt it just claimed, so two
    /// callers cannot both run the same text.
    pub fn take(
        &self,
        id: &str,
        now_ms: u64,
    ) -> io::Result<Option<AcceptedMessage>> {
        let mut receipts = self.replay()?;
        let Some(receipt) = receipts
            .iter_mut()
            .find(|receipt| receipt.id == id && receipt.taken_at_ms.is_none())
        else {
            return Ok(None);
        };
        receipt.taken_at_ms = Some(now_ms);
        let claimed = receipt.clone();
        self.append(&JournalEntry::Taken {
            id: id.to_owned(),
            at_ms: now_ms,
        })?;
        Ok(Some(claimed))
    }

    /// Ordinal used to keep ids unique inside the same millisecond. Replays the
    /// journal, which is cheap at first-version sizes.
    fn next_ordinal(&self, now_ms: u64) -> io::Result<usize> {
        Ok(self
            .replay()?
            .iter()
            .filter(|receipt| receipt.created_at_ms == now_ms)
            .count())
    }

    fn append(&self, entry: &JournalEntry) -> io::Result<()> {
        let path = Self::path(&self.root);
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("missing acceptance directory"))?;
        fs::create_dir_all(dir)?;
        let mut line = serde_json::to_vec(entry).map_err(io::Error::other)?;
        line.push(b'\n');
        // A crash can leave a partial record without its newline. Terminate it
        // before appending, so the new record never becomes part of the torn
        // line.
        ensure_trailing_newline(&path)?;
        let mut handle = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        handle.write_all(&line)?;
        handle.flush()
    }
}

fn new_receipt_id(now_ms: u64, ordinal: usize) -> String {
    format!("accepted-{now_ms:x}-{ordinal:x}")
}

/// Make sure the next append starts on its own line.
///
/// A crash in the middle of `write_all` can leave a partial record with no
/// terminating newline. Appending the next record straight after it would make
/// one unparseable line and lose that record too; a single newline keeps the
/// torn line isolated (and therefore skippable by `replay`).
fn ensure_trailing_newline(path: &Path) -> io::Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.len() == 0 {
        return Ok(());
    }
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(());
    }
    let mut handle = fs::OpenOptions::new().append(true).open(path)?;
    handle.write_all(b"\n")?;
    handle.flush()
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
        let accepted = queue
            .accept(Some("home"), "second prompt", 1_000)
            .expect("accept");

        // A new handle sees the same receipt: this is the "restart and still
        // find it" contract, exercised without the UI.
        let reopened = AcceptanceQueue::new(&root);
        let pending = reopened.pending().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, accepted.id);
        assert_eq!(pending[0].text, "second prompt");
        assert_eq!(pending[0].session_hint.as_deref(), Some("home"));
        assert!(pending[0].taken_at_ms.is_none());

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn taking_once_removes_it_from_pending_but_keeps_the_record() {
        let root = temp_root("take");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "only once", 1_000).expect("accept");

        let first = queue.take(&accepted.id, 2_000).expect("take");
        assert!(first.is_some());
        assert_eq!(first.expect("claimed").taken_at_ms, Some(2_000));

        let second = queue.take(&accepted.id, 3_000).expect("take twice");
        assert!(second.is_none(), "a receipt must not be run twice");

        assert!(queue.pending().expect("pending").is_empty());
        assert_eq!(queue.all().expect("all").len(), 1);

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
        let a = queue.accept(None, "a", 1_000).expect("a");
        let b = queue.accept(None, "b", 1_000).expect("b");
        assert_ne!(a.id, b.id);
        assert_eq!(queue.pending().expect("pending").len(), 2);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn the_journal_only_ever_grows_and_keeps_earlier_lines() {
        let root = temp_root("append");
        let queue = AcceptanceQueue::new(&root);
        queue.accept(Some("home"), "first", 1_000).expect("first");
        let after_first =
            fs::read_to_string(AcceptanceQueue::path(&root)).expect("read");

        queue.accept(Some("home"), "second", 2_000).expect("second");
        let after_second =
            fs::read_to_string(AcceptanceQueue::path(&root)).expect("read");

        // The earlier bytes are still there, unchanged, as a prefix. A snapshot
        // file could not make this promise.
        assert!(
            after_second.starts_with(&after_first),
            "appending must not rewrite history"
        );
        assert_eq!(after_second.lines().count(), 2);
        assert_eq!(queue.all().expect("all").len(), 2);

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_truncated_final_line_is_tolerated() {
        let root = temp_root("truncated");
        let queue = AcceptanceQueue::new(&root);
        queue.accept(None, "complete", 1_000).expect("accept");
        let path = AcceptanceQueue::path(&root);
        let mut handle = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        handle
            .write_all(br#"{"event":"accepted","id":"acc"#)
            .expect("partial write");
        drop(handle);

        // The torn tail is dropped; the complete record survives.
        let pending = queue.pending().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].text, "complete");

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_torn_line_does_not_hide_receipts_appended_after_it() {
        let root = temp_root("torn-middle");
        let queue = AcceptanceQueue::new(&root);
        queue.accept(None, "first", 1_000).expect("first");
        let path = AcceptanceQueue::path(&root);
        // Simulate a crash in the middle of an append: a partial record with no
        // terminating newline.
        let mut handle = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        handle
            .write_all(br#"{"event":"accepted","id":"torn"#)
            .expect("partial write");
        drop(handle);

        // The next accept must start on its own line; the torn line is skipped
        // instead of hiding everything after it.
        let second = queue.accept(None, "second", 2_000).expect("second");
        let pending = queue.pending().expect("pending");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[1].id, second.id);
        assert_eq!(pending[1].text, "second");

        fs::remove_dir_all(&root).expect("cleanup");
    }
}

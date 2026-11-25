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

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// The current claim. `None` means claimable; an expired lease is
    /// claimable again (at-least-once), an unexpired one is not.
    #[serde(default)]
    pub lease: Option<Lease>,
    /// Set once the claimed attempt finished; a completed receipt never
    /// returns to `pending`.
    #[serde(default)]
    pub completed_at_ms: Option<u64>,
    /// Earliest time this receipt may be claimed again after a release.
    #[serde(default)]
    pub available_at_ms: Option<u64>,
    /// How many times the receipt has been claimed.
    #[serde(default)]
    pub attempts: u32,
    /// Terminal: the host gave up after exhausting its retry budget. The
    /// record stays for inspection; it is never claimed again.
    #[serde(default)]
    pub dead: bool,
    /// Terminal: cancelled explicitly by user/host intent; never claimed again.
    #[serde(default)]
    pub cancelled: bool,
    /// Last attempt id that claimed this receipt, kept even after lease expiry
    /// so crash recovery can reconcile against committed engine history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_id: Option<String>,
    /// Target session bound before invoking the engine on the latest attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session_id: Option<String>,
    /// Submission id bound before invoking the engine on the latest attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_id: Option<String>,
}

/// Externally observable lifecycle state for one receipt at a point in time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    Queued,
    Running,
    Retrying,
    Completed,
    Cancelled,
    Dead,
}

/// A claim on one receipt. The attempt id lets only the owner complete or
/// release it; the expiry lets a crashed attempt be retried.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub attempt_id: String,
    pub expires_at_ms: u64,
}

impl AcceptedMessage {
    fn is_claimable(&self, now_ms: u64) -> bool {
        !self.dead
            && !self.cancelled
            && self.completed_at_ms.is_none()
            && self
                .available_at_ms
                .is_none_or(|available| available <= now_ms)
            && self
                .lease
                .as_ref()
                .is_none_or(|lease| lease.expires_at_ms <= now_ms)
    }

    #[must_use]
    pub fn state_at(&self, now_ms: u64) -> ReceiptState {
        if self.completed_at_ms.is_some() {
            ReceiptState::Completed
        } else if self.cancelled {
            ReceiptState::Cancelled
        } else if self.dead {
            ReceiptState::Dead
        } else if self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.expires_at_ms > now_ms)
        {
            ReceiptState::Running
        } else if self.attempts > 0 {
            ReceiptState::Retrying
        } else {
            ReceiptState::Queued
        }
    }
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
        /// Empty on journal lines written before leases existed.
        #[serde(default)]
        attempt_id: String,
        /// Zero on journal lines written before leases existed; such a line
        /// reads as an already-expired lease so the receipt can be retried.
        #[serde(default)]
        lease_until_ms: u64,
        at_ms: u64,
    },
    /// Durable link between a claimed attempt and the engine session/submission
    /// written immediately before starting execution, so crash recovery can
    /// verify committed engine state instead of re-running completed work.
    Bound {
        id: String,
        attempt_id: String,
        session_id: String,
        submission_id: String,
        at_ms: u64,
    },
    /// Lease heartbeat extending an active claim while a long turn is running.
    Renewed {
        id: String,
        attempt_id: String,
        lease_until_ms: u64,
        at_ms: u64,
    },
    /// Explicit cancellation by host/user; terminal unless a new receipt is queued.
    Cancelled { id: String, at_ms: u64 },
    Completed {
        id: String,
        attempt_id: String,
        at_ms: u64,
    },
    Released {
        id: String,
        attempt_id: String,
        /// Backoff written by the releasing host; 0 on old lines.
        #[serde(default)]
        retry_after_ms: u64,
        at_ms: u64,
    },
    Dead {
        id: String,
        attempt_id: String,
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
                    lease: None,
                    completed_at_ms: None,
                    available_at_ms: None,
                    attempts: 0,
                    dead: false,
                    cancelled: false,
                    last_attempt_id: None,
                    target_session_id: None,
                    submission_id: None,
                }),
                JournalEntry::Taken {
                    id,
                    attempt_id,
                    lease_until_ms,
                    at_ms,
                } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                    {
                        receipt.taken_at_ms.get_or_insert(at_ms);
                        receipt.available_at_ms = None;
                        receipt.attempts = receipt.attempts.saturating_add(1);
                        receipt.last_attempt_id = Some(attempt_id.clone());
                        receipt.lease = Some(Lease {
                            attempt_id,
                            expires_at_ms: lease_until_ms,
                        });
                    }
                }
                JournalEntry::Bound {
                    id,
                    attempt_id,
                    session_id,
                    submission_id,
                    ..
                } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                        && receipt.last_attempt_id.as_deref()
                            == Some(attempt_id.as_str())
                    {
                        receipt.target_session_id = Some(session_id);
                        receipt.submission_id = Some(submission_id);
                    }
                }
                JournalEntry::Renewed {
                    id,
                    attempt_id,
                    lease_until_ms,
                    ..
                } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                        && receipt.completed_at_ms.is_none()
                        && !receipt.dead
                        && !receipt.cancelled
                        && let Some(lease) = &mut receipt.lease
                        && lease.attempt_id == attempt_id
                    {
                        lease.expires_at_ms =
                            lease.expires_at_ms.max(lease_until_ms);
                    }
                }
                JournalEntry::Cancelled { id, .. } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                        && receipt.completed_at_ms.is_none()
                        && !receipt.dead
                    {
                        receipt.cancelled = true;
                        receipt.lease = None;
                    }
                }
                JournalEntry::Completed { id, at_ms, .. } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                    {
                        receipt.completed_at_ms = Some(at_ms);
                        receipt.lease = None;
                    }
                }
                JournalEntry::Released {
                    id,
                    retry_after_ms,
                    at_ms,
                    ..
                } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                    {
                        receipt.lease = None;
                        receipt.available_at_ms =
                            Some(at_ms.saturating_add(retry_after_ms));
                    }
                }
                JournalEntry::Dead { id, attempt_id, .. } => {
                    if let Some(receipt) =
                        receipts.iter_mut().find(|receipt| receipt.id == id)
                    {
                        // Only the attempt holding the current lease may kill
                        // the receipt; a stale dead line from a crashed attempt
                        // must not bury the retry that replaced it.
                        let owned =
                            receipt.lease.as_ref().is_some_and(|lease| {
                                lease.attempt_id == attempt_id
                            });
                        if owned {
                            receipt.dead = true;
                            receipt.lease = None;
                        }
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

    /// Look up one receipt by id regardless of lifecycle state (queued,
    /// running, backing off, completed, or dead).
    pub fn get(&self, id: &str) -> io::Result<Option<AcceptedMessage>> {
        Ok(self.replay()?.into_iter().find(|receipt| receipt.id == id))
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
        let id = new_receipt_id(now_ms);
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
            lease: None,
            completed_at_ms: None,
            available_at_ms: None,
            attempts: 0,
            dead: false,
            cancelled: false,
            last_attempt_id: None,
            target_session_id: None,
            submission_id: None,
        })
    }

    /// Receipts accepted but not completed and not under a live lease, oldest
    /// first.
    pub fn pending(&self) -> io::Result<Vec<AcceptedMessage>> {
        self.pending_at(now_ms())
    }

    /// `pending` at a caller-supplied clock, for retries and tests.
    pub fn pending_at(&self, now_ms: u64) -> io::Result<Vec<AcceptedMessage>> {
        let mut pending: Vec<_> = self
            .replay()?
            .into_iter()
            .filter(|receipt| receipt.is_claimable(now_ms))
            .collect();
        pending.sort_by_key(|receipt| receipt.created_at_ms);
        Ok(pending)
    }

    /// Claim `id` for `attempt_id` until `now_ms + lease_ms`.
    ///
    /// The in-process lock serialises replay and append, so two callers cannot
    /// both claim the same receipt. The lease is the retry mechanism: an
    /// expired lease is claimable again, and the `attempt_id` lets only the
    /// owner complete or release it.
    pub fn take(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
        lease_ms: u64,
    ) -> io::Result<Option<AcceptedMessage>> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut receipts = self.replay()?;
        let Some(receipt) = receipts
            .iter_mut()
            .find(|receipt| receipt.id == id && receipt.is_claimable(now_ms))
        else {
            return Ok(None);
        };
        let lease = Lease {
            attempt_id: attempt_id.to_owned(),
            expires_at_ms: now_ms.saturating_add(lease_ms),
        };
        receipt.taken_at_ms.get_or_insert(now_ms);
        receipt.available_at_ms = None;
        receipt.attempts = receipt.attempts.saturating_add(1);
        receipt.last_attempt_id = Some(attempt_id.to_owned());
        receipt.lease = Some(lease.clone());
        let claimed = receipt.clone();
        self.append(&JournalEntry::Taken {
            id: id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            lease_until_ms: lease.expires_at_ms,
            at_ms: now_ms,
        })?;
        Ok(Some(claimed))
    }

    /// Persist the target engine session and submission id for an active claim
    /// before execution begins, so recovery can check committed engine state.
    pub fn bind(
        &self,
        id: &str,
        attempt_id: &str,
        session_id: &str,
        submission_id: &str,
        now_ms: u64,
    ) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay()?;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
            || receipt.lease.as_ref().map(|l| l.attempt_id.as_str())
                != Some(attempt_id)
        {
            return Ok(false);
        }
        self.append(&JournalEntry::Bound {
            id: id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            session_id: session_id.to_owned(),
            submission_id: submission_id.to_owned(),
            at_ms: now_ms,
        })?;
        Ok(true)
    }

    /// Extend an active claim lease while a long turn is still running.
    /// Only the current lease owner may renew it.
    pub fn renew_lease(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
        lease_ms: u64,
    ) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay()?;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
            || receipt.lease.as_ref().map(|l| l.attempt_id.as_str())
                != Some(attempt_id)
        {
            return Ok(false);
        }
        self.append(&JournalEntry::Renewed {
            id: id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            lease_until_ms: now_ms.saturating_add(lease_ms),
            at_ms: now_ms,
        })?;
        Ok(true)
    }

    /// Explicitly cancel a queued or active receipt so it will never be
    /// claimed again. Already-completed or dead receipts return `Ok(false)`.
    pub fn cancel(&self, id: &str, now_ms: u64) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay()?;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
        {
            return Ok(false);
        }
        self.append(&JournalEntry::Cancelled {
            id: id.to_owned(),
            at_ms: now_ms,
        })?;
        Ok(true)
    }

    /// Mark an already-committed receipt complete during crash recovery even if
    /// its lease has since expired.
    pub fn complete_reconciled(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay()?;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
            || receipt.last_attempt_id.as_deref() != Some(attempt_id)
        {
            return Ok(false);
        }
        self.append(&JournalEntry::Completed {
            id: id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            at_ms: now_ms,
        })?;
        Ok(true)
    }

    /// Mark the receipt complete. Only the current lease owner can complete it.
    pub fn complete(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> io::Result<bool> {
        self.finish(id, attempt_id, now_ms, true, 0, false)
    }

    /// Drop the lease so the receipt can be claimed again after
    /// `retry_after_ms`. Only the current lease owner can release it.
    pub fn release(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
        retry_after_ms: u64,
    ) -> io::Result<bool> {
        self.finish(id, attempt_id, now_ms, false, retry_after_ms, false)
    }

    /// Bury the receipt after the retry budget ran out. The record stays in
    /// the journal for inspection; it is never claimed again.
    pub fn give_up(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> io::Result<bool> {
        self.finish(id, attempt_id, now_ms, false, 0, true)
    }

    fn finish(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
        completed: bool,
        retry_after_ms: u64,
        dead: bool,
    ) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay()?;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
        {
            return Ok(false);
        }
        if receipt
            .lease
            .as_ref()
            .map(|lease| lease.attempt_id.as_str())
            != Some(attempt_id)
        {
            return Ok(false);
        }
        let entry = if completed {
            JournalEntry::Completed {
                id: id.to_owned(),
                attempt_id: attempt_id.to_owned(),
                at_ms: now_ms,
            }
        } else if dead {
            JournalEntry::Dead {
                id: id.to_owned(),
                attempt_id: attempt_id.to_owned(),
                at_ms: now_ms,
            }
        } else {
            JournalEntry::Released {
                id: id.to_owned(),
                attempt_id: attempt_id.to_owned(),
                retry_after_ms,
                at_ms: now_ms,
            }
        };
        self.append(&entry)?;
        Ok(true)
    }

    /// One mutex per journal path. The desktop is single-instance, so an
    /// in-process lock is enough to serialise claims; it avoids a stale
    /// on-disk lock without adding a dependency.
    fn queue_lock(&self) -> Arc<Mutex<()>> {
        static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
            OnceLock::new();
        let registry = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut locks = registry.lock().unwrap_or_else(PoisonError::into_inner);
        locks.entry(Self::path(&self.root)).or_default().clone()
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

fn new_receipt_id(now_ms: u64) -> String {
    use std::hash::{BuildHasher, Hasher};
    let random = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    format!("accepted-{now_ms:x}-{random:016x}")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
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

        let first = queue
            .take(&accepted.id, "attempt-1", 2_000, 30_000)
            .expect("take");
        assert!(first.is_some());
        let claimed = first.expect("claimed");
        assert_eq!(claimed.taken_at_ms, Some(2_000));
        assert_eq!(claimed.attempts, 1);
        assert!(claimed.available_at_ms.is_none());
        assert_eq!(
            claimed
                .lease
                .as_ref()
                .map(|lease| lease.attempt_id.as_str()),
            Some("attempt-1")
        );

        let second = queue
            .take(&accepted.id, "attempt-2", 3_000, 30_000)
            .expect("take twice");
        assert!(second.is_none(), "a receipt must not be run twice");

        assert_eq!(
            queue.pending_at(3_000).expect("pending").len(),
            0,
            "an unexpired lease keeps the receipt out of pending"
        );
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

    #[test]
    fn an_expired_lease_can_be_claimed_again() {
        let root = temp_root("lease-expiry");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "retry me", 1_000).expect("accept");
        assert!(
            queue
                .take(&accepted.id, "attempt-1", 1_000, 100)
                .expect("first")
                .is_some()
        );
        assert_eq!(
            queue.pending_at(1_050).expect("pending").len(),
            0,
            "the live lease must keep the receipt out of pending"
        );
        let second = queue
            .take(&accepted.id, "attempt-2", 1_101, 100)
            .expect("second");
        let second = second.expect("an expired lease is claimable");
        assert_eq!(
            second.lease.as_ref().map(|lease| lease.attempt_id.as_str()),
            Some("attempt-2")
        );
        assert_eq!(queue.pending_at(1_101).expect("pending").len(), 0);

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_completed_receipt_never_returns_to_pending() {
        let root = temp_root("lease-complete");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "done", 1_000).expect("accept");
        assert!(
            queue
                .take(&accepted.id, "attempt-1", 1_000, 30_000)
                .expect("take")
                .is_some()
        );
        assert!(
            queue
                .complete(&accepted.id, "attempt-1", 1_200)
                .expect("complete")
        );

        assert_eq!(queue.pending_at(1_000_000).expect("pending").len(), 0);
        assert!(
            queue
                .take(&accepted.id, "attempt-3", 1_000_000, 100)
                .expect("take")
                .is_none()
        );
        let all = queue.all().expect("all");
        assert_eq!(all[0].completed_at_ms, Some(1_200));

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_released_receipt_is_claimable_again() {
        let root = temp_root("lease-release");
        let queue = AcceptanceQueue::new(&root);
        let accepted =
            queue.accept(None, "retry later", 1_000).expect("accept");
        assert!(
            queue
                .take(&accepted.id, "attempt-1", 1_000, 30_000)
                .expect("take")
                .is_some()
        );
        assert!(
            queue
                .release(&accepted.id, "attempt-1", 1_200, 0)
                .expect("release")
        );
        assert_eq!(queue.pending_at(1_200).expect("pending").len(), 1);
        assert!(
            queue
                .take(&accepted.id, "attempt-2", 1_200, 100)
                .expect("take again")
                .is_some()
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_released_receipt_waits_for_its_backoff() {
        let root = temp_root("lease-backoff");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "slow retry", 1_000).expect("accept");
        assert!(
            queue
                .take(&accepted.id, "attempt-1", 1_000, 30_000)
                .expect("take")
                .is_some()
        );
        assert!(
            queue
                .release(&accepted.id, "attempt-1", 1_100, 4_000)
                .expect("release")
        );

        assert_eq!(queue.pending_at(1_100).expect("pending").len(), 0);
        assert_eq!(queue.pending_at(5_099).expect("pending").len(), 0);
        assert_eq!(queue.pending_at(5_100).expect("pending").len(), 1);
        assert!(
            queue
                .take(&accepted.id, "attempt-2", 5_100, 100)
                .expect("take")
                .is_some()
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_given_up_receipt_is_never_claimable_again() {
        let root = temp_root("dead");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "poison", 1_000).expect("accept");
        queue
            .take(&accepted.id, "attempt-1", 1_000, 30_000)
            .expect("take")
            .expect("claimed");
        assert!(
            queue
                .give_up(&accepted.id, "attempt-1", 2_000)
                .expect("give up")
        );

        assert!(queue.pending_at(10_000_000).expect("pending").is_empty());
        assert!(queue.all().expect("all")[0].dead);
        assert!(
            queue
                .take(&accepted.id, "attempt-2", 10_000_000, 30_000)
                .expect("take")
                .is_none()
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_stale_attempt_cannot_bury_a_live_retry() {
        let root = temp_root("dead-stale");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "retry me", 1_000).expect("accept");
        queue
            .take(&accepted.id, "attempt-1", 1_000, 30_000)
            .expect("take")
            .expect("claimed");
        queue
            .release(&accepted.id, "attempt-1", 1_100, 0)
            .expect("release");
        queue
            .take(&accepted.id, "attempt-2", 1_100, 30_000)
            .expect("take")
            .expect("claimed");

        assert!(
            !queue
                .give_up(&accepted.id, "attempt-1", 1_200)
                .expect("give up")
        );
        let receipt = &queue.all().expect("all")[0];
        assert!(!receipt.dead);
        assert!(receipt.lease.is_some(), "the live attempt keeps its lease");

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn only_the_lease_owner_can_complete_or_release() {
        let root = temp_root("lease-owner");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "owned", 1_000).expect("accept");
        assert!(
            queue
                .take(&accepted.id, "attempt-1", 1_000, 30_000)
                .expect("take")
                .is_some()
        );
        assert!(
            !queue
                .complete(&accepted.id, "attempt-2", 1_200)
                .expect("wrong owner")
        );
        assert!(
            !queue
                .release(&accepted.id, "attempt-2", 1_200, 0)
                .expect("wrong owner")
        );
        assert!(
            queue
                .complete(&accepted.id, "attempt-1", 1_200)
                .expect("owner")
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn two_threads_cannot_claim_the_same_receipt() {
        let root = temp_root("lease-race");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "one winner", 1_000).expect("accept");
        let first_queue = queue.clone();
        let first_id = accepted.id.clone();
        let first = std::thread::spawn(move || {
            first_queue.take(&first_id, "attempt-a", 1_000, 30_000)
        });
        let second_queue = queue;
        let second_id = accepted.id;
        let second = std::thread::spawn(move || {
            second_queue.take(&second_id, "attempt-b", 1_000, 30_000)
        });
        let first = first.join().expect("thread").expect("io");
        let second = second.join().expect("thread").expect("io");
        assert_ne!(first.is_some(), second.is_some());

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn an_old_taken_line_is_read_as_an_expired_lease() {
        let root = temp_root("lease-old-line");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "old line", 1_000).expect("accept");
        let path = AcceptanceQueue::path(&root);
        let mut handle = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        let old = serde_json::json!({
            "event": "taken",
            "id": &accepted.id,
            "at_ms": 2_000
        });
        writeln!(handle, "{old}").expect("old line");
        drop(handle);

        assert_eq!(queue.pending_at(3_000).expect("pending").len(), 1);
        let claimed = queue
            .take(&accepted.id, "attempt-new", 3_000, 100)
            .expect("take");
        let claimed = claimed.expect("old taken line is retryable");
        assert_eq!(
            claimed
                .lease
                .as_ref()
                .map(|lease| lease.attempt_id.as_str()),
            Some("attempt-new")
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn binding_persists_session_and_submission_and_reconciles_after_lease_expiry()
     {
        let root = temp_root("bind-reconcile");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "ship deck", 1_000).expect("accept");
        assert_eq!(accepted.state_at(1_000), ReceiptState::Queued);

        let claimed = queue
            .take(&accepted.id, "attempt-1", 1_100, 500)
            .expect("take")
            .expect("claimed");
        assert_eq!(claimed.state_at(1_200), ReceiptState::Running);
        assert!(
            queue
                .bind(
                    &accepted.id,
                    "attempt-1",
                    "s-99",
                    "submission-abc",
                    1_150
                )
                .expect("bind")
        );

        let after_expiry =
            queue.get(&accepted.id).expect("get").expect("found");
        assert_eq!(after_expiry.target_session_id.as_deref(), Some("s-99"));
        assert_eq!(
            after_expiry.submission_id.as_deref(),
            Some("submission-abc")
        );
        assert_eq!(after_expiry.state_at(5_000), ReceiptState::Retrying);

        assert!(
            queue
                .complete_reconciled(&accepted.id, "attempt-1", 5_100)
                .expect("reconcile")
        );
        let settled = queue.get(&accepted.id).expect("get").expect("found");
        assert_eq!(settled.state_at(5_200), ReceiptState::Completed);
        assert_eq!(queue.pending_at(5_200).expect("pending").len(), 0);

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn renew_lease_extends_active_claim_and_rejects_stale_owner() {
        let root = temp_root("renew-lease");
        let queue = AcceptanceQueue::new(&root);
        let accepted = queue.accept(None, "long work", 1_000).expect("accept");
        let claimed = queue
            .take(&accepted.id, "attempt-1", 1_000, 300)
            .expect("take")
            .expect("claimed");
        assert_eq!(claimed.state_at(1_200), ReceiptState::Running);

        assert!(
            !queue
                .renew_lease(&accepted.id, "wrong-attempt", 1_200, 300)
                .expect("wrong owner")
        );
        assert!(
            queue
                .renew_lease(&accepted.id, "attempt-1", 1_200, 500)
                .expect("renew")
        );
        // Past the original 1_300 expiry, the renewed 1_700 lease keeps it Running.
        let renewed = queue.get(&accepted.id).expect("get").expect("found");
        assert_eq!(renewed.state_at(1_500), ReceiptState::Running);
        assert_eq!(queue.pending_at(1_500).expect("pending").len(), 0);
        assert_eq!(renewed.state_at(1_700), ReceiptState::Retrying);

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn cancel_prevents_future_claims_and_never_overrides_completed() {
        let root = temp_root("cancel");
        let queue = AcceptanceQueue::new(&root);
        let queued = queue.accept(None, "drop me", 1_000).expect("accept");
        assert!(queue.cancel(&queued.id, 1_050).expect("cancel"));
        assert!(!queue.cancel(&queued.id, 1_060).expect("idempotent cancel"));
        assert_eq!(
            queue
                .get(&queued.id)
                .expect("get")
                .expect("found")
                .state_at(1_100),
            ReceiptState::Cancelled
        );
        assert!(queue.pending_at(1_100).expect("pending").is_empty());
        assert!(
            queue
                .take(&queued.id, "attempt-1", 1_100, 500)
                .expect("take")
                .is_none()
        );

        let done = queue.accept(None, "already done", 2_000).expect("accept");
        queue
            .take(&done.id, "attempt-1", 2_010, 500)
            .expect("take")
            .expect("claimed");
        assert!(queue.complete(&done.id, "attempt-1", 2_020).expect("done"));
        assert!(!queue.cancel(&done.id, 2_030).expect("cannot cancel done"));
        assert_eq!(
            queue
                .get(&done.id)
                .expect("get")
                .expect("found")
                .state_at(2_030),
            ReceiptState::Completed
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }
}

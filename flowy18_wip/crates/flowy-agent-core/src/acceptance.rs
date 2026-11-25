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
use std::io::{self, Write};
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
    /// Quarantined because interior journal corruption occurred after this
    /// receipt was already taken, so its terminal outcome is uncertain and it
    /// must not be silently re-dispatched.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub quarantined: bool,
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
    Quarantined,
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
            && !self.quarantined
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
        } else if self.quarantined {
            ReceiptState::Quarantined
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
    /// Dead receipt replaced by a freshly queued receipt via explicit retry.
    Superseded {
        id: String,
        replaced_by: String,
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
    /// A torn final line is tolerated: an append interrupted by a crash leaves
    /// a partial tail record that is dropped safely. An unparseable **interior**
    /// line (followed by newer valid lines) is treated as sealed-segment
    /// corruption: unsettled receipts that were already taken before the
    /// corrupt line are quarantined instead of being silently re-dispatched,
    /// while receipts never taken or appended afterwards remain usable.
    pub fn replay(&self) -> io::Result<Vec<AcceptedMessage>> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(self.replay_detailed_locked()?.0)
    }

    /// Whether the journal contains any corrupt interior (non-tail) lines.
    pub fn has_interior_corruption(&self) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(self.replay_detailed_locked()?.1 > 0)
    }

    fn replay_detailed_locked(
        &self,
    ) -> io::Result<(Vec<AcceptedMessage>, usize)> {
        let path = Self::path(&self.root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((Vec::new(), 0));
            }
            Err(error) => return Err(error),
        };
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let mut receipts: Vec<AcceptedMessage> = Vec::new();
        let mut index_by_id: HashMap<String, usize> = HashMap::new();
        let mut last_taken_line: HashMap<String, usize> = HashMap::new();
        let mut corrupt_interior = 0_usize;
        let mut last_interior_corrupt_line: Option<usize> = None;
        for (idx, line) in lines.iter().enumerate() {
            let Ok(entry) = serde_json::from_str::<JournalEntry>(line) else {
                if idx + 1 < lines.len() {
                    corrupt_interior = corrupt_interior.saturating_add(1);
                    last_interior_corrupt_line = Some(idx);
                }
                continue;
            };
            match entry {
                JournalEntry::Accepted {
                    id,
                    session_hint,
                    text,
                    at_ms,
                } => {
                    let pos = receipts.len();
                    index_by_id.entry(id.clone()).or_insert(pos);
                    receipts.push(AcceptedMessage {
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
                        quarantined: false,
                        last_attempt_id: None,
                        target_session_id: None,
                        submission_id: None,
                    });
                }
                JournalEntry::Taken {
                    id,
                    attempt_id,
                    lease_until_ms,
                    at_ms,
                } => {
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
                        last_taken_line.insert(id, idx);
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
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
                        if receipt.last_attempt_id.as_deref()
                            == Some(attempt_id.as_str())
                        {
                            receipt.target_session_id = Some(session_id);
                            receipt.submission_id = Some(submission_id);
                        }
                    }
                }
                JournalEntry::Renewed {
                    id,
                    attempt_id,
                    lease_until_ms,
                    ..
                } => {
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
                        if receipt.completed_at_ms.is_none()
                            && !receipt.dead
                            && !receipt.cancelled
                            && let Some(lease) = &mut receipt.lease
                            && lease.attempt_id == attempt_id
                        {
                            lease.expires_at_ms =
                                lease.expires_at_ms.max(lease_until_ms);
                        }
                    }
                }
                JournalEntry::Cancelled { id, .. } => {
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
                        if receipt.completed_at_ms.is_none() && !receipt.dead {
                            receipt.cancelled = true;
                            receipt.lease = None;
                        }
                    }
                }
                JournalEntry::Completed { id, at_ms, .. } => {
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
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
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
                        receipt.lease = None;
                        receipt.available_at_ms =
                            Some(at_ms.saturating_add(retry_after_ms));
                    }
                }
                JournalEntry::Dead { id, attempt_id, .. } => {
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
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
                JournalEntry::Superseded { id, .. } => {
                    if let Some(&pos) = index_by_id.get(&id) {
                        let receipt = &mut receipts[pos];
                        if receipt.completed_at_ms.is_none()
                            && receipt.dead
                            && !receipt.cancelled
                        {
                            receipt.cancelled = true;
                            receipt.lease = None;
                        }
                    }
                }
            }
        }
        if let Some(corrupt_idx) = last_interior_corrupt_line {
            tracing::error!(
                target: "flowy.acceptance",
                path = %path.display(),
                corrupt_interior,
                "interior journal corruption detected; quarantining pre-corruption in-flight receipts"
            );
            for receipt in &mut receipts {
                if receipt.completed_at_ms.is_none()
                    && !receipt.dead
                    && !receipt.cancelled
                    && last_taken_line
                        .get(&receipt.id)
                        .is_some_and(|&taken_idx| taken_idx < corrupt_idx)
                {
                    receipt.quarantined = true;
                    receipt.lease = None;
                }
            }
        }
        Ok((receipts, corrupt_interior))
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
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let id = new_receipt_id(now_ms);
        let entry = JournalEntry::Accepted {
            id: id.clone(),
            session_hint: session_hint.map(str::to_owned),
            text: text.to_owned(),
            at_ms: now_ms,
        };
        self.append_locked(&entry)?;
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
            quarantined: false,
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

    /// Receipts that remain active or need user visibility: every receipt that
    /// is neither `Completed` nor `Cancelled` (`Queued`, `Running`, `Retrying`
    /// including backoff windows, `Quarantined`, and `Dead`), sorted by
    /// `created_at_ms` ascending.
    pub fn visible(&self) -> io::Result<Vec<(AcceptedMessage, ReceiptState)>> {
        self.visible_at(now_ms())
    }

    /// `visible` at a caller-supplied clock, for deterministic tests.
    pub fn visible_at(
        &self,
        now_ms: u64,
    ) -> io::Result<Vec<(AcceptedMessage, ReceiptState)>> {
        let mut visible: Vec<(AcceptedMessage, ReceiptState)> = self
            .replay()?
            .into_iter()
            .filter_map(|receipt| {
                let state = receipt.state_at(now_ms);
                (!matches!(
                    state,
                    ReceiptState::Completed | ReceiptState::Cancelled
                ))
                .then_some((receipt, state))
            })
            .collect();
        visible.sort_by_key(|(receipt, _)| receipt.created_at_ms);
        Ok(visible)
    }

    /// Requeue a `Dead` receipt as a fresh receipt ID and atomically supersede
    /// the original dead record so it exits `visible` and cannot be retried
    /// twice across restarts or double-clicks.
    pub fn retry_dead(
        &self,
        id: &str,
        now_ms: u64,
    ) -> io::Result<Option<AcceptedMessage>> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay_detailed_locked()?.0;
        let Some(dead) = receipts.iter().find(|receipt| {
            receipt.id == id
                && receipt.dead
                && !receipt.cancelled
                && receipt.completed_at_ms.is_none()
        }) else {
            return Ok(None);
        };
        let mut new_id = new_receipt_id(now_ms);
        while new_id == id {
            new_id = new_receipt_id(now_ms);
        }
        let entries = [
            JournalEntry::Accepted {
                id: new_id.clone(),
                session_hint: dead.session_hint.clone(),
                text: dead.text.clone(),
                at_ms: now_ms,
            },
            JournalEntry::Superseded {
                id: id.to_owned(),
                replaced_by: new_id.clone(),
                at_ms: now_ms,
            },
        ];
        self.append_batch_locked(&entries)?;
        Ok(Some(AcceptedMessage {
            id: new_id,
            session_hint: dead.session_hint.clone(),
            text: dead.text.clone(),
            created_at_ms: now_ms,
            taken_at_ms: None,
            lease: None,
            completed_at_ms: None,
            available_at_ms: None,
            attempts: 0,
            dead: false,
            cancelled: false,
            quarantined: false,
            last_attempt_id: None,
            target_session_id: None,
            submission_id: None,
        }))
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
        let mut receipts = self.replay_detailed_locked()?.0;
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
        self.append_locked(&JournalEntry::Taken {
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
        let receipts = self.replay_detailed_locked()?.0;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
            || receipt.quarantined
            || receipt.lease.as_ref().map(|l| l.attempt_id.as_str())
                != Some(attempt_id)
        {
            return Ok(false);
        }
        self.append_locked(&JournalEntry::Bound {
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
        let receipts = self.replay_detailed_locked()?.0;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
            || receipt.quarantined
            || receipt.lease.as_ref().map(|l| l.attempt_id.as_str())
                != Some(attempt_id)
        {
            return Ok(false);
        }
        self.append_locked(&JournalEntry::Renewed {
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
        let receipts = self.replay_detailed_locked()?.0;
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
        self.append_locked(&JournalEntry::Cancelled {
            id: id.to_owned(),
            at_ms: now_ms,
        })?;
        Ok(true)
    }

    /// Mark an already-committed receipt complete during crash recovery even if
    /// its lease has since expired or it was quarantined by later corruption.
    pub fn complete_reconciled(
        &self,
        id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay_detailed_locked()?.0;
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
        self.append_locked(&JournalEntry::Completed {
            id: id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            at_ms: now_ms,
        })?;
        Ok(true)
    }

    /// Release any unexpired leases left behind by a crashed predecessor
    /// process on single-instance cold boot (with 0 ms backoff) so they become
    /// immediately retryable instead of falsely displaying `Running` until
    /// lease expiry.
    pub fn release_uncommitted_leases(
        &self,
        now_ms: u64,
    ) -> io::Result<Vec<String>> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay_detailed_locked()?.0;
        let mut entries = Vec::new();
        let mut released_ids = Vec::new();
        for receipt in receipts {
            if receipt.completed_at_ms.is_none()
                && !receipt.dead
                && !receipt.cancelled
                && !receipt.quarantined
                && let Some(lease) = &receipt.lease
                && lease.expires_at_ms > now_ms
            {
                released_ids.push(receipt.id.clone());
                entries.push(JournalEntry::Released {
                    id: receipt.id,
                    attempt_id: lease.attempt_id.clone(),
                    retry_after_ms: 0,
                    at_ms: now_ms,
                });
            }
        }
        if !entries.is_empty() {
            self.append_batch_locked(&entries)?;
        }
        Ok(released_ids)
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
        let receipts = self.replay_detailed_locked()?.0;
        let Some(receipt) = receipts.iter().find(|receipt| receipt.id == id)
        else {
            return Ok(false);
        };
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
            || receipt.quarantined
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
        self.append_locked(&entry)?;
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

    fn append_locked(&self, entry: &JournalEntry) -> io::Result<()> {
        self.append_batch_locked(std::slice::from_ref(entry))
    }

    fn append_batch_locked(&self, entries: &[JournalEntry]) -> io::Result<()> {
        let path = Self::path(&self.root);
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("missing acceptance directory"))?;
        fs::create_dir_all(dir)?;
        let mut buf = Vec::new();
        for entry in entries {
            let line = serde_json::to_vec(entry).map_err(io::Error::other)?;
            buf.extend_from_slice(&line);
            buf.push(b'\n');
        }
        // Repair a crash-truncated trailing line in place (truncate unparseable
        // partial bytes, or terminate a complete JSON record) so a torn tail
        // never gets promoted into an interior corrupt line on the next append.
        repair_torn_tail_locked(&path)?;
        let mut handle = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        handle.write_all(&buf)?;
        handle.flush()?;
        handle.sync_all()
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

/// Repair a crash-truncated trailing line before the next append (caller holds
/// `queue_lock`).
///
/// - If the file already ends with `\n` (or is empty / missing), does nothing.
/// - If the trailing non-newline fragment parses as a complete [`JournalEntry`],
///   appends `\n` so that valid entry is preserved.
/// - If the trailing fragment is unparseable (partial crash write), truncates
///   the file back to the end of the last complete newline-terminated line so
///   the torn bytes are never promoted into an interior corrupt line that would
///   quarantine earlier healthy receipts.
fn repair_torn_tail_locked(path: &Path) -> io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let len = file.seek(SeekFrom::End(0))?;
    if len == 0 {
        return Ok(());
    }
    // Fast path: check only the final byte so normal appends never read the
    // whole journal into memory.
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(());
    }
    drop(file);
    // Rare crash-recovery path: the final line lacks a trailing newline.
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(());
    }
    let cut = bytes
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |idx| idx + 1);
    let tail = String::from_utf8_lossy(&bytes[cut..]);
    let trimmed = tail.trim();
    if !trimmed.is_empty()
        && serde_json::from_str::<JournalEntry>(trimmed).is_ok()
    {
        let mut handle = fs::OpenOptions::new().append(true).open(path)?;
        handle.write_all(b"\n")?;
        handle.flush()?;
        handle.sync_all()
    } else {
        let handle = fs::OpenOptions::new().write(true).open(path)?;
        handle.set_len(cut as u64)?;
        handle.sync_all()
    }
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

    #[test]
    fn kernel_review_corrupt_middle_terminal_event_never_reclaims_completed_receipt()
     {
        let root = temp_root("corrupt-middle-terminal");
        let queue = AcceptanceQueue::new(&root);
        let first = queue.accept(None, "first work", 1_000).expect("accept a");
        assert!(
            queue
                .take(&first.id, "attempt-1", 1_010, 100)
                .expect("take a")
                .is_some()
        );
        assert!(
            queue
                .complete(&first.id, "attempt-1", 1_020)
                .expect("complete a")
        );
        let second =
            queue.accept(None, "second work", 2_000).expect("accept b");

        // Corrupt the interior `completed` line for `first` while keeping the
        // subsequent valid `accepted` line for `second`.
        let path = AcceptanceQueue::path(&root);
        let raw = fs::read_to_string(&path).expect("read journal");
        let mut mutated: Vec<String> = raw.lines().map(str::to_owned).collect();
        let completed_idx = mutated
            .iter()
            .position(|line| line.contains("\"completed\""))
            .expect("completed line");
        mutated[completed_idx] = r#"{"event":"completed","corrupt"#.to_owned();
        fs::write(&path, format!("{}\n", mutated.join("\n")))
            .expect("rewrite journal");

        assert!(
            queue.has_interior_corruption().expect("corruption flag"),
            "interior corrupt line must be flagged"
        );
        let first_after = queue.get(&first.id).expect("get a").expect("found");
        assert!(first_after.quarantined);
        assert_eq!(first_after.state_at(9_999), ReceiptState::Quarantined);
        assert!(
            queue
                .take(&first.id, "attempt-2", 9_999, 1_000)
                .expect("take a after corruption")
                .is_none(),
            "corrupted terminal line must never allow silent re-claim"
        );
        // Subsequent valid receipts remain claimable and completable.
        let pending = queue.pending_at(9_999).expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, second.id);
        assert!(
            queue
                .take(&second.id, "attempt-b", 9_999, 1_000)
                .expect("take b")
                .is_some()
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn visible_at_includes_backoff_running_dead_and_quarantined_in_order() {
        let root = temp_root("visible-states");
        let queue = AcceptanceQueue::new(&root);

        let r_done = queue.accept(None, "done", 1_000).expect("done");
        queue
            .take(&r_done.id, "a-done", 1_010, 500)
            .expect("take done")
            .expect("claimed");
        assert!(queue.complete(&r_done.id, "a-done", 1_020).expect("done"));

        let r_cancel = queue.accept(None, "cancel", 1_100).expect("cancel");
        assert!(queue.cancel(&r_cancel.id, 1_110).expect("cancel"));

        let r_backoff = queue.accept(None, "backoff", 1_200).expect("backoff");
        queue
            .take(&r_backoff.id, "a-back", 1_210, 500)
            .expect("take back")
            .expect("claimed");
        assert!(
            queue
                .release(&r_backoff.id, "a-back", 1_220, 60_000)
                .expect("release")
        );

        let r_dead = queue.accept(None, "dead", 1_300).expect("dead");
        queue
            .take(&r_dead.id, "a-dead", 1_310, 500)
            .expect("take dead")
            .expect("claimed");
        assert!(queue.give_up(&r_dead.id, "a-dead", 1_320).expect("dead"));

        let r_running = queue.accept(None, "running", 1_400).expect("running");
        queue
            .take(&r_running.id, "a-run", 1_410, 10_000)
            .expect("take run")
            .expect("claimed");

        let r_queued = queue.accept(None, "queued", 1_500).expect("queued");

        // During backoff (`now = 2_000`), `pending_at` hides backoff/dead/running,
        // while `visible_at` keeps every non-Completed, non-Cancelled item in
        // ascending `created_at_ms` order.
        let pending = queue.pending_at(2_000).expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, r_queued.id);

        let visible = queue.visible_at(2_000).expect("visible");
        let summary: Vec<(String, ReceiptState)> =
            visible.into_iter().map(|(m, s)| (m.id, s)).collect();
        assert_eq!(
            summary,
            vec![
                (r_backoff.id, ReceiptState::Retrying),
                (r_dead.id, ReceiptState::Dead),
                (r_running.id, ReceiptState::Running),
                (r_queued.id, ReceiptState::Queued),
            ]
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn retry_dead_supersedes_original_and_is_idempotent_across_reopen() {
        let root = temp_root("retry-dead-supersede");
        let queue = AcceptanceQueue::new(&root);

        let dead = queue
            .accept(Some("sess-1"), "deliver deck", 1_000)
            .expect("accept");
        queue
            .take(&dead.id, "a1", 1_010, 500)
            .expect("take")
            .expect("claimed");
        assert!(queue.give_up(&dead.id, "a1", 1_020).expect("give_up"));

        let requeued = queue
            .retry_dead(&dead.id, 2_000)
            .expect("retry_dead")
            .expect("requeued");
        assert_ne!(requeued.id, dead.id);
        assert_eq!(requeued.session_hint.as_deref(), Some("sess-1"));
        assert_eq!(requeued.text, "deliver deck");

        // Second retry on the already-superseded dead id must be rejected.
        assert!(
            queue
                .retry_dead(&dead.id, 2_010)
                .expect("second retry")
                .is_none()
        );

        // Fresh handle after simulated restart only sees the new queued receipt.
        let reopened = AcceptanceQueue::new(&root);
        let old = reopened.get(&dead.id).expect("get old").expect("present");
        assert!(old.dead);
        assert!(old.cancelled);
        assert_eq!(old.state_at(2_050), ReceiptState::Cancelled);

        let visible = reopened.visible_at(2_050).expect("visible");
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].0.id, requeued.id);
        assert_eq!(visible[0].1, ReceiptState::Queued);

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn torn_tail_repaired_in_place_never_quarantines_earlier_taken_receipt() {
        let root = temp_root("torn-tail-repair");
        let queue = AcceptanceQueue::new(&root);
        let first = queue.accept(None, "first", 1_000).expect("accept first");
        queue
            .take(&first.id, "att-1", 1_010, 5_000)
            .expect("take")
            .expect("claimed");
        // Append an incomplete non-newline crash tail.
        let path = AcceptanceQueue::path(&root);
        let mut handle = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        handle
            .write_all(br#"{"event":"renewed","id":"torn"#)
            .expect("torn write");
        drop(handle);

        // Next append must truncate the torn tail instead of promoting it into
        // an interior corrupt line that permanently quarantines `first`.
        let second =
            queue.accept(None, "second", 2_000).expect("accept second");
        assert!(!queue.has_interior_corruption().expect("corrupt flag"));
        let first_after = queue.get(&first.id).expect("get").expect("present");
        assert!(
            !first_after.quarantined,
            "repaired torn tail must not quarantine healthy taken receipt"
        );
        assert_eq!(first_after.state_at(2_000), ReceiptState::Running);
        assert_eq!(
            queue
                .get(&second.id)
                .expect("get second")
                .expect("present")
                .state_at(2_000),
            ReceiptState::Queued
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn release_uncommitted_leases_moves_dead_predecessor_running_to_retrying() {
        let root = temp_root("release-stale-leases");
        let queue = AcceptanceQueue::new(&root);
        let r = queue
            .accept(None, "crashed mid-turn", 1_000)
            .expect("accept");
        queue
            .take(&r.id, "att-dead-proc", 1_010, 30_000)
            .expect("take")
            .expect("claimed");
        assert_eq!(
            queue.get(&r.id).unwrap().unwrap().state_at(2_000),
            ReceiptState::Running
        );

        let released = queue
            .release_uncommitted_leases(2_000)
            .expect("release stale leases");
        assert_eq!(released, vec![r.id.clone()]);
        assert_eq!(
            queue.get(&r.id).unwrap().unwrap().state_at(2_000),
            ReceiptState::Retrying
        );
        assert_eq!(queue.pending_at(2_000).unwrap().len(), 1);
        fs::remove_dir_all(&root).expect("cleanup");
    }
}

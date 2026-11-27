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

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

const BOUNDARY_SLICE_BYTES: usize = 4096;
const MAX_CACHE_ENTRIES: usize = 64;
const COMPACT_MIN_LINES: usize = 512;
const COMPACT_MIN_BYTES: u64 = 256 * 1024;
const COMPACT_RETAIN_TERMINAL: usize = 200;

use serde::{Deserialize, Serialize};

use crate::session::{SessionOrigin, SessionRole, SessionWorkspace};

/// Captured conversation origin, workspace, and role intent for a cold-start
/// empty-draft receipt accepted before any live session exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftTarget {
    pub origin: SessionOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<SessionWorkspace>,
    pub role: SessionRole,
}

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
    pub lease: Option<Box<Lease>>,
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
    /// When `true`, `target_session_id` / `session_hint` is a strict Worker
    /// binding: if the target session was deleted or invalidated, the host
    /// MUST cancel this receipt instead of spawning a new Home session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict_target: bool,
    /// Optional Router dispatch request identifier for audit and reconciliation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_request_id: Option<String>,
    /// Captured cold-start origin, workspace, and role intent when accepted from
    /// an empty Home/Project draft (boxed to keep `AcceptedMessage` compact).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_target: Option<Box<DraftTarget>>,
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
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        strict_target: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft_target: Option<Box<DraftTarget>>,
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
        let raw_path = Self::path(&self.root);
        let cache_key = normalize_cache_path(&raw_path);
        let mut file = match fs::File::open(&raw_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                cache_invalidate(&cache_key);
                return Ok((Vec::new(), 0));
            }
            Err(error) => return Err(error),
        };
        let meta = file.metadata()?;
        let file_len = meta.len();
        let file_modified = meta.modified().ok();

        if let Some(mut cached) = cache_get(&cache_key)
            && file_len >= cached.parsed_len
            && verify_boundary_slices(
                &mut file,
                cached.parsed_len,
                &cached.head,
                &cached.tail,
            )?
        {
            if file_len == cached.parsed_len {
                if file_modified.is_some() && file_modified == cached.modified {
                    #[cfg(test)]
                    record_replay_stat(ReplayStatKind::CacheHit);
                    return Ok((cached.receipts, cached.corrupt_interior));
                }
            } else if !cached.has_unparseable {
                file.seek(SeekFrom::Start(cached.parsed_len))?;
                let mut tail_bytes = Vec::new();
                file.read_to_end(&mut tail_bytes)?;
                if !tail_bytes.is_empty() {
                    let complete_len = tail_bytes
                        .iter()
                        .rposition(|&b| b == b'\n')
                        .map_or(0, |idx| idx + 1);
                    let complete_slice = &tail_bytes[..complete_len];
                    let trailing_fragment = &tail_bytes[complete_len..];
                    let complete_text = String::from_utf8_lossy(complete_slice);
                    let mut parsed_entries = Vec::new();
                    let mut all_valid = true;
                    for line in complete_text
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                    {
                        if let Ok(entry) =
                            serde_json::from_str::<JournalEntry>(line)
                        {
                            parsed_entries.push(entry);
                        } else {
                            all_valid = false;
                            break;
                        }
                    }
                    if all_valid {
                        #[cfg(test)]
                        record_replay_stat(ReplayStatKind::TailParse);
                        if complete_len > 0 {
                            for entry in parsed_entries {
                                apply_journal_entry(
                                    &mut cached.receipts,
                                    &mut cached.index_by_id,
                                    entry,
                                );
                            }
                            cached.parsed_len = cached
                                .parsed_len
                                .saturating_add(complete_len as u64);
                            advance_boundary_slices(
                                &mut cached.head,
                                &mut cached.tail,
                                complete_slice,
                            );
                            cached.modified = if trailing_fragment.is_empty() {
                                file.metadata()
                                    .ok()
                                    .filter(|m| m.len() == cached.parsed_len)
                                    .and_then(|m| m.modified().ok())
                            } else {
                                None
                            };
                            cache_insert(cache_key, cached.clone());
                        }
                        let mut receipts = cached.receipts;
                        let mut index_by_id = cached.index_by_id;
                        if !trailing_fragment.is_empty() {
                            let trailing_text =
                                String::from_utf8_lossy(trailing_fragment);
                            let trimmed = trailing_text.trim();
                            if !trimmed.is_empty()
                                && let Ok(entry) =
                                    serde_json::from_str::<JournalEntry>(
                                        trimmed,
                                    )
                            {
                                apply_journal_entry(
                                    &mut receipts,
                                    &mut index_by_id,
                                    entry,
                                );
                            }
                        }
                        return Ok((receipts, cached.corrupt_interior));
                    }
                }
            }
        }

        // Fail-closed fallback: invalidate cache and replay from offset 0.
        #[cfg(test)]
        record_replay_stat(ReplayStatKind::FullReplay);
        cache_invalidate(&cache_key);
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let after_modified = file
            .metadata()
            .ok()
            .filter(|m| m.len() == bytes.len() as u64)
            .and_then(|m| m.modified().ok());
        let committed_len = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |idx| idx + 1);
        let trailing_fragment = &bytes[committed_len..];
        let trailing_non_empty =
            !String::from_utf8_lossy(trailing_fragment).trim().is_empty();

        let (
            mut receipts,
            mut index_by_id,
            last_taken_line,
            corrupt_interior,
            last_interior_corrupt_line,
            has_unparseable_committed,
        ) = replay_bytes(&bytes[..committed_len]);

        if trailing_non_empty && has_unparseable_committed {
            // An unparseable `\n`-terminated tail in the committed prefix
            // becomes interior corruption when followed by non-empty trailing
            // bytes; replay the whole buffer directly without caching.
            let (
                mut full_receipts,
                _,
                full_last_taken,
                full_corrupt,
                full_last_corrupt,
                _,
            ) = replay_bytes(&bytes);
            if let Some(corrupt_idx) = full_last_corrupt {
                quarantine_pre_corruption_receipts(
                    &raw_path,
                    &mut full_receipts,
                    &full_last_taken,
                    corrupt_idx,
                    full_corrupt,
                );
            }
            return Ok((full_receipts, full_corrupt));
        }

        if let Some(corrupt_idx) = last_interior_corrupt_line {
            quarantine_pre_corruption_receipts(
                &raw_path,
                &mut receipts,
                &last_taken_line,
                corrupt_idx,
                corrupt_interior,
            );
        }

        let committed_bytes = &bytes[..committed_len];
        let (head, tail) = compute_boundary_slices(committed_bytes);
        cache_insert(
            cache_key,
            AcceptanceCacheEntry {
                parsed_len: committed_len as u64,
                head,
                tail,
                modified: if trailing_non_empty || committed_len != bytes.len()
                {
                    None
                } else {
                    after_modified
                },
                has_unparseable: has_unparseable_committed,
                receipts: receipts.clone(),
                index_by_id: index_by_id.clone(),
                corrupt_interior,
                last_used_seq: 0,
            },
        );

        if trailing_non_empty {
            let trailing_text = String::from_utf8_lossy(trailing_fragment);
            let trimmed = trailing_text.trim();
            if let Ok(entry) = serde_json::from_str::<JournalEntry>(trimmed) {
                apply_journal_entry(&mut receipts, &mut index_by_id, entry);
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
        self.accept_inner(session_hint, text, false, None, None, now_ms)
    }

    /// Accept `text` with explicit cold-start draft intent (origin, workspace,
    /// role) or strict session targeting.
    pub fn accept_with_target(
        &self,
        session_hint: Option<&str>,
        text: &str,
        strict_target: bool,
        target: Option<DraftTarget>,
        dispatch_request_id: Option<&str>,
        now_ms: u64,
    ) -> io::Result<AcceptedMessage> {
        self.accept_inner(
            session_hint,
            text,
            strict_target,
            dispatch_request_id,
            target,
            now_ms,
        )
    }

    /// Accept `text` strictly bound to an existing Worker `worker_id`.
    ///
    /// Unlike [`Self::accept`], `strict_target = true` forbids the scheduler
    /// from falling back to a freshly spawned background Home session if
    /// `worker_id` is deleted before execution.
    pub fn accept_strict(
        &self,
        worker_id: &str,
        text: &str,
        dispatch_request_id: Option<&str>,
        now_ms: u64,
    ) -> io::Result<AcceptedMessage> {
        self.accept_inner(
            Some(worker_id),
            text,
            true,
            dispatch_request_id,
            None,
            now_ms,
        )
    }

    fn accept_inner(
        &self,
        session_hint: Option<&str>,
        text: &str,
        strict_target: bool,
        dispatch_request_id: Option<&str>,
        target: Option<DraftTarget>,
        now_ms: u64,
    ) -> io::Result<AcceptedMessage> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let id = new_receipt_id(now_ms);
        let hint = session_hint.map(str::to_owned);
        let request_id = dispatch_request_id.map(str::to_owned);
        let draft_target = target.map(Box::new);
        let entry = JournalEntry::Accepted {
            id: id.clone(),
            session_hint: hint.clone(),
            text: text.to_owned(),
            at_ms: now_ms,
            strict_target,
            dispatch_request_id: request_id.clone(),
            draft_target: draft_target.clone(),
        };
        self.append_locked(&entry)?;
        Ok(AcceptedMessage {
            id,
            session_hint: hint.clone(),
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
            target_session_id: if strict_target { hint } else { None },
            submission_id: None,
            strict_target,
            dispatch_request_id: request_id,
            draft_target,
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
                strict_target: dead.strict_target,
                dispatch_request_id: dead.dispatch_request_id.clone(),
                draft_target: dead.draft_target.clone(),
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
            target_session_id: dead
                .strict_target
                .then(|| dead.session_hint.clone())
                .flatten(),
            submission_id: None,
            strict_target: dead.strict_target,
            dispatch_request_id: dead.dispatch_request_id.clone(),
            draft_target: dead.draft_target.clone(),
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
        let lease_until_ms = now_ms.saturating_add(lease_ms);
        receipt.taken_at_ms.get_or_insert(now_ms);
        receipt.available_at_ms = None;
        receipt.attempts = receipt.attempts.saturating_add(1);
        receipt.last_attempt_id = Some(attempt_id.to_owned());
        receipt.lease = Some(Box::new(Lease {
            attempt_id: attempt_id.to_owned(),
            expires_at_ms: lease_until_ms,
        }));
        let claimed = receipt.clone();
        self.append_locked(&JournalEntry::Taken {
            id: id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            lease_until_ms,
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
        self.maybe_compact_after_terminal();
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
        self.maybe_compact_after_terminal();
        Ok(true)
    }

    /// Cold-boot lease release that protects receipts bound to sessions in
    /// `protected_sessions` (such as sessions suspended in
    /// [`crate::TurnState::WaitingForUser`] awaiting a persisted user card), so
    /// restarts never auto-requeue or drop user approval/disambiguation gates.
    pub fn release_uncommitted_leases_except(
        &self,
        now_ms: u64,
        protected_sessions: &BTreeSet<String>,
    ) -> io::Result<Vec<String>> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let receipts = self.replay_detailed_locked()?.0;
        let mut entries = Vec::new();
        let mut released_ids = Vec::new();
        for receipt in receipts {
            let protected = receipt
                .target_session_id
                .as_deref()
                .or(receipt.session_hint.as_deref())
                .is_some_and(|sid| protected_sessions.contains(sid));
            if protected {
                continue;
            }
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

    /// Compact excessive terminal history once the journal exceeds `min_lines`
    /// lines or `min_bytes` bytes. Exposed for deterministic tests.
    pub fn compact_with_thresholds(
        &self,
        min_lines: usize,
        min_bytes: u64,
    ) -> io::Result<bool> {
        let lock = self.queue_lock();
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.compact_locked(min_lines, min_bytes)
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
        if completed || dead {
            self.maybe_compact_after_terminal();
        }
        Ok(true)
    }

    fn maybe_compact_after_terminal(&self) {
        if let Err(error) =
            self.compact_locked(COMPACT_MIN_LINES, COMPACT_MIN_BYTES)
        {
            tracing::warn!(
                target: "flowy.acceptance",
                path = %Self::path(&self.root).display(),
                %error,
                "acceptance journal terminal compaction failed; retaining uncompacted journal"
            );
        }
    }

    /// One mutex per journal path. The desktop is single-instance, so an
    /// in-process lock is enough to serialise claims; it avoids a stale
    /// on-disk lock without adding a dependency.
    fn queue_lock(&self) -> Arc<Mutex<()>> {
        static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
            OnceLock::new();
        let registry = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
        let key = normalize_cache_path(&Self::path(&self.root));
        let mut locks = registry.lock().unwrap_or_else(PoisonError::into_inner);
        locks.entry(key).or_default().clone()
    }

    fn append_locked(&self, entry: &JournalEntry) -> io::Result<()> {
        self.append_batch_locked(std::slice::from_ref(entry))
    }

    fn append_batch_locked(&self, entries: &[JournalEntry]) -> io::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let path = Self::path(&self.root);
        let cache_key = normalize_cache_path(&path);
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
        let repaired = repair_torn_tail_locked(&path, &cache_key)?;
        let pre_len = fs::metadata(&path).map_or(0, |m| m.len());
        let write_result = (|| {
            let mut handle = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            handle.write_all(&buf)?;
            handle.flush()?;
            handle.sync_all()?;
            Ok::<Option<SystemTime>, io::Error>(
                handle
                    .metadata()
                    .ok()
                    .filter(|m| {
                        m.len() == pre_len.saturating_add(buf.len() as u64)
                    })
                    .and_then(|m| m.modified().ok()),
            )
        })();
        match write_result {
            Ok(post_modified) => {
                if !repaired
                    && let Some(mut cached) = cache_get(&cache_key)
                    && !cached.has_unparseable
                    && pre_len == cached.parsed_len
                    && buf.ends_with(b"\n")
                {
                    for entry in entries {
                        apply_journal_entry(
                            &mut cached.receipts,
                            &mut cached.index_by_id,
                            entry.clone(),
                        );
                    }
                    cached.parsed_len =
                        cached.parsed_len.saturating_add(buf.len() as u64);
                    advance_boundary_slices(
                        &mut cached.head,
                        &mut cached.tail,
                        &buf,
                    );
                    cached.modified = post_modified;
                    cache_insert(cache_key, cached);
                } else {
                    cache_invalidate(&cache_key);
                }
                Ok(())
            }
            Err(error) => {
                cache_invalidate(&cache_key);
                Err(error)
            }
        }
    }

    fn compact_locked(
        &self,
        min_lines: usize,
        min_bytes: u64,
    ) -> io::Result<bool> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        static REPLACE: Mutex<()> = Mutex::new(());

        let path = Self::path(&self.root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        let file_len = bytes.len() as u64;
        if file_len < min_bytes && file_len < min_lines as u64 {
            return Ok(false);
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if lines.len() < min_lines && file_len < min_bytes {
            return Ok(false);
        }
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Ok(false);
        }

        let mut receipts: Vec<AcceptedMessage> = Vec::new();
        let mut index_by_id: HashMap<String, usize> = HashMap::new();
        let mut last_event_at_ms: Vec<u64> = Vec::new();
        let mut last_event_seq: Vec<usize> = Vec::new();
        let mut superseded_by: HashMap<String, (String, u64)> = HashMap::new();
        let mut dead_at_ms: HashMap<String, u64> = HashMap::new();
        let mut cancelled_at_ms: HashMap<String, u64> = HashMap::new();
        let mut bound_at_ms: HashMap<String, u64> = HashMap::new();
        let mut released_at_ms: HashMap<String, (u64, u64)> = HashMap::new();

        for (seq, line) in lines.iter().enumerate() {
            let Ok(entry) = serde_json::from_str::<JournalEntry>(line) else {
                // Fail-closed: preserve corrupt journals untouched so forensic
                // evidence and quarantine invariants remain intact.
                return Ok(false);
            };
            let entry_id = entry.id().to_owned();
            let entry_at = entry.at_ms();
            let superseded_info = match &entry {
                JournalEntry::Superseded {
                    id,
                    replaced_by,
                    at_ms,
                } => Some((id.clone(), replaced_by.clone(), *at_ms)),
                _ => None,
            };
            let pre_dead = index_by_id
                .get(&entry_id)
                .is_some_and(|&pos| receipts[pos].dead);
            let pre_cancelled = index_by_id
                .get(&entry_id)
                .is_some_and(|&pos| receipts[pos].cancelled);
            let pre_bound = index_by_id.get(&entry_id).map(|&pos| {
                (
                    receipts[pos].target_session_id.clone(),
                    receipts[pos].submission_id.clone(),
                )
            });

            match &entry {
                JournalEntry::Released {
                    id,
                    retry_after_ms,
                    at_ms,
                    ..
                } => {
                    released_at_ms
                        .insert(id.clone(), (*at_ms, *retry_after_ms));
                }
                JournalEntry::Taken { id, .. } => {
                    released_at_ms.remove(id);
                }
                _ => {}
            }

            apply_journal_entry(&mut receipts, &mut index_by_id, entry);

            if let Some(&pos) = index_by_id.get(&entry_id) {
                if pos == last_event_at_ms.len() {
                    last_event_at_ms.push(entry_at);
                    last_event_seq.push(seq);
                } else {
                    last_event_at_ms[pos] = entry_at;
                    last_event_seq[pos] = seq;
                }
                let rec = &receipts[pos];
                if !pre_dead && rec.dead {
                    dead_at_ms.insert(entry_id.clone(), entry_at);
                }
                if !pre_cancelled && rec.cancelled {
                    if let Some((sid, rep, sup_at)) = superseded_info {
                        superseded_by.insert(sid, (rep, sup_at));
                    } else {
                        cancelled_at_ms.insert(entry_id.clone(), entry_at);
                    }
                }
                if let Some((prev_sid, prev_sub)) = pre_bound
                    && (rec.target_session_id != prev_sid
                        || rec.submission_id != prev_sub)
                {
                    bound_at_ms.insert(entry_id, entry_at);
                }
            }
        }

        let mut keep_indices: Vec<usize> = Vec::new();
        let mut terminal_indices: Vec<usize> = Vec::new();
        for (pos, receipt) in receipts.iter().enumerate() {
            if receipt.dead
                || (receipt.completed_at_ms.is_none() && !receipt.cancelled)
            {
                keep_indices.push(pos);
            } else {
                terminal_indices.push(pos);
            }
        }
        terminal_indices.sort_by(|&a, &b| {
            last_event_at_ms[b]
                .cmp(&last_event_at_ms[a])
                .then_with(|| last_event_seq[b].cmp(&last_event_seq[a]))
        });
        terminal_indices.truncate(COMPACT_RETAIN_TERMINAL);
        keep_indices.extend(terminal_indices);
        keep_indices.sort_unstable();
        keep_indices.dedup();

        let mut compacted_entries: Vec<JournalEntry> = Vec::new();
        for pos in keep_indices {
            let r = &receipts[pos];
            let fallback_at = last_event_at_ms[pos];
            compacted_entries.push(JournalEntry::Accepted {
                id: r.id.clone(),
                session_hint: r.session_hint.clone(),
                text: r.text.clone(),
                at_ms: r.created_at_ms,
                strict_target: r.strict_target,
                dispatch_request_id: r.dispatch_request_id.clone(),
                draft_target: r.draft_target.clone(),
            });
            if r.attempts > 0 {
                let taken_at = r.taken_at_ms.unwrap_or(r.created_at_ms);
                let final_attempt = r
                    .lease
                    .as_ref()
                    .map(|l| l.attempt_id.clone())
                    .or_else(|| r.last_attempt_id.clone())
                    .unwrap_or_default();
                for attempt_idx in 0..r.attempts {
                    if attempt_idx + 1 < r.attempts {
                        compacted_entries.push(JournalEntry::Taken {
                            id: r.id.clone(),
                            attempt_id: format!(
                                "compacted-attempt-{}",
                                attempt_idx + 1
                            ),
                            lease_until_ms: 0,
                            at_ms: taken_at,
                        });
                    } else {
                        compacted_entries.push(JournalEntry::Taken {
                            id: r.id.clone(),
                            attempt_id: final_attempt.clone(),
                            lease_until_ms: r
                                .lease
                                .as_ref()
                                .map_or(0, |l| l.expires_at_ms),
                            at_ms: taken_at,
                        });
                    }
                }
                if let (Some(sid), Some(sub_id)) =
                    (&r.target_session_id, &r.submission_id)
                {
                    compacted_entries.push(JournalEntry::Bound {
                        id: r.id.clone(),
                        attempt_id: final_attempt.clone(),
                        session_id: sid.clone(),
                        submission_id: sub_id.clone(),
                        at_ms: bound_at_ms
                            .get(&r.id)
                            .copied()
                            .unwrap_or(taken_at),
                    });
                }
                if let Some(avail) = r.available_at_ms {
                    let (rel_at, rel_retry) = released_at_ms
                        .get(&r.id)
                        .copied()
                        .filter(|(at, retry)| {
                            at.saturating_add(*retry) == avail
                        })
                        .unwrap_or((avail, 0));
                    compacted_entries.push(JournalEntry::Released {
                        id: r.id.clone(),
                        attempt_id: final_attempt.clone(),
                        retry_after_ms: rel_retry,
                        at_ms: rel_at,
                    });
                }
                if r.dead {
                    compacted_entries.push(JournalEntry::Dead {
                        id: r.id.clone(),
                        attempt_id: final_attempt,
                        at_ms: dead_at_ms
                            .get(&r.id)
                            .copied()
                            .unwrap_or(fallback_at),
                    });
                    if r.cancelled {
                        let (replaced_by, sup_at) =
                            superseded_by.get(&r.id).cloned().unwrap_or_else(
                                || ("compacted".to_owned(), fallback_at),
                            );
                        compacted_entries.push(JournalEntry::Superseded {
                            id: r.id.clone(),
                            replaced_by,
                            at_ms: sup_at,
                        });
                    }
                } else if let Some(done_at) = r.completed_at_ms {
                    compacted_entries.push(JournalEntry::Completed {
                        id: r.id.clone(),
                        attempt_id: final_attempt,
                        at_ms: done_at,
                    });
                } else if r.cancelled {
                    compacted_entries.push(JournalEntry::Cancelled {
                        id: r.id.clone(),
                        at_ms: cancelled_at_ms
                            .get(&r.id)
                            .copied()
                            .unwrap_or(fallback_at),
                    });
                }
            } else if r.cancelled {
                compacted_entries.push(JournalEntry::Cancelled {
                    id: r.id.clone(),
                    at_ms: cancelled_at_ms
                        .get(&r.id)
                        .copied()
                        .unwrap_or(fallback_at),
                });
            }
        }

        if compacted_entries.len() >= lines.len() {
            return Ok(false);
        }

        let mut out_buf = Vec::new();
        for entry in &compacted_entries {
            let line = serde_json::to_vec(entry).map_err(io::Error::other)?;
            out_buf.extend_from_slice(&line);
            out_buf.push(b'\n');
        }

        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("missing acceptance directory"))?;
        let (tmp, mut tmp_file) = loop {
            let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let tmp = dir.join(format!(
                ".journal.jsonl.{}.{}.tmp",
                std::process::id(),
                number
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
            {
                Ok(file) => break (tmp, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };
        let replace_result = (|| {
            tmp_file.write_all(&out_buf)?;
            tmp_file.flush()?;
            tmp_file.sync_all()?;
            drop(tmp_file);
            let _replace =
                REPLACE.lock().unwrap_or_else(PoisonError::into_inner);
            fs::rename(&tmp, &path)
        })();
        if replace_result.is_err() {
            let _ = fs::remove_file(&tmp);
            return replace_result.map(|()| false);
        }
        cache_invalidate(&normalize_cache_path(&path));
        Ok(true)
    }
}

impl JournalEntry {
    const fn id(&self) -> &str {
        match self {
            Self::Accepted { id, .. }
            | Self::Taken { id, .. }
            | Self::Bound { id, .. }
            | Self::Renewed { id, .. }
            | Self::Cancelled { id, .. }
            | Self::Completed { id, .. }
            | Self::Released { id, .. }
            | Self::Dead { id, .. }
            | Self::Superseded { id, .. } => id.as_str(),
        }
    }

    const fn at_ms(&self) -> u64 {
        match self {
            Self::Accepted { at_ms, .. }
            | Self::Taken { at_ms, .. }
            | Self::Bound { at_ms, .. }
            | Self::Renewed { at_ms, .. }
            | Self::Cancelled { at_ms, .. }
            | Self::Completed { at_ms, .. }
            | Self::Released { at_ms, .. }
            | Self::Dead { at_ms, .. }
            | Self::Superseded { at_ms, .. } => *at_ms,
        }
    }
}

#[derive(Clone)]
struct AcceptanceCacheEntry {
    parsed_len: u64,
    head: Vec<u8>,
    tail: Vec<u8>,
    modified: Option<SystemTime>,
    has_unparseable: bool,
    receipts: Vec<AcceptedMessage>,
    index_by_id: HashMap<String, usize>,
    corrupt_interior: usize,
    last_used_seq: u64,
}

fn cache_registry() -> &'static Mutex<HashMap<PathBuf, AcceptanceCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, AcceptanceCacheEntry>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &Path) -> Option<AcceptanceCacheEntry> {
    static CACHE_TOUCH: AtomicU64 = AtomicU64::new(1);
    let mut map = cache_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let cloned = if let Some(entry) = map.get_mut(key) {
        entry.last_used_seq = CACHE_TOUCH.fetch_add(1, Ordering::Relaxed);
        Some(entry.clone())
    } else {
        None
    };
    drop(map);
    cloned
}

fn cache_insert(key: PathBuf, mut entry: AcceptanceCacheEntry) {
    static CACHE_SEQ: AtomicU64 = AtomicU64::new(1);
    let mut map = cache_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !map.contains_key(&key)
        && map.len() >= MAX_CACHE_ENTRIES
        && let Some(evict_key) = map
            .iter()
            .min_by_key(|(_, val)| val.last_used_seq)
            .map(|(k, _)| k.clone())
    {
        map.remove(&evict_key);
    }
    entry.last_used_seq = CACHE_SEQ.fetch_add(1, Ordering::Relaxed);
    map.insert(key, entry);
}

fn cache_invalidate(key: &Path) {
    cache_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(key);
}

fn normalize_cache_path(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut clean = PathBuf::new();
    for component in abs.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    let mut tail = Vec::new();
    let mut probe = clean.as_path();
    loop {
        if let Ok(canon) = fs::canonicalize(probe) {
            let mut out = canon;
            for seg in tail.into_iter().rev() {
                out.push(seg);
            }
            return out;
        }
        match (probe.parent(), probe.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                probe = parent;
            }
            _ => return clean,
        }
    }
}

fn compute_boundary_slices(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let head_len = bytes.len().min(BOUNDARY_SLICE_BYTES);
    let tail_start = bytes.len().saturating_sub(BOUNDARY_SLICE_BYTES);
    (bytes[..head_len].to_vec(), bytes[tail_start..].to_vec())
}

fn advance_boundary_slices(
    head: &mut Vec<u8>,
    tail: &mut Vec<u8>,
    appended: &[u8],
) {
    if head.len() < BOUNDARY_SLICE_BYTES {
        let need = BOUNDARY_SLICE_BYTES - head.len();
        head.extend_from_slice(&appended[..appended.len().min(need)]);
    }
    if appended.len() >= BOUNDARY_SLICE_BYTES {
        *tail = appended[appended.len() - BOUNDARY_SLICE_BYTES..].to_vec();
    } else {
        let mut combined = Vec::with_capacity(
            (tail.len() + appended.len()).min(BOUNDARY_SLICE_BYTES * 2),
        );
        combined.extend_from_slice(tail);
        combined.extend_from_slice(appended);
        let start = combined.len().saturating_sub(BOUNDARY_SLICE_BYTES);
        *tail = combined[start..].to_vec();
    }
}

fn verify_boundary_slices(
    file: &mut fs::File,
    parsed_len: u64,
    expected_head: &[u8],
    expected_tail: &[u8],
) -> io::Result<bool> {
    if parsed_len == 0 {
        return Ok(expected_head.is_empty() && expected_tail.is_empty());
    }
    let head_len = usize::try_from(parsed_len.min(BOUNDARY_SLICE_BYTES as u64))
        .unwrap_or(BOUNDARY_SLICE_BYTES);
    if expected_head.len() != head_len {
        return Ok(false);
    }
    let mut head_buf = vec![0_u8; head_len];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut head_buf)?;
    if head_buf != expected_head {
        return Ok(false);
    }
    if parsed_len <= BOUNDARY_SLICE_BYTES as u64 {
        return Ok(head_buf == expected_tail);
    }
    if expected_tail.len() != BOUNDARY_SLICE_BYTES {
        return Ok(false);
    }
    let tail_start = parsed_len - BOUNDARY_SLICE_BYTES as u64;
    let mut tail_buf = vec![0_u8; BOUNDARY_SLICE_BYTES];
    file.seek(SeekFrom::Start(tail_start))?;
    file.read_exact(&mut tail_buf)?;
    Ok(tail_buf == expected_tail)
}

type ReplayParseOutput = (
    Vec<AcceptedMessage>,
    HashMap<String, usize>,
    HashMap<String, usize>,
    usize,
    Option<usize>,
    bool,
);

fn replay_bytes(bytes: &[u8]) -> ReplayParseOutput {
    let text = String::from_utf8_lossy(bytes);
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
    let mut has_unparseable = false;
    for (idx, line) in lines.iter().enumerate() {
        let Ok(entry) = serde_json::from_str::<JournalEntry>(line) else {
            has_unparseable = true;
            if idx + 1 < lines.len() {
                corrupt_interior = corrupt_interior.saturating_add(1);
                last_interior_corrupt_line = Some(idx);
            }
            continue;
        };
        if let JournalEntry::Taken { id, .. } = &entry
            && index_by_id.contains_key(id)
        {
            last_taken_line.insert(id.clone(), idx);
        }
        apply_journal_entry(&mut receipts, &mut index_by_id, entry);
    }
    (
        receipts,
        index_by_id,
        last_taken_line,
        corrupt_interior,
        last_interior_corrupt_line,
        has_unparseable,
    )
}

fn apply_journal_entry(
    receipts: &mut Vec<AcceptedMessage>,
    index_by_id: &mut HashMap<String, usize>,
    entry: JournalEntry,
) {
    match entry {
        JournalEntry::Accepted {
            id,
            session_hint,
            text,
            at_ms,
            strict_target,
            dispatch_request_id,
            draft_target,
        } => {
            let pos = receipts.len();
            index_by_id.entry(id.clone()).or_insert(pos);
            let target_session_id =
                strict_target.then(|| session_hint.clone()).flatten();
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
                target_session_id,
                submission_id: None,
                strict_target,
                dispatch_request_id,
                draft_target,
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
                receipt.taken_at_ms.get_or_insert(at_ms);
                receipt.available_at_ms = None;
                receipt.attempts = receipt.attempts.saturating_add(1);
                receipt.last_attempt_id = Some(attempt_id.clone());
                receipt.lease = Some(Box::new(Lease {
                    attempt_id,
                    expires_at_ms: lease_until_ms,
                }));
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
                let owned = receipt
                    .lease
                    .as_ref()
                    .is_some_and(|lease| lease.attempt_id == attempt_id);
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

fn quarantine_pre_corruption_receipts(
    path: &Path,
    receipts: &mut [AcceptedMessage],
    last_taken_line: &HashMap<String, usize>,
    corrupt_idx: usize,
    corrupt_interior: usize,
) {
    tracing::error!(
        target: "flowy.acceptance",
        path = %path.display(),
        corrupt_interior,
        "interior journal corruption detected; quarantining pre-corruption in-flight receipts"
    );
    for receipt in receipts {
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
/// - If the file already ends with `\n` (or is empty / missing), returns `Ok(false)`.
/// - If the trailing non-newline fragment parses as a complete [`JournalEntry`],
///   appends `\n` so that valid entry is preserved, invalidates cache, and returns `Ok(true)`.
/// - If the trailing fragment is unparseable (partial crash write), truncates
///   the file back to the end of the last complete newline-terminated line so
///   the torn bytes are never promoted into an interior corrupt line that would
///   quarantine earlier healthy receipts, invalidates cache, and returns `Ok(true)`.
fn repair_torn_tail_locked(path: &Path, cache_key: &Path) -> io::Result<bool> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let len = file.seek(SeekFrom::End(0))?;
    if len == 0 {
        return Ok(false);
    }
    // Fast path: check only the final byte so normal appends never read the
    // whole journal into memory.
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(false);
    }
    drop(file);
    // Rare crash-recovery path: the final line lacks a trailing newline.
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(false);
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
        handle.sync_all()?;
    } else {
        let handle = fs::OpenOptions::new().write(true).open(path)?;
        handle.set_len(cut as u64)?;
        handle.sync_all()?;
    }
    cache_invalidate(cache_key);
    Ok(true)
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum ReplayStatKind {
    CacheHit,
    TailParse,
    FullReplay,
}

#[cfg(test)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct ReplayStats {
    cache_hits: usize,
    tail_parses: usize,
    full_replays: usize,
}

#[cfg(test)]
std::thread_local! {
    static TEST_REPLAY_STATS: std::cell::Cell<ReplayStats> =
        const { std::cell::Cell::new(ReplayStats { cache_hits: 0, tail_parses: 0, full_replays: 0 }) };
}

#[cfg(test)]
fn record_replay_stat(kind: ReplayStatKind) {
    TEST_REPLAY_STATS.with(|cell| {
        let mut stats = cell.get();
        match kind {
            ReplayStatKind::CacheHit => stats.cache_hits += 1,
            ReplayStatKind::TailParse => stats.tail_parses += 1,
            ReplayStatKind::FullReplay => stats.full_replays += 1,
        }
        cell.set(stats);
    });
}

#[cfg(test)]
fn take_replay_stats() -> ReplayStats {
    TEST_REPLAY_STATS.with(|cell| cell.replace(ReplayStats::default()))
}

#[cfg(test)]
#[path = "acceptance_tests.rs"]
mod tests;

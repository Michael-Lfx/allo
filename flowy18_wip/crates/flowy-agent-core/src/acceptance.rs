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
        assert_eq!(queue.pending().expect("pending"), Vec::new());
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

        assert_eq!(queue.pending_at(10_000_000).expect("pending"), Vec::new());
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
        assert_eq!(queue.pending_at(1_100).expect("pending"), Vec::new());
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
            .release_uncommitted_leases_except(2_000, &BTreeSet::new())
            .expect("release stale leases");
        assert_eq!(released, vec![r.id.clone()]);
        assert_eq!(
            queue.get(&r.id).unwrap().unwrap().state_at(2_000),
            ReceiptState::Retrying
        );
        assert_eq!(queue.pending_at(2_000).unwrap().len(), 1);

        // A receipt bound to session B whose stale session_hint points to
        // protected session A must still be released on cold boot.
        let r2 = queue
            .accept(Some("session-a"), "rebound to b", 2_100)
            .expect("accept r2");
        queue
            .take(&r2.id, "att-r2", 2_110, 30_000)
            .expect("take r2")
            .expect("claimed r2");
        queue
            .bind(&r2.id, "att-r2", "session-b", "sub-r2", 2_120)
            .expect("bind r2");
        let protected = BTreeSet::from(["session-a".to_owned()]);
        let released_r2 = queue
            .release_uncommitted_leases_except(2_200, &protected)
            .expect("release rebound r2");
        assert_eq!(released_r2, vec![r2.id]);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn draft_target_survives_journal_replay_and_retry_dead_across_reopen() {
        let root = temp_root("draft-target");
        let queue = AcceptanceQueue::new(&root);
        let target = DraftTarget {
            origin: SessionOrigin::Project {
                project_id: Some("proj-alpha".into()),
            },
            workspace: Some(SessionWorkspace {
                root: PathBuf::from(r"C:\work\proj-alpha"),
            }),
            role: SessionRole::Router,
        };
        let accepted = queue
            .accept_with_target(
                None,
                "cold start draft prompt",
                false,
                Some(target.clone()),
                None,
                1_000,
            )
            .expect("accept draft");
        assert_eq!(accepted.draft_target.as_deref(), Some(&target));

        let reopened = AcceptanceQueue::new(&root);
        let replayed = reopened.get(&accepted.id).unwrap().unwrap();
        assert_eq!(replayed.draft_target.as_deref(), Some(&target));

        reopened
            .take(&accepted.id, "att-1", 1_100, 30_000)
            .unwrap()
            .unwrap();
        assert!(reopened.give_up(&accepted.id, "att-1", 1_200).unwrap());
        let retried = reopened
            .retry_dead(&accepted.id, 1_300)
            .unwrap()
            .expect("retry_dead preserves draft target");
        assert_eq!(retried.draft_target.as_deref(), Some(&target));

        let reopened_again = AcceptanceQueue::new(&root);
        let retried_replayed =
            reopened_again.get(&retried.id).unwrap().unwrap();
        assert_eq!(retried_replayed.draft_target.as_deref(), Some(&target));

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn tail_cache_hits_unchanged_and_incrementally_parses_appends_including_torn_tail()
     {
        let root = temp_root("tail-cache-incremental");
        let queue = AcceptanceQueue::new(&root);
        // Write enough entries so file exceeds 4096 bytes (exercises separate
        // head and tail boundary slices).
        let padding = "x".repeat(256);
        for i in 0..24_u64 {
            let _ = queue
                .accept(Some("sess"), &format!("msg-{i}-{padding}"), 1_000 + i)
                .expect("accept");
        }
        let path = AcceptanceQueue::path(&root);
        assert!(fs::metadata(&path).unwrap().len() > 4096);

        let _ = take_replay_stats();
        let initial = queue.all().expect("initial replay");
        assert_eq!(initial.len(), 24);
        assert_eq!(
            take_replay_stats(),
            ReplayStats {
                cache_hits: 0,
                tail_parses: 0,
                full_replays: 1
            }
        );

        // Unchanged second read must hit cache directly without full replay.
        let cached_read = queue.all().expect("cached replay");
        assert_eq!(cached_read, initial);
        assert_eq!(
            take_replay_stats(),
            ReplayStats {
                cache_hits: 1,
                tail_parses: 0,
                full_replays: 0
            }
        );

        // Append a torn trailing fragment without `\n`: parsed_len must NOT
        // advance across the partial fragment, and incremental tail read must
        // tolerate it cleanly.
        let raw_before_torn = fs::read(&path).expect("read before torn");
        {
            let mut handle = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open append");
            handle
                .write_all(br#"{"event":"accepted","id":"torn-"#)
                .expect("write torn");
        }
        let with_torn = queue.all().expect("replay with torn");
        assert_eq!(with_torn.len(), 24);
        assert_eq!(
            take_replay_stats(),
            ReplayStats {
                cache_hits: 0,
                tail_parses: 1,
                full_replays: 0
            }
        );

        // Complete the torn fragment externally into a valid `\n`-terminated
        // line; incremental tail parse must pick up ONLY from previous
        // `parsed_len` (`raw_before_torn.len()`).
        let completed_line = JournalEntry::Accepted {
            id: "ext-completed".into(),
            session_hint: Some("sess".into()),
            text: "completed externally".into(),
            at_ms: 2_000,
            strict_target: false,
            dispatch_request_id: None,
            draft_target: None,
        };
        let mut repaired_bytes = raw_before_torn;
        repaired_bytes
            .extend_from_slice(&serde_json::to_vec(&completed_line).unwrap());
        repaired_bytes.push(b'\n');
        fs::write(&path, &repaired_bytes).expect("write completed");

        let after_complete = queue.all().expect("replay after external line");
        assert_eq!(after_complete.len(), 25);
        assert_eq!(after_complete[24].id, "ext-completed");
        assert_eq!(
            take_replay_stats(),
            ReplayStats {
                cache_hits: 0,
                tail_parses: 1,
                full_replays: 0
            }
        );

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn tail_cache_fails_closed_on_truncation_and_same_length_head_or_tail_rewrite()
     {
        let root = temp_root("tail-cache-fail-closed");
        let queue = AcceptanceQueue::new(&root);
        let padding = "p".repeat(256);
        for i in 0..30_u64 {
            queue
                .accept(
                    Some("home"),
                    &format!("item-{i:02}-{padding}"),
                    1_000 + i,
                )
                .expect("accept");
        }
        let path = AcceptanceQueue::path(&root);
        let original = fs::read(&path).expect("read original");
        assert!(original.len() > 8192);

        let _ = take_replay_stats();
        let first = queue.all().expect("prime cache");
        assert_eq!(first.len(), 30);
        assert_eq!(take_replay_stats().full_replays, 1);

        // 1) Same-length external rewrite inside head (first 4096 bytes) must
        //    fail closed via boundary mismatch and fall back to full replay.
        let mut head_mutated = original.clone();
        let needle = b"item-00-p";
        let head_pos = head_mutated
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("needle in head");
        assert!(head_pos < 4096);
        head_mutated[head_pos..head_pos + needle.len()]
            .copy_from_slice(b"item-00-Q");
        assert_eq!(head_mutated.len(), original.len());
        fs::write(&path, &head_mutated).expect("write head mutated");

        let after_head_rewrite =
            queue.all().expect("replay after head rewrite");
        assert_eq!(take_replay_stats().full_replays, 1);
        assert!(after_head_rewrite[0].text.starts_with("item-00-Q"));

        // 2) Same-length external rewrite inside tail (last 4096 bytes) must
        //    also fail closed and re-read from offset 0.
        let mut tail_mutated = head_mutated.clone();
        let tail_needle = b"item-29-p";
        let tail_pos = tail_mutated
            .windows(tail_needle.len())
            .position(|w| w == tail_needle)
            .expect("needle in tail");
        assert!(tail_pos + 4096 >= tail_mutated.len());
        tail_mutated[tail_pos..tail_pos + tail_needle.len()]
            .copy_from_slice(b"item-29-Z");
        assert_eq!(tail_mutated.len(), head_mutated.len());
        fs::write(&path, &tail_mutated).expect("write tail mutated");

        let after_tail_rewrite =
            queue.all().expect("replay after tail rewrite");
        assert_eq!(take_replay_stats().full_replays, 1);
        assert!(after_tail_rewrite[29].text.starts_with("item-29-Z"));

        // 3) External truncation (`len < parsed_len`) must invalidate and fall
        //    back to full offset-0 replay.
        let first_line_end = tail_mutated
            .iter()
            .position(|&b| b == b'\n')
            .expect("newline")
            + 1;
        fs::write(&path, &tail_mutated[..first_line_end])
            .expect("truncate externally");
        let after_truncate = queue.all().expect("replay after truncate");
        assert_eq!(take_replay_stats().full_replays, 1);
        assert_eq!(after_truncate.len(), 1);
        assert!(after_truncate[0].text.starts_with("item-00-Q"));

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn tail_cache_stays_bounded_under_max_entries() {
        let mut roots = Vec::new();
        for i in 0..80_usize {
            let root = temp_root(&format!("cache-bound-{i}"));
            let queue = AcceptanceQueue::new(&root);
            queue.accept(None, "bound", 1_000).expect("accept");
            let _ = queue.all().expect("populate cache");
            roots.push(root);
        }
        let len = cache_registry()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        assert!(
            len <= MAX_CACHE_ENTRIES,
            "cache size {len} exceeded bound {MAX_CACHE_ENTRIES}"
        );
        for root in roots {
            fs::remove_dir_all(&root).expect("cleanup");
        }
    }

    #[test]
    fn accept_never_compacts_while_terminal_transition_compacts_preserving_fidelity_across_reopen()
     {
        let root = temp_root("safe-compact");
        let queue = AcceptanceQueue::new(&root);

        // 1) Append 260 completed receipts -> total > 512 lines if completed,
        //    but first test that `accept` alone NEVER compacts even > 512 lines.
        for i in 0..520_u64 {
            let _ = queue
                .accept(Some("s-init"), &format!("queued-{i}"), 1_000 + i)
                .expect("accept");
        }
        let path = AcceptanceQueue::path(&root);
        let accept_only_bytes = fs::read_to_string(&path).expect("read");
        assert_eq!(
            accept_only_bytes.lines().count(),
            520,
            "accept() must never trigger compaction"
        );

        // 2) Now build a rich mixed-state journal in a second root:
        //    - 250 completed receipts (various attempts, bindings, releases)
        //    - 10 cancelled receipts
        //    - 2 dead receipts (one un-superseded, one superseded via retry_dead)
        //    - 3 non-terminal receipts (Queued, Running, Released-with-backoff)
        let rich_root = temp_root("safe-compact-rich");
        let rich = AcceptanceQueue::new(&rich_root);
        let target = DraftTarget {
            origin: SessionOrigin::Project {
                project_id: Some("proj-compact".into()),
            },
            workspace: Some(SessionWorkspace {
                root: PathBuf::from(r"C:\work\compact"),
            }),
            role: SessionRole::Worker,
        };

        let mut expected_kept_ids = BTreeSet::new();
        for i in 0..250_u64 {
            let r = rich
                .accept_with_target(
                    Some("hint-term"),
                    &format!("done-{i}"),
                    false,
                    Some(target.clone()),
                    Some(&format!("req-{i}")),
                    10_000 + i * 10,
                )
                .expect("accept done");
            rich.take(&r.id, "att-1", 10_001 + i * 10, 5_000)
                .expect("take")
                .expect("claimed");
            if i % 3 == 0 {
                // Multi-attempt with release + bind on attempt 2
                rich.release(&r.id, "att-1", 10_002 + i * 10, 100)
                    .expect("release");
                rich.take(&r.id, "att-2", 10_200 + i * 10, 5_000)
                    .expect("take 2")
                    .expect("claimed 2");
                rich.bind(
                    &r.id,
                    "att-2",
                    &format!("sess-{i}"),
                    &format!("sub-{i}"),
                    10_201 + i * 10,
                )
                .expect("bind");
                rich.complete(&r.id, "att-2", 10_205 + i * 10)
                    .expect("complete");
            } else {
                rich.complete(&r.id, "att-1", 10_005 + i * 10)
                    .expect("complete");
            }
        }
        for i in 0..10_u64 {
            let r = rich
                .accept(Some("hint-cancel"), &format!("cancel-{i}"), 20_000 + i)
                .expect("accept cancel");
            assert!(rich.cancel(&r.id, 20_500 + i).expect("cancel"));
            expected_kept_ids.insert(r.id);
        }

        // Dead (un-superseded)
        let dead_keep = rich
            .accept(Some("hint-dead"), "dead keep", 21_000)
            .expect("dead keep");
        rich.take(&dead_keep.id, "att-dead", 21_010, 5_000)
            .expect("take dead")
            .expect("claimed");
        assert!(
            rich.give_up(&dead_keep.id, "att-dead", 21_020)
                .expect("give_up")
        );
        expected_kept_ids.insert(dead_keep.id.clone());

        // Dead + Superseded (must stay dead & cancelled across compaction so
        // retry_dead remains idempotent)
        let dead_sup = rich
            .accept(Some("hint-sup"), "dead superseded", 21_100)
            .expect("dead sup");
        rich.take(&dead_sup.id, "att-sup", 21_110, 5_000)
            .expect("take sup")
            .expect("claimed");
        assert!(
            rich.give_up(&dead_sup.id, "att-sup", 21_120)
                .expect("give_up")
        );
        let retried_from_sup = rich
            .retry_dead(&dead_sup.id, 21_130)
            .expect("retry_dead")
            .expect("requeued");
        expected_kept_ids.insert(dead_sup.id.clone());
        expected_kept_ids.insert(retried_from_sup.id);

        // Non-terminal: Queued (strict_target = true), Running (bound + renewed),
        // Released (with backoff)
        let nt_queued = rich
            .accept_strict(
                "worker-strict",
                "queued non-term",
                Some("req-q"),
                22_000,
            )
            .expect("accept strict");
        expected_kept_ids.insert(nt_queued.id);

        let nt_running = rich
            .accept(Some("hint-run"), "running non-term", 22_100)
            .expect("accept run");
        rich.take(&nt_running.id, "att-run", 22_110, 5_000)
            .expect("take run")
            .expect("claimed");
        rich.bind(&nt_running.id, "att-run", "sess-run", "sub-run", 22_115)
            .expect("bind run");
        rich.renew_lease(&nt_running.id, "att-run", 22_120, 15_000)
            .expect("renew run");
        expected_kept_ids.insert(nt_running.id);

        let nt_released = rich
            .accept(Some("hint-rel"), "released non-term", 22_200)
            .expect("accept rel");
        rich.take(&nt_released.id, "att-rel", 22_210, 5_000)
            .expect("take rel")
            .expect("claimed");
        rich.release(&nt_released.id, "att-rel", 22_220, 3_000)
            .expect("release rel");
        expected_kept_ids.insert(nt_released.id);

        let before_snapshot = rich.all().expect("snapshot before compact");
        let rich_path = AcceptanceQueue::path(&rich_root);
        let lines_before = fs::read_to_string(&rich_path)
            .expect("read before")
            .lines()
            .count();
        assert!(lines_before > COMPACT_MIN_LINES);

        // Trigger terminal compaction on one more completion (note: all prior
        // completions already hit COMPACT_MIN_LINES once line 512 was crossed,
        // and we can also verify compact_with_thresholds directly).
        let final_done = rich
            .accept(Some("hint-final"), "final done", 23_000)
            .expect("final");
        rich.take(&final_done.id, "att-fin", 23_010, 5_000)
            .expect("take fin")
            .expect("claimed");
        assert!(
            rich.complete(&final_done.id, "att-fin", 23_020)
                .expect("complete fin")
        );
        expected_kept_ids.insert(final_done.id);

        let lines_after = fs::read_to_string(&rich_path)
            .expect("read after")
            .lines()
            .count();
        assert!(
            lines_after < lines_before,
            "terminal compaction must prune older terminal records ({lines_after} < {lines_before})"
        );

        // Reopen fresh handle (no cache reuse across handles if invalidated,
        // and full roundtrip verification).
        let _ = take_replay_stats();
        let reopened = AcceptanceQueue::new(&rich_root);
        let after_reopen = reopened.all().expect("replay after compact");
        assert_eq!(
            take_replay_stats().full_replays,
            1,
            "compaction must invalidate cache"
        );

        // Verify exact count: 200 newest terminal + 2 dead + 4 non-terminal
        // (`retried_from_sup`, `nt_queued`, `nt_running`, `nt_released`) = 206.
        assert_eq!(after_reopen.len(), 206);

        let before_by_id: HashMap<String, AcceptedMessage> = before_snapshot
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        for kept in &after_reopen {
            if let Some(prev) = before_by_id.get(&kept.id) {
                assert_eq!(
                    kept, prev,
                    "compacted receipt {} must roundtrip 100% losslessly",
                    kept.id
                );
            }
        }
        for id in &expected_kept_ids {
            assert!(
                after_reopen.iter().any(|r| &r.id == id),
                "expected kept receipt {id} missing after compaction"
            );
        }
        // `retry_dead` on superseded dead receipt must still be rejected!
        assert!(
            reopened
                .retry_dead(&dead_sup.id, 30_000)
                .expect("idempotent retry_dead")
                .is_none()
        );
        // Un-superseded dead receipt must still be retryable!
        assert!(
            reopened
                .retry_dead(&dead_keep.id, 30_001)
                .expect("retry_dead on kept dead")
                .is_some()
        );

        // Also test deterministic helper `compact_with_thresholds` does not
        // compact when below threshold.
        let small_root = temp_root("compact-threshold");
        let small = AcceptanceQueue::new(&small_root);
        for i in 0..5_u64 {
            let r = small.accept(None, &format!("s-{i}"), 1_000 + i).unwrap();
            small.take(&r.id, "a", 1_010 + i, 5_000).unwrap().unwrap();
            small.complete(&r.id, "a", 1_020 + i).unwrap();
        }
        assert!(
            !small
                .compact_with_thresholds(100, 1024 * 1024)
                .expect("below threshold")
        );

        fs::remove_dir_all(&root).expect("cleanup");
        fs::remove_dir_all(&rich_root).expect("cleanup");
        fs::remove_dir_all(&small_root).expect("cleanup");
    }

    #[test]
    fn compaction_skips_corrupt_journals_preserving_quarantine_flag() {
        let root = temp_root("compact-corrupt-skip");
        let queue = AcceptanceQueue::new(&root);
        let first = queue.accept(None, "in-flight", 1_000).expect("first");
        queue
            .take(&first.id, "att-1", 1_010, 5_000)
            .expect("take")
            .expect("claimed");
        let path = AcceptanceQueue::path(&root);
        {
            let mut f =
                fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(f, r#"{{"event":"broken""#).unwrap();
        }
        for i in 0..10_u64 {
            let r = queue.accept(None, &format!("ok-{i}"), 2_000 + i).unwrap();
            queue.take(&r.id, "a", 2_010 + i, 5_000).unwrap().unwrap();
            queue.complete(&r.id, "a", 2_020 + i).unwrap();
        }
        assert!(queue.has_interior_corruption().unwrap());
        assert!(
            !queue
                .compact_with_thresholds(1, 1)
                .expect("must skip corrupt journal")
        );
        assert!(queue.has_interior_corruption().unwrap());
        assert!(queue.get(&first.id).unwrap().unwrap().quarantined);
        fs::remove_dir_all(&root).expect("cleanup");
    }
}

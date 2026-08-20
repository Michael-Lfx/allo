//! JSONL observation writer under `{data_dir}/diagnostics/observation/`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capture::capture_and_size_cap;
use crate::event::{
    ids_from_payload, ObservationEvent, ObservationIds, EVENT_OBSERVATION_GAP,
    EVENT_TURN_END, EVENT_TURN_START, OBSERVATION_SCHEMA_VERSION, PROCESS_BOUNDARY_ID,
};
use crate::project::{
    event_belongs_to_turn, project_event_refs, ObservationSummary, ObservationSummaryFold,
};

const FOLDER_SEGMENT_MAX: usize = 128;

fn fnv1a64(raw: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in raw.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn percent_encode_path_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        if byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_' {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Folder name for one conversation. Safe charset is kept as-is; anything else
/// is percent-encoded so `foo.bar` and `foobar` cannot share a directory.
fn sanitize_path_segment(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "unknown".to_owned();
    }
    if trimmed.len() <= FOLDER_SEGMENT_MAX
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return trimmed.to_owned();
    }
    let encoded = percent_encode_path_segment(trimmed);
    if encoded.len() <= FOLDER_SEGMENT_MAX {
        encoded
    } else {
        format!("h_{:016x}", fnv1a64(trimmed))
    }
}

pub const OBSERVATION_DIR: &str = "diagnostics/observation";
pub const ROTATE_BYTES: u64 = 48 * 1024 * 1024;
pub const MAX_TOTAL_OBSERVATION_BYTES: u64 = 1024 * 1024 * 1024;
pub const GC_QUOTA_HIGH_BYTES: u64 = MAX_TOTAL_OBSERVATION_BYTES;
pub const GC_QUOTA_LOW_BYTES: u64 = MAX_TOTAL_OBSERVATION_BYTES - 200 * 1024 * 1024;
pub const GC_QUOTA_EMERGENCY_BYTES: u64 = MAX_TOTAL_OBSERVATION_BYTES + 200 * 1024 * 1024;
/// Writer-queue idle before a normal quota scan.
const GC_IDLE_SECS: u64 = 30;
/// Skip a full directory walk when a normal GC ran recently.
const GC_INTERVAL_SECS: u64 = 60 * 60;
const WRITER_FLUSH_INTERVAL: Duration = Duration::from_millis(75);
const WRITER_FLUSH_BATCH: usize = 32;
const WRITER_ACK_TIMEOUT: Duration = Duration::from_secs(10);
/// List/turn/call reads wait this long for a flush ACK, then read whatever is on disk.
const READ_FLUSH_TIMEOUT: Duration = Duration::from_millis(500);
const BUFWRITER_CAPACITY: usize = 64 * 1024;
const WRITER_IDLE: Duration = Duration::from_secs(10 * 60);
const SEQ_IDLE: Duration = Duration::from_secs(10 * 60);
const MAX_WRITERS: usize = 64;
const MAX_SEQ_BOUNDARIES: usize = 4096;
pub const EVENTS_FILE: &str = "events.jsonl";
/// Ordinary event queue depth. 128 × 128 KiB ≈ 16 MiB.
pub const MAX_QUEUE_EVENTS: usize = 128;
pub const MAX_CONTROL_EVENTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecorderHealthStatus {
    Healthy,
    QueueDropped,
    StorageError,
    WriterDisconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecorderHealth {
    pub status: RecorderHealthStatus,
    pub last_error: Option<String>,
}

impl Default for RecorderHealth {
    fn default() -> Self {
        Self {
            status: RecorderHealthStatus::Healthy,
            last_error: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecorderError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("lock poisoned")]
    LockPoisoned,
}

/// Writer-thread command. `enqueue_order` is assigned at enqueue and never
/// stored on the JSONL envelope.
#[derive(Debug)]
pub(crate) enum WriterCommand {
    Event {
        event_type: String,
        ids: ObservationIds,
        payload: Value,
        generation: u64,
    },
    Flush {
        ack: Option<Sender<()>>,
    },
    DeleteConversation {
        conversation_id: String,
        ack: Option<Sender<()>>,
    },
    ClearConversation {
        conversation_id: String,
        ack: Option<Sender<()>>,
    },
    ResetAll {
        ack: Option<Sender<()>>,
    },
    Shutdown {
        ack: Option<Sender<()>>,
    },
}

impl WriterCommand {
    fn is_control(&self) -> bool {
        match self {
            Self::Event { event_type, .. } => is_control_event_type(event_type),
            Self::Flush { .. }
            | Self::DeleteConversation { .. }
            | Self::ClearConversation { .. }
            | Self::ResetAll { .. }
            | Self::Shutdown { .. } => true,
        }
    }

    fn is_lifecycle(&self) -> bool {
        !matches!(self, Self::Event { .. })
    }

    fn take_ack(&mut self) -> Option<Sender<()>> {
        match self {
            Self::Flush { ack }
            | Self::DeleteConversation { ack, .. }
            | Self::ClearConversation { ack, .. }
            | Self::ResetAll { ack }
            | Self::Shutdown { ack } => ack.take(),
            Self::Event { .. } => None,
        }
    }
}

pub(crate) fn is_control_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        EVENT_OBSERVATION_GAP | EVENT_TURN_START | EVENT_TURN_END
    ) || event_type.starts_with("turn/")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnqueueOutcome {
    Queued,
    DroppedNormal,
    DroppedControl,
}

/// Dual bounded queues merged by `enqueue_order` (never control-first persist).
pub(crate) struct DualQueue {
    enqueue_order: AtomicU64,
    normal: VecDeque<(u64, WriterCommand)>,
    control: VecDeque<(u64, WriterCommand)>,
    max_normal: usize,
    max_control: usize,
    dropped_normal: u64,
    /// `(conversation_id, root_turn_id, generation)` → lost count. Empty strings mean unbound.
    dropped_overflows: HashMap<(String, String, u64), u64>,
    flush_pending: bool,
    extra_flush_acks: Vec<mpsc::Sender<()>>,
}

impl DualQueue {
    fn new(max_normal: usize, max_control: usize) -> Self {
        Self {
            enqueue_order: AtomicU64::new(0),
            normal: VecDeque::new(),
            control: VecDeque::new(),
            max_normal,
            max_control,
            dropped_normal: 0,
            dropped_overflows: HashMap::new(),
            flush_pending: false,
            extra_flush_acks: Vec::new(),
        }
    }

    fn overflow_key(ids: &ObservationIds, generation: u64) -> (String, String, u64) {
        (
            ids.conversation_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .unwrap_or("")
                .to_owned(),
            ids.root_turn_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .unwrap_or("")
                .to_owned(),
            generation,
        )
    }

    fn note_dropped_event(&mut self, command: &WriterCommand) {
        self.dropped_normal = self.dropped_normal.saturating_add(1);
        let WriterCommand::Event { ids, generation, .. } = command else {
            return;
        };
        *self
            .dropped_overflows
            .entry(Self::overflow_key(ids, *generation))
            .or_insert(0) += 1;
    }

    fn take_extra_flush_acks(&mut self) -> Vec<mpsc::Sender<()>> {
        std::mem::take(&mut self.extra_flush_acks)
    }

    fn try_enqueue(&mut self, command: WriterCommand) -> EnqueueOutcome {
        if let WriterCommand::Flush { ack } = command {
            if self.flush_pending {
                if let Some(ack) = ack {
                    self.extra_flush_acks.push(ack);
                }
                return EnqueueOutcome::Queued;
            }
            self.flush_pending = true;
            let order = self.enqueue_order.fetch_add(1, Ordering::Relaxed);
            self.control.push_back((order, WriterCommand::Flush { ack }));
            return EnqueueOutcome::Queued;
        }
        let order = self.enqueue_order.fetch_add(1, Ordering::Relaxed);
        if command.is_lifecycle() {
            self.control.push_back((order, command));
            return EnqueueOutcome::Queued;
        }
        if command.is_control() {
            if self.control.len() >= self.max_control {
                if let Some((_, dropped)) = self.normal.pop_front() {
                    self.note_dropped_event(&dropped);
                }
                if self.control.len() >= self.max_control {
                    return EnqueueOutcome::DroppedControl;
                }
            } else if self.normal.len() >= self.max_normal {
                if let Some((_, dropped)) = self.normal.pop_front() {
                    self.note_dropped_event(&dropped);
                }
            }
            self.control.push_back((order, command));
            EnqueueOutcome::Queued
        } else if self.normal.len() >= self.max_normal {
            self.note_dropped_event(&command);
            EnqueueOutcome::DroppedNormal
        } else {
            self.normal.push_back((order, command));
            EnqueueOutcome::Queued
        }
    }

    fn dequeue_next(&mut self) -> Option<(u64, WriterCommand)> {
        let next = match (
            self.normal.front().map(|(order, _)| *order),
            self.control.front().map(|(order, _)| *order),
        ) {
            (Some(normal_order), Some(control_order)) if normal_order <= control_order => {
                self.normal.pop_front()
            }
            (Some(_), Some(_)) => self.control.pop_front(),
            (Some(_), None) => self.normal.pop_front(),
            (None, Some(_)) => self.control.pop_front(),
            (None, None) => None,
        };
        if matches!(next.as_ref().map(|(_, command)| command), Some(WriterCommand::Flush { .. })) {
            self.flush_pending = false;
        }
        next
    }

    fn take_dropped_overflows(&mut self) -> Vec<(ObservationIds, u64, u64)> {
        let buckets = std::mem::take(&mut self.dropped_overflows);
        self.dropped_normal = 0;
        buckets
            .into_iter()
            .map(|((conversation_id, root_turn_id, generation), count)| {
                (
                    ObservationIds {
                        conversation_id: if conversation_id.is_empty() {
                            None
                        } else {
                            Some(conversation_id)
                        },
                        root_turn_id: if root_turn_id.is_empty() {
                            None
                        } else {
                            Some(root_turn_id)
                        },
                        ..ObservationIds::default()
                    },
                    generation,
                    count,
                )
            })
            .collect()
    }

    fn restore_dropped_overflows(&mut self, buckets: Vec<(ObservationIds, u64, u64)>) {
        for (ids, generation, count) in buckets {
            if count == 0 {
                continue;
            }
            self.dropped_normal = self.dropped_normal.saturating_add(count);
            *self
                .dropped_overflows
                .entry(Self::overflow_key(&ids, generation))
                .or_insert(0) += count;
        }
    }

    #[cfg(test)]
    fn take_dropped_normal(&mut self) -> u64 {
        let dropped = self.dropped_normal;
        self.dropped_normal = 0;
        self.dropped_overflows.clear();
        dropped
    }

    #[cfg(test)]
    fn restore_dropped_normal(&mut self, count: u64) {
        self.dropped_normal = self.dropped_normal.saturating_add(count);
    }

    fn drop_pending_for_conversation(&mut self, conversation_id: &str) {
        let matches = |command: &WriterCommand| match command {
            WriterCommand::Event { ids, .. } => {
                ids.conversation_id.as_deref() == Some(conversation_id)
            }
            _ => false,
        };
        self.normal.retain(|(_, command)| !matches(command));
        self.control.retain(|(_, command)| !matches(command));
    }

    fn drop_all_events(&mut self) {
        self.normal.clear();
        self.control.retain(|(_, command)| command.is_lifecycle());
    }
}

struct ConversationWriter {
    dir: PathBuf,
    file: BufWriter<File>,
    current_size: u64,
    last_used: Instant,
}

struct SeqEntry {
    seq: u64,
    last_used: Instant,
}

struct RecorderInner {
    seq_by_boundary: HashMap<String, SeqEntry>,
    writers: HashMap<String, ConversationWriter>,
}

#[cfg(test)]
struct WriteGate {
    open: Mutex<bool>,
    cv: Condvar,
}

#[cfg(test)]
impl WriteGate {
    fn closed() -> Arc<Self> {
        Arc::new(Self {
            open: Mutex::new(false),
            cv: Condvar::new(),
        })
    }

    fn wait_if_closed(&self) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            open = self.cv.wait(open).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn release(&self) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        *open = true;
        self.cv.notify_all();
    }
}

struct LifecycleState {
    tombstones: HashSet<String>,
    generation: HashMap<String, u64>,
    started_root_turns: HashSet<String>,
    ended_root_turns: HashSet<String>,
}

impl LifecycleState {
    fn current_generation(&self, conversation_id: Option<&str>) -> u64 {
        conversation_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .and_then(|id| self.generation.get(id).copied())
            .unwrap_or(0)
    }

    fn is_tombstoned(&self, conversation_id: Option<&str>) -> bool {
        conversation_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .is_some_and(|id| self.tombstones.contains(id))
    }

    fn drop_turn_claims_for_conversation(&mut self, conversation_id: &str) {
        let folder = folder_id(Some(conversation_id));
        let prefix = format!("{folder}\0");
        self.started_root_turns
            .retain(|key| key != &folder && !key.starts_with(&prefix));
        self.ended_root_turns
            .retain(|key| key != &folder && !key.starts_with(&prefix));
    }
}

struct WriterShared {
    root: PathBuf,
    rotate_bytes: u64,
    queue: Mutex<DualQueue>,
    cv: Condvar,
    inner: Mutex<RecorderInner>,
    health: Mutex<RecorderHealth>,
    lifecycle: Mutex<LifecycleState>,
    shutdown: AtomicBool,
    started: Instant,
    last_event_elapsed_ms: AtomicU64,
    last_gc_unix_secs: AtomicU64,
    last_reconciled_total: AtomicU64,
    bytes_written_since: AtomicU64,
    gc_estimate_ready: AtomicBool,
    #[cfg(test)]
    write_gate: Mutex<Option<Arc<WriteGate>>>,
    #[cfg(test)]
    suppress_background_gc: AtomicBool,
    #[cfg(test)]
    gc_quota_override: Mutex<Option<(u64, u64, u64)>>,
    #[cfg(test)]
    gc_idle_ms_override: Mutex<Option<u64>>,
}

/// Process-wide interned JSONL recorder. `shared(data_dir)` returns the same Arc
/// for the same data directory (factory and Hub). Not a business context.
pub struct ObservationRecorder {
    data_dir: PathBuf,
    shared: Arc<WriterShared>,
    enabled: AtomicBool,
    writer: Mutex<Option<JoinHandle<()>>>,
}

impl ObservationRecorder {
    /// Interned recorder for `data_dir`. Capture stays on; developer mode gates HTTP reads.
    pub fn shared(data_dir: impl AsRef<Path>) -> Arc<Self> {
        let key = normalize_data_dir(data_dir.as_ref());
        let mut registry = shared_registry().lock().unwrap_or_else(|e| e.into_inner());
        registry.retain(|_, weak| weak.strong_count() > 0);
        if let Some(existing) = registry.get(&key).and_then(|weak| weak.upgrade()) {
            return existing;
        }
        let recorder = Arc::new(Self::create(key.clone(), ROTATE_BYTES));
        registry.insert(key, Arc::downgrade(&recorder));
        recorder
    }

    /// Isolated instance for tests (not interned).
    pub fn isolated(data_dir: impl AsRef<Path>) -> Arc<Self> {
        Arc::new(Self::create(normalize_data_dir(data_dir.as_ref()), ROTATE_BYTES))
    }

    #[cfg(test)]
    pub fn isolated_with_rotate(data_dir: impl AsRef<Path>, rotate_bytes: u64) -> Arc<Self> {
        Arc::new(Self::create(normalize_data_dir(data_dir.as_ref()), rotate_bytes))
    }

    fn create(data_dir: PathBuf, rotate_bytes: u64) -> Self {
        let root = data_dir.join("diagnostics").join("observation");
        let shared = Arc::new(WriterShared {
            root,
            rotate_bytes,
            queue: Mutex::new(DualQueue::new(MAX_QUEUE_EVENTS, MAX_CONTROL_EVENTS)),
            cv: Condvar::new(),
            inner: Mutex::new(RecorderInner {
                seq_by_boundary: HashMap::new(),
                writers: HashMap::new(),
            }),
            health: Mutex::new(RecorderHealth::default()),
            lifecycle: Mutex::new(LifecycleState {
                tombstones: HashSet::new(),
                generation: HashMap::new(),
                started_root_turns: HashSet::new(),
                ended_root_turns: HashSet::new(),
            }),
            shutdown: AtomicBool::new(false),
            started: Instant::now(),
            last_event_elapsed_ms: AtomicU64::new(0),
            last_gc_unix_secs: AtomicU64::new(0),
            last_reconciled_total: AtomicU64::new(0),
            bytes_written_since: AtomicU64::new(0),
            gc_estimate_ready: AtomicBool::new(false),
            #[cfg(test)]
            write_gate: Mutex::new(None),
            #[cfg(test)]
            suppress_background_gc: AtomicBool::new(true),
            #[cfg(test)]
            gc_quota_override: Mutex::new(None),
            #[cfg(test)]
            gc_idle_ms_override: Mutex::new(None),
        });
        let thread_shared = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("observation-writer".into())
            .spawn(move || {
                let panicked = catch_unwind(AssertUnwindSafe(|| writer_loop(&thread_shared)));
                if panicked.is_err() {
                    set_health(
                        &thread_shared,
                        RecorderHealthStatus::WriterDisconnected,
                        Some("observation writer panicked".into()),
                    );
                }
            })
            .expect("spawn observation writer");
        Self {
            data_dir,
            shared,
            enabled: AtomicBool::new(true),
            writer: Mutex::new(Some(handle)),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn root(&self) -> &Path {
        &self.shared.root
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn health(&self) -> RecorderHealth {
        self.shared
            .health
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// Queue one captured event. Never waits for disk. Returns `Ok(None)`
    /// because `event_seq` is assigned after dequeue on the writer thread.
    pub fn emit(
        &self,
        event_type: &str,
        ids: &ObservationIds,
        payload: Value,
    ) -> Result<Option<ObservationEvent>, RecorderError> {
        if !self.is_enabled() {
            return Ok(None);
        }
        if self.shared.shutdown.load(Ordering::Relaxed) {
            set_health(
                &self.shared,
                RecorderHealthStatus::WriterDisconnected,
                Some("observation writer is shut down".into()),
            );
            return Ok(None);
        }
        let payload = prepare_payload(ids, payload);
        let conversation_id = ids.conversation_id.as_deref().map(str::trim).filter(|id| !id.is_empty());
        let generation = {
            let lifecycle = self
                .shared
                .lifecycle
                .lock()
                .map_err(|_| RecorderError::LockPoisoned)?;
            if lifecycle.is_tombstoned(conversation_id) {
                return Ok(None);
            }
            lifecycle.current_generation(conversation_id)
        };
        let command = WriterCommand::Event {
            event_type: event_type.to_owned(),
            ids: ids.clone(),
            payload,
            generation,
        };
        let _ = self.try_send(command);
        Ok(None)
    }

    pub fn emit_gap(
        &self,
        ids: &ObservationIds,
        reason: &str,
        from_seq: Option<u64>,
        to_seq: Option<u64>,
        lost_count: Option<u64>,
    ) -> Result<Option<ObservationEvent>, RecorderError> {
        self.emit(
            EVENT_OBSERVATION_GAP,
            ids,
            serde_json::json!({
                "reason": reason,
                "from_seq": from_seq,
                "to_seq": to_seq,
                "lost_count": lost_count,
            }),
        )
    }

    pub fn read_events(&self, conversation_id: Option<&str>) -> Result<Vec<ObservationEvent>, RecorderError> {
        self.flush_for_read();
        let dir = conversation_dir(&self.shared.root, conversation_id);
        read_dir_events(&dir, None)
    }

    pub fn read_events_for_turn(
        &self,
        conversation_id: Option<&str>,
        root_turn_id: &str,
    ) -> Result<Vec<ObservationEvent>, RecorderError> {
        self.flush_for_read();
        let dir = conversation_dir(&self.shared.root, conversation_id);
        read_dir_events_for_turn(&dir, root_turn_id)
    }

    /// Newest `limit` projected turns, reading JSONL files newest-first and
    /// dropping the oldest in-window turn when older files remain unread.
    pub fn read_events_for_latest_turns(
        &self,
        conversation_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ObservationEvent>, RecorderError> {
        self.flush_for_read();
        let dir = conversation_dir(&self.shared.root, conversation_id);
        read_dir_events_latest_turns(&dir, limit.max(1))
    }

    /// Stream JSONL into a summary fold without retaining event payloads.
    pub fn read_summary(
        &self,
        conversation_id: Option<&str>,
    ) -> Result<ObservationSummary, RecorderError> {
        self.flush_for_read();
        let dir = conversation_dir(&self.shared.root, conversation_id);
        fold_dir_summary(&dir)
    }

    /// Close the writer and delete `{root}/{sanitize(conversation_id)}/`.
    ///
    /// Used by conversation reset / clear / delete. Blank ids are ignored so
    /// this never removes the process-level folder.
    pub fn remove_conversation(&self, conversation_id: &str) -> Result<(), RecorderError> {
        let trimmed = conversation_id.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let folder = folder_id(Some(trimmed));
        if folder == PROCESS_BOUNDARY_ID {
            return Ok(());
        }
        {
            let mut lifecycle = self
                .shared
                .lifecycle
                .lock()
                .map_err(|_| RecorderError::LockPoisoned)?;
            lifecycle.tombstones.insert(trimmed.to_owned());
            lifecycle.drop_turn_claims_for_conversation(trimmed);
            let mut queue = self.shared.queue.lock().map_err(|_| RecorderError::LockPoisoned)?;
            queue.drop_pending_for_conversation(trimmed);
        }
        let (tx, rx) = mpsc::channel();
        self.try_send(WriterCommand::DeleteConversation {
            conversation_id: trimmed.to_owned(),
            ack: Some(tx),
        })?;
        wait_ack(rx)
    }

    /// Clear one conversation's observation files and bump generation so the
    /// same id can keep recording. Not a permanent tombstone.
    pub fn clear_conversation(&self, conversation_id: &str) -> Result<(), RecorderError> {
        let trimmed = conversation_id.trim();
        if trimmed.is_empty() || folder_id(Some(trimmed)) == PROCESS_BOUNDARY_ID {
            return Ok(());
        }
        {
            let mut lifecycle = self
                .shared
                .lifecycle
                .lock()
                .map_err(|_| RecorderError::LockPoisoned)?;
            let next = lifecycle.current_generation(Some(trimmed)).saturating_add(1);
            lifecycle.generation.insert(trimmed.to_owned(), next);
            lifecycle.tombstones.remove(trimmed);
            lifecycle.drop_turn_claims_for_conversation(trimmed);
            let mut queue = self.shared.queue.lock().map_err(|_| RecorderError::LockPoisoned)?;
            queue.drop_pending_for_conversation(trimmed);
        }
        let (tx, rx) = mpsc::channel();
        self.try_send(WriterCommand::ClearConversation {
            conversation_id: trimmed.to_owned(),
            ack: Some(tx),
        })?;
        wait_ack(rx)
    }

    /// Drop every pending event, wipe the observation root, and keep the writer.
    pub fn reset_all(&self) -> Result<(), RecorderError> {
        {
            let mut lifecycle = self
                .shared
                .lifecycle
                .lock()
                .map_err(|_| RecorderError::LockPoisoned)?;
            lifecycle.tombstones.clear();
            lifecycle.generation.clear();
            lifecycle.started_root_turns.clear();
            lifecycle.ended_root_turns.clear();
            let mut queue = self.shared.queue.lock().map_err(|_| RecorderError::LockPoisoned)?;
            queue.drop_all_events();
        }
        let (tx, rx) = mpsc::channel();
        self.try_send(WriterCommand::ResetAll { ack: Some(tx) })?;
        wait_ack(rx)
    }

    /// First writer for this conversation/`root_turn_id` wins. Shared across
    /// interned sessions so failover rebuilds do not emit a second `turn/start`.
    pub fn claim_turn_start(&self, ids: &ObservationIds) -> bool {
        claim_turn_boundary(&self.shared, ids, true)
    }

    /// First writer for this conversation/`root_turn_id` wins. Shared across
    /// interned sessions so a rebuilt runtime cannot emit a second `turn/end`.
    pub fn claim_turn_end(&self, ids: &ObservationIds) -> bool {
        claim_turn_boundary(&self.shared, ids, false)
    }

    pub fn gc(&self) {
        let _ = self.flush_blocking();
    }

    fn try_send(&self, command: WriterCommand) -> Result<EnqueueOutcome, RecorderError> {
        let mut queue = self.shared.queue.lock().map_err(|_| RecorderError::LockPoisoned)?;
        let outcome = queue.try_enqueue(command);
        drop(queue);
        self.shared.cv.notify_one();
        if matches!(
            outcome,
            EnqueueOutcome::DroppedNormal | EnqueueOutcome::DroppedControl
        ) {
            set_health(
                &self.shared,
                RecorderHealthStatus::QueueDropped,
                Some("observation writer queue overflow".into()),
            );
        }
        Ok(outcome)
    }

    fn flush_for_read(&self) {
        if self.shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        if self.try_send(WriterCommand::Flush { ack: Some(tx) }).is_err() {
            return;
        }
        match rx.recv_timeout(READ_FLUSH_TIMEOUT) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => {
                tracing::debug!("observation read proceeding without flush ack");
            }
            Err(RecvTimeoutError::Disconnected) => {
                tracing::debug!("observation writer disconnected during read flush");
            }
        }
    }

    fn flush_blocking(&self) -> Result<(), RecorderError> {
        if self.shared.shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }
        let (tx, rx) = mpsc::channel();
        self.try_send(WriterCommand::Flush { ack: Some(tx) })?;
        wait_ack(rx)
    }

    fn shutdown_and_join(&self) {
        #[cfg(test)]
        if let Ok(gate) = self.shared.write_gate.lock() {
            if let Some(gate) = gate.as_ref() {
                gate.release();
            }
        }
        if self.shared.shutdown.swap(true, Ordering::SeqCst) {
            if let Ok(mut guard) = self.writer.lock() {
                if let Some(handle) = guard.take() {
                    let _ = handle.join();
                }
            }
            return;
        }
        let (tx, rx) = mpsc::channel();
        if let Ok(mut queue) = self.shared.queue.lock() {
            let _ = queue.try_enqueue(WriterCommand::Shutdown { ack: Some(tx) });
        }
        self.shared.cv.notify_one();
        let _ = rx.recv_timeout(WRITER_ACK_TIMEOUT);
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }

    #[cfg(test)]
    fn isolated_with_write_gate(data_dir: impl AsRef<Path>) -> (Arc<Self>, Arc<WriteGate>) {
        let recorder = Self::isolated(data_dir);
        let gate = WriteGate::closed();
        *recorder
            .shared
            .write_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&gate));
        (recorder, gate)
    }

    #[cfg(test)]
    fn gc_quota_with_limit(&self, max_bytes: u64) {
        self.gc_quota_with_limits(max_bytes, max_bytes);
    }

    #[cfg(test)]
    fn gc_quota_with_limits(&self, high_bytes: u64, low_bytes: u64) {
        let _ = self.flush_blocking();
        let active: HashSet<PathBuf> = self
            .shared
            .inner
            .lock()
            .map(|inner| {
                inner
                    .writers
                    .values()
                    .map(|writer| writer.dir.join(EVENTS_FILE))
                    .collect()
            })
            .unwrap_or_default();
        gc_quota_except(&self.shared.root, &active, high_bytes, low_bytes);
    }

    #[cfg(test)]
    fn set_gc_quota_override(&self, high_bytes: u64, low_bytes: u64, emergency_bytes: u64) {
        *self
            .shared
            .gc_quota_override
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some((high_bytes, low_bytes, emergency_bytes));
    }

    #[cfg(test)]
    fn set_gc_idle_ms(&self, idle_ms: u64) {
        *self
            .shared
            .gc_idle_ms_override
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(idle_ms);
    }

    #[cfg(test)]
    fn run_maybe_gc(&self) {
        let _ = self.flush_blocking();
        maybe_gc_writer_inner(&self.shared);
    }

    #[cfg(test)]
    fn last_gc_unix_secs(&self) -> u64 {
        self.shared.last_gc_unix_secs.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn reconciled_total(&self) -> u64 {
        self.shared.last_reconciled_total.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn writer_count(&self) -> usize {
        let _ = self.flush_blocking();
        self.shared
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .writers
            .len()
    }

    #[cfg(test)]
    fn seq_count(&self) -> usize {
        let _ = self.flush_blocking();
        self.shared
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .seq_by_boundary
            .len()
    }

    #[cfg(test)]
    fn prune_idle_for_test(&self, idle: Duration) {
        let _ = self.flush_blocking();
        let mut inner = self
            .shared
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        prune_idle_maps_with(&mut inner, Instant::now(), idle, idle, usize::MAX, usize::MAX);
    }
}

impl Drop for ObservationRecorder {
    fn drop(&mut self) {
        self.shutdown_and_join();
    }
}

fn shared_registry() -> &'static Mutex<HashMap<PathBuf, Weak<ObservationRecorder>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Weak<ObservationRecorder>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize_data_dir(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn wait_ack(rx: mpsc::Receiver<()>) -> Result<(), RecorderError> {
    match rx.recv_timeout(WRITER_ACK_TIMEOUT) {
        Ok(()) => Ok(()),
        Err(RecvTimeoutError::Timeout) => Err(RecorderError::Io(std::io::Error::other(
            "observation writer ack timed out",
        ))),
        Err(RecvTimeoutError::Disconnected) => Err(RecorderError::Io(std::io::Error::other(
            "observation writer disconnected",
        ))),
    }
}

fn set_health(shared: &WriterShared, status: RecorderHealthStatus, last_error: Option<String>) {
    let Ok(mut health) = shared.health.lock() else {
        return;
    };
    let rank = |status: RecorderHealthStatus| match status {
        RecorderHealthStatus::Healthy => 0,
        RecorderHealthStatus::QueueDropped => 1,
        RecorderHealthStatus::StorageError => 2,
        RecorderHealthStatus::WriterDisconnected => 3,
    };
    if rank(status) >= rank(health.status) {
        health.status = status;
        if last_error.is_some() {
            health.last_error = last_error;
        }
    }
}

fn recover_health_after_success(shared: &WriterShared) {
    let Ok(mut health) = shared.health.lock() else {
        return;
    };
    match health.status {
        RecorderHealthStatus::QueueDropped | RecorderHealthStatus::StorageError => {
            health.status = RecorderHealthStatus::Healthy;
            health.last_error = None;
        }
        RecorderHealthStatus::Healthy | RecorderHealthStatus::WriterDisconnected => {}
    }
}

fn writer_loop(shared: &WriterShared) {
    let mut unflushed = 0usize;
    let mut last_flush = Instant::now();
    loop {
        let command = next_writer_command(shared);
        match command {
            Some((_, mut command)) => {
                if matches!(command, WriterCommand::Shutdown { .. }) {
                    drain_remaining_events(shared, &mut unflushed);
                    let _ = flush_all_writers(shared);
                    if let Some(ack) = command.take_ack() {
                        let _ = ack.send(());
                    }
                    return;
                }
                if let Err(error) = handle_writer_command(shared, command, &mut unflushed) {
                    set_health(
                        shared,
                        RecorderHealthStatus::StorageError,
                        Some(error.to_string()),
                    );
                }
                if unflushed >= WRITER_FLUSH_BATCH {
                    if let Err(error) = flush_all_writers(shared) {
                        set_health(
                            shared,
                            RecorderHealthStatus::StorageError,
                            Some(error.to_string()),
                        );
                    } else {
                        unflushed = 0;
                        last_flush = Instant::now();
                    }
                }
            }
            None => {
                if unflushed > 0 || last_flush.elapsed() >= WRITER_FLUSH_INTERVAL {
                    if let Err(error) = flush_all_writers(shared) {
                        set_health(
                            shared,
                            RecorderHealthStatus::StorageError,
                            Some(error.to_string()),
                        );
                    } else {
                        unflushed = 0;
                        last_flush = Instant::now();
                    }
                }
                maybe_gc_writer(shared);
            }
        }
        write_overflow_gap(shared, &mut unflushed);
    }
}

fn next_writer_command(shared: &WriterShared) -> Option<(u64, WriterCommand)> {
    let Ok(mut queue) = shared.queue.lock() else {
        return None;
    };
    loop {
        if let Some(command) = queue.dequeue_next() {
            return Some(command);
        }
        let (guard, timeout) = shared
            .cv
            .wait_timeout(queue, WRITER_FLUSH_INTERVAL)
            .unwrap_or_else(|e| e.into_inner());
        queue = guard;
        if timeout.timed_out() {
            return None;
        }
    }
}

fn drain_remaining_events(shared: &WriterShared, unflushed: &mut usize) {
    loop {
        let next = shared.queue.lock().ok().and_then(|mut queue| queue.dequeue_next());
        let Some((_, command)) = next else {
            return;
        };
        if matches!(command, WriterCommand::Shutdown { .. }) {
            continue;
        }
        let _ = handle_writer_command(shared, command, unflushed);
    }
}

fn handle_writer_command(
    shared: &WriterShared,
    command: WriterCommand,
    unflushed: &mut usize,
) -> Result<(), RecorderError> {
    #[cfg(test)]
    wait_write_gate(shared);

    match command {
        WriterCommand::Event {
            event_type,
            ids,
            payload,
            generation,
        } => {
            if persist_event(shared, &event_type, &ids, payload, generation)? {
                if ids
                    .conversation_id
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|id| !id.is_empty())
                {
                    recover_health_after_success(shared);
                }
                *unflushed += 1;
            }
        }
        WriterCommand::Flush { ack } => {
            flush_all_writers(shared)?;
            *unflushed = 0;
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
            if let Ok(mut queue) = shared.queue.lock() {
                for extra in queue.take_extra_flush_acks() {
                    let _ = extra.send(());
                }
            }
        }
        WriterCommand::DeleteConversation {
            conversation_id,
            ack,
        } => {
            if let Ok(mut queue) = shared.queue.lock() {
                queue.drop_pending_for_conversation(&conversation_id);
            }
            delete_conversation_dir(shared, &conversation_id)?;
            flush_all_writers(shared)?;
            reconcile_gc_estimate(shared);
            *unflushed = 0;
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
        }
        WriterCommand::ClearConversation {
            conversation_id,
            ack,
        } => {
            delete_conversation_dir(shared, &conversation_id)?;
            flush_all_writers(shared)?;
            reconcile_gc_estimate(shared);
            *unflushed = 0;
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
        }
        WriterCommand::ResetAll { ack } => {
            reset_all_observations(shared)?;
            reconcile_gc_estimate(shared);
            *unflushed = 0;
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
        }
        WriterCommand::Shutdown { ack } => {
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn wait_write_gate(shared: &WriterShared) {
    let gate = shared
        .write_gate
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(gate) = gate {
        gate.wait_if_closed();
    }
}

fn persist_event(
    shared: &WriterShared,
    event_type: &str,
    ids: &ObservationIds,
    payload: Value,
    generation: u64,
) -> Result<bool, RecorderError> {
    let conversation_id = ids.conversation_id.as_deref().map(str::trim).filter(|id| !id.is_empty());
    {
        let lifecycle = shared.lifecycle.lock().map_err(|_| RecorderError::LockPoisoned)?;
        if lifecycle.is_tombstoned(conversation_id) {
            return Ok(false);
        }
        if lifecycle.current_generation(conversation_id) != generation {
            return Ok(false);
        }
    }
    let boundary = ids_from_payload(&payload).boundary_id();
    let now = Utc::now();
    let mut inner = shared.inner.lock().map_err(|_| RecorderError::LockPoisoned)?;
    prune_idle_maps(&mut inner, Instant::now());
    let event_seq = next_seq(&mut inner, &shared.root, ids, &boundary)?;
    let event = ObservationEvent {
        schema_version: OBSERVATION_SCHEMA_VERSION,
        event_type: event_type.to_owned(),
        event_seq,
        timestamp: now.to_rfc3339_opts(SecondsFormat::Millis, true),
        timestamp_ms: u64::try_from(now.timestamp_millis()).unwrap_or(0),
        payload,
    };
    let written = write_event(&mut inner, &shared.root, ids, &event, shared.rotate_bytes)?;
    prune_idle_maps(&mut inner, Instant::now());
    drop(inner);
    note_persisted_bytes(shared, written);
    Ok(true)
}

fn write_overflow_gap(shared: &WriterShared, unflushed: &mut usize) {
    let buckets = shared
        .queue
        .lock()
        .ok()
        .map(|mut queue| queue.take_dropped_overflows())
        .unwrap_or_default();
    if buckets.is_empty() {
        return;
    }
    let mut failed = Vec::new();
    for (ids, generation, count) in buckets {
        if count == 0 {
            continue;
        }
        let payload = serde_json::json!({
            "ids": ids,
            "reason": "writer_queue_overflow",
            "lost_count": count,
        });
        match persist_event(shared, EVENT_OBSERVATION_GAP, &ids, payload, generation) {
            Ok(true) => *unflushed += 1,
            Ok(false) | Err(_) => failed.push((ids, generation, count)),
        }
    }
    if !failed.is_empty() {
        if let Ok(mut queue) = shared.queue.lock() {
            queue.restore_dropped_overflows(failed);
        }
    }
}

fn delete_conversation_dir(shared: &WriterShared, conversation_id: &str) -> Result<(), RecorderError> {
    let folder = folder_id(Some(conversation_id));
    if folder == PROCESS_BOUNDARY_ID {
        return Ok(());
    }
    {
        let mut inner = shared.inner.lock().map_err(|_| RecorderError::LockPoisoned)?;
        inner.writers.remove(&folder);
        drop_seq_for_folder(&mut inner, &folder);
    }
    let dir = conversation_dir(&shared.root, Some(conversation_id));
    match fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn reset_all_observations(shared: &WriterShared) -> Result<(), RecorderError> {
    {
        let mut inner = shared.inner.lock().map_err(|_| RecorderError::LockPoisoned)?;
        inner.writers.clear();
        inner.seq_by_boundary.clear();
    }
    match fs::remove_dir_all(&shared.root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn flush_all_writers(shared: &WriterShared) -> Result<(), RecorderError> {
    let mut inner = shared.inner.lock().map_err(|_| RecorderError::LockPoisoned)?;
    for writer in inner.writers.values_mut() {
        writer.file.flush()?;
    }
    Ok(())
}

fn gc_is_idle(started_elapsed_ms: u64, last_event_elapsed_ms: u64, idle_ms: u64) -> bool {
    started_elapsed_ms.saturating_sub(last_event_elapsed_ms) >= idle_ms
}

fn gc_should_run(idle: bool, interval_elapsed: bool, emergency: bool) -> bool {
    emergency || (idle && interval_elapsed)
}

fn note_persisted_bytes(shared: &WriterShared, bytes: u64) {
    let elapsed = shared
        .started
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    shared
        .last_event_elapsed_ms
        .store(elapsed.max(1), Ordering::Relaxed);
    shared
        .bytes_written_since
        .fetch_add(bytes, Ordering::Relaxed);
}

fn gc_idle_threshold_ms(shared: &WriterShared) -> u64 {
    #[cfg(test)]
    {
        if let Some(ms) = *shared
            .gc_idle_ms_override
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            return ms;
        }
    }
    let _ = shared;
    GC_IDLE_SECS.saturating_mul(1000)
}

fn gc_quota_limits(shared: &WriterShared) -> (u64, u64, u64) {
    #[cfg(test)]
    {
        if let Some(limits) = *shared
            .gc_quota_override
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            return limits;
        }
    }
    let _ = shared;
    (
        GC_QUOTA_HIGH_BYTES,
        GC_QUOTA_LOW_BYTES,
        GC_QUOTA_EMERGENCY_BYTES,
    )
}

fn reconcile_gc_estimate(shared: &WriterShared) {
    let total: u64 = collect_observation_files(&shared.root)
        .iter()
        .map(|(_, size, _)| *size)
        .sum();
    shared
        .last_reconciled_total
        .store(total, Ordering::Relaxed);
    shared.bytes_written_since.store(0, Ordering::Relaxed);
    shared.gc_estimate_ready.store(true, Ordering::Relaxed);
}

fn unix_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn maybe_gc_writer(shared: &WriterShared) {
    #[cfg(test)]
    if shared.suppress_background_gc.load(Ordering::Relaxed) {
        return;
    }
    maybe_gc_writer_inner(shared);
}

fn maybe_gc_writer_inner(shared: &WriterShared) {
    let (high, low, emergency_limit) = gc_quota_limits(shared);
    let started_elapsed_ms = shared
        .started
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let last_event_elapsed_ms = shared.last_event_elapsed_ms.load(Ordering::Relaxed);
    let idle = gc_is_idle(
        started_elapsed_ms,
        last_event_elapsed_ms,
        gc_idle_threshold_ms(shared),
    );
    // Queue-empty gap only (not the persist path). Count files already on disk so
    // emergency GC sees leftover size after a process restart.
    if !shared.gc_estimate_ready.load(Ordering::Relaxed) {
        reconcile_gc_estimate(shared);
    }
    let now = unix_secs_now();
    let prev = shared.last_gc_unix_secs.load(Ordering::Relaxed);
    let interval_elapsed = prev == 0 || now.saturating_sub(prev) >= GC_INTERVAL_SECS;
    let wrote_since = shared.bytes_written_since.load(Ordering::Relaxed) > 0;
    let estimated = shared
        .last_reconciled_total
        .load(Ordering::Relaxed)
        .saturating_add(shared.bytes_written_since.load(Ordering::Relaxed));
    let emergency =
        estimated >= emergency_limit && (interval_elapsed || wrote_since || prev == 0);
    if !gc_should_run(idle, interval_elapsed, emergency) {
        return;
    }
    if shared
        .last_gc_unix_secs
        .compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let active: HashSet<PathBuf> = shared
        .inner
        .lock()
        .map(|inner| {
            inner
                .writers
                .values()
                .map(|writer| writer.dir.join(EVENTS_FILE))
                .collect()
        })
        .unwrap_or_default();
    gc_quota_except(&shared.root, &active, high, low);
    reconcile_gc_estimate(shared);
    if let Ok(mut inner) = shared.inner.lock() {
        prune_idle_maps(&mut inner, Instant::now());
    }
}

fn prepare_payload(ids: &ObservationIds, payload: Value) -> Value {
    let mut payload = match payload {
        Value::Object(map) => Value::Object(map),
        other => serde_json::json!({ "value": other }),
    };
    if let Some(obj) = payload.as_object_mut() {
        if !obj.contains_key("ids") {
            if let Ok(value) = serde_json::to_value(ids) {
                obj.insert("ids".into(), value);
            }
        }
    }
    capture_and_size_cap(payload)
}

fn seq_map_key(folder: &str, boundary: &str) -> String {
    format!("{folder}\0{boundary}")
}

fn turn_claim_key(conversation_id: Option<&str>, root_turn_id: &str) -> String {
    format!("{}\0{}", folder_id(conversation_id), root_turn_id.trim())
}

fn claim_turn_boundary(shared: &WriterShared, ids: &ObservationIds, start: bool) -> bool {
    let Some(root) = ids
        .root_turn_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let key = turn_claim_key(ids.conversation_id.as_deref(), root);
    let Ok(mut lifecycle) = shared.lifecycle.lock() else {
        return false;
    };
    if start {
        lifecycle.started_root_turns.insert(key)
    } else {
        lifecycle.ended_root_turns.insert(key)
    }
}

fn drop_seq_for_folder(inner: &mut RecorderInner, folder: &str) {
    let prefix = format!("{folder}\0");
    inner
        .seq_by_boundary
        .retain(|key, _| key != folder && !key.starts_with(&prefix));
}

fn next_seq(
    inner: &mut RecorderInner,
    root: &Path,
    ids: &ObservationIds,
    boundary: &str,
) -> Result<u64, RecorderError> {
    let folder = folder_id(ids.conversation_id.as_deref());
    let key = seq_map_key(&folder, boundary);
    if !inner.seq_by_boundary.contains_key(&key) {
        let dir = conversation_dir(root, ids.conversation_id.as_deref());
        let loaded = load_max_seqs(&dir)?;
        let loaded_at = Instant::now();
        for (loaded_boundary, seq) in loaded {
            inner.seq_by_boundary.entry(seq_map_key(&folder, &loaded_boundary)).or_insert(SeqEntry {
                seq,
                last_used: loaded_at,
            });
        }
    }
    let now = Instant::now();
    let entry = inner
        .seq_by_boundary
        .entry(key)
        .or_insert(SeqEntry {
            seq: 0,
            last_used: now,
        });
    entry.seq += 1;
    entry.last_used = now;
    Ok(entry.seq)
}

fn write_event(
    inner: &mut RecorderInner,
    root: &Path,
    ids: &ObservationIds,
    event: &ObservationEvent,
    rotate_bytes: u64,
) -> Result<u64, RecorderError> {
    let folder = folder_id(ids.conversation_id.as_deref());
    if !inner.writers.contains_key(&folder) {
        let dir = conversation_dir(root, ids.conversation_id.as_deref());
        inner.writers.insert(folder.clone(), open_writer(&dir)?);
    }
    let writer = inner
        .writers
        .get_mut(&folder)
        .ok_or_else(|| std::io::Error::other("missing observation writer"))?;
    writer.last_used = Instant::now();
    if writer.current_size >= rotate_bytes {
        rotate_writer(writer, rotate_bytes)?;
    }
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    writer.file.write_all(&line)?;
    let written = line.len() as u64;
    writer.current_size += written;
    if writer.current_size >= rotate_bytes {
        rotate_writer(writer, rotate_bytes)?;
    }
    Ok(written)
}

fn open_writer(dir: &Path) -> Result<ConversationWriter, RecorderError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(EVENTS_FILE);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let current_size = file.metadata()?.len();
    Ok(ConversationWriter {
        dir: dir.to_path_buf(),
        file: BufWriter::with_capacity(BUFWRITER_CAPACITY, file),
        current_size,
        last_used: Instant::now(),
    })
}

fn prune_idle_maps(inner: &mut RecorderInner, now: Instant) {
    prune_idle_maps_with(
        inner,
        now,
        WRITER_IDLE,
        SEQ_IDLE,
        MAX_WRITERS,
        MAX_SEQ_BOUNDARIES,
    );
}

fn prune_idle_maps_with(
    inner: &mut RecorderInner,
    now: Instant,
    writer_idle: Duration,
    seq_idle: Duration,
    max_writers: usize,
    max_seq_boundaries: usize,
) {
    inner.writers.retain(|_, writer| {
        writer.dir.exists() && now.saturating_duration_since(writer.last_used) < writer_idle
    });
    inner.seq_by_boundary.retain(|_, entry| {
        now.saturating_duration_since(entry.last_used) < seq_idle
    });
    evict_lru_over_cap(&mut inner.writers, max_writers, |writer| writer.last_used);
    evict_lru_over_cap(&mut inner.seq_by_boundary, max_seq_boundaries, |entry| {
        entry.last_used
    });
}

fn evict_lru_over_cap<V>(
    map: &mut HashMap<String, V>,
    max: usize,
    last_used: impl Fn(&V) -> Instant,
) {
    if map.len() <= max {
        return;
    }
    let mut keys: Vec<(Instant, String)> = map
        .iter()
        .map(|(key, value)| (last_used(value), key.clone()))
        .collect();
    keys.sort_by_key(|(used, _)| *used);
    let drop_n = map.len() - max;
    for (_, key) in keys.into_iter().take(drop_n) {
        map.remove(&key);
    }
}

fn rotate_writer(writer: &mut ConversationWriter, _rotate_bytes: u64) -> Result<(), RecorderError> {
    let current = writer.dir.join(EVENTS_FILE);
    let next = next_rotate_index(&writer.dir)?;
    let dest = writer.dir.join(format!("events.{next}.jsonl"));
    writer.file.flush()?;
    let previous = std::mem::replace(
        &mut writer.file,
        BufWriter::with_capacity(
            BUFWRITER_CAPACITY,
            File::create(writer.dir.join(".rotate-placeholder"))?,
        ),
    );
    drop(previous.into_inner().map_err(|error| error.into_error())?);
    if current.exists() {
        fs::rename(&current, &dest)?;
    }
    let _ = fs::remove_file(writer.dir.join(".rotate-placeholder"));
    let file = OpenOptions::new().create(true).append(true).open(&current)?;
    writer.current_size = file.metadata()?.len();
    writer.file = BufWriter::with_capacity(BUFWRITER_CAPACITY, file);
    Ok(())
}

fn next_rotate_index(dir: &Path) -> Result<u32, RecorderError> {
    let mut max = 0_u32;
    if dir.exists() {
        for entry in fs::read_dir(dir)? {
            let name = entry?.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(n) = parse_rotated_name(name) {
                max = max.max(n);
            }
        }
    }
    Ok(max + 1)
}

fn parse_rotated_name(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("events.")?.strip_suffix(".jsonl")?;
    rest.parse().ok()
}

fn folder_id(conversation_id: Option<&str>) -> String {
    match conversation_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => sanitize_path_segment(id),
        None => PROCESS_BOUNDARY_ID.to_owned(),
    }
}

fn conversation_dir(root: &Path, conversation_id: Option<&str>) -> PathBuf {
    root.join(folder_id(conversation_id))
}

fn load_max_seqs(dir: &Path) -> Result<HashMap<String, u64>, RecorderError> {
    let mut max = HashMap::new();
    for event in read_dir_events(dir, None)? {
        let boundary = ids_from_payload(&event.payload).boundary_id();
        let entry = max.entry(boundary).or_insert(0);
        *entry = (*entry).max(event.event_seq);
    }
    Ok(max)
}

fn list_event_files(dir: &Path) -> Result<(Vec<(u32, PathBuf)>, Option<PathBuf>), RecorderError> {
    if !dir.exists() {
        return Ok((Vec::new(), None));
    }
    let mut rotated = Vec::new();
    let mut current = None;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == EVENTS_FILE {
            current = Some(entry.path());
        } else if let Some(n) = parse_rotated_name(name) {
            rotated.push((n, entry.path()));
        }
    }
    rotated.sort_by_key(|(n, _)| *n);
    Ok((rotated, current))
}

fn event_files_newest_first(dir: &Path) -> Result<Vec<PathBuf>, RecorderError> {
    let (rotated, current) = list_event_files(dir)?;
    let mut files = Vec::new();
    if let Some(path) = current {
        files.push(path);
    }
    for (_, path) in rotated.into_iter().rev() {
        files.push(path);
    }
    Ok(files)
}

fn read_dir_events(
    dir: &Path,
    root_turn_id: Option<&str>,
) -> Result<Vec<ObservationEvent>, RecorderError> {
    let (rotated, current) = list_event_files(dir)?;
    let mut events = Vec::new();
    for (_, path) in rotated {
        read_jsonl_file(&path, &mut events, root_turn_id)?;
    }
    if let Some(path) = current {
        read_jsonl_file(&path, &mut events, root_turn_id)?;
    }
    Ok(events)
}

fn read_dir_events_for_turn(
    dir: &Path,
    root_turn_id: &str,
) -> Result<Vec<ObservationEvent>, RecorderError> {
    let mut newest_first_chunks: Vec<Vec<ObservationEvent>> = Vec::new();
    let mut found = false;
    for path in event_files_newest_first(dir)? {
        let mut chunk = Vec::new();
        read_jsonl_file(&path, &mut chunk, Some(root_turn_id))?;
        if chunk.is_empty() {
            if found {
                break;
            }
            continue;
        }
        found = true;
        newest_first_chunks.push(chunk);
    }
    Ok(newest_first_chunks.into_iter().rev().flatten().collect())
}

fn read_dir_events_latest_turns(
    dir: &Path,
    limit: usize,
) -> Result<Vec<ObservationEvent>, RecorderError> {
    let files = event_files_newest_first(dir)?;
    let mut newest_first_chunks: Vec<Vec<ObservationEvent>> = Vec::new();
    for (index, path) in files.iter().enumerate() {
        let mut chunk = Vec::new();
        read_jsonl_file(path, &mut chunk, None)?;
        newest_first_chunks.push(chunk);
        let unread_older = index + 1 < files.len();
        let turns = project_event_refs(newest_first_chunks.iter().rev().flatten());
        let complete = if unread_older {
            turns.len().saturating_sub(1)
        } else {
            turns.len()
        };
        if complete >= limit {
            let keep: HashSet<String> = turns
                .iter()
                .rev()
                .take(limit)
                .map(|turn| turn.root_turn_id.clone())
                .collect();
            return Ok(newest_first_chunks
                .into_iter()
                .rev()
                .flatten()
                .filter(|event| keep.iter().any(|id| event_belongs_to_turn(event, id)))
                .collect());
        }
    }
    Ok(newest_first_chunks.into_iter().rev().flatten().collect())
}

fn jsonl_corrupt_gap(last: Option<&ObservationEvent>, lost_count: u64) -> ObservationEvent {
    let ids = last
        .map(|event| ids_from_payload(&event.payload))
        .unwrap_or_default();
    let timestamp = last
        .map(|event| event.timestamp.clone())
        .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
    let timestamp_ms = last.map(|event| event.timestamp_ms).unwrap_or(0);
    ObservationEvent::new(
        EVENT_OBSERVATION_GAP,
        last.map(|event| event.event_seq).unwrap_or(0),
        timestamp,
        timestamp_ms,
        serde_json::json!({
            "ids": ids,
            "reason": "jsonl_corrupt",
            "lost_count": lost_count,
        }),
    )
}

fn attach_jsonl_corrupt_gap(
    out: &mut Vec<ObservationEvent>,
    last: Option<&ObservationEvent>,
    lost_count: u64,
    root_turn_id: Option<&str>,
) {
    if lost_count == 0 {
        return;
    }
    let mut gap = jsonl_corrupt_gap(last, lost_count);
    if let Some(turn_id) = root_turn_id {
        let file_had_turn = out.iter().any(|event| event_belongs_to_turn(event, turn_id));
        if !file_had_turn && !event_belongs_to_turn(&gap, turn_id) {
            return;
        }
        if file_had_turn {
            let sample_ids = out
                .iter()
                .rev()
                .find(|event| event_belongs_to_turn(event, turn_id))
                .map(|event| ids_from_payload(&event.payload))
                .unwrap_or_else(|| ObservationIds {
                    root_turn_id: Some(turn_id.to_owned()),
                    ..ObservationIds::default()
                });
            if let Some(obj) = gap.payload.as_object_mut() {
                if let Ok(value) = serde_json::to_value(&sample_ids) {
                    obj.insert("ids".into(), value);
                }
            }
        }
    }
    out.push(gap);
}

fn read_jsonl_file(
    path: &Path,
    out: &mut Vec<ObservationEvent>,
    root_turn_id: Option<&str>,
) -> Result<(), RecorderError> {
    let file = File::open(path)?;
    let mut last_good: Option<ObservationEvent> = None;
    let mut corrupt = 0u64;
    for line in BufReader::new(file).lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match crate::event::read_event(trimmed) {
            Ok(event) => {
                last_good = Some(event.clone());
                if let Some(turn_id) = root_turn_id {
                    if !event_belongs_to_turn(&event, turn_id) {
                        continue;
                    }
                }
                out.push(event);
            }
            Err(error) => {
                corrupt += 1;
                tracing::warn!(path = %path.display(), %error, "observation JSONL line is corrupt");
            }
        }
    }
    attach_jsonl_corrupt_gap(
        out,
        last_good.as_ref(),
        corrupt,
        root_turn_id,
    );
    Ok(())
}

fn fold_dir_summary(dir: &Path) -> Result<ObservationSummary, RecorderError> {
    let mut fold = ObservationSummaryFold::default();
    let (rotated, current) = list_event_files(dir)?;
    for (_, path) in rotated {
        fold_jsonl_file(&path, &mut fold)?;
    }
    if let Some(path) = current {
        fold_jsonl_file(&path, &mut fold)?;
    }
    Ok(fold.finish())
}

fn fold_jsonl_file(path: &Path, fold: &mut ObservationSummaryFold) -> Result<(), RecorderError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let mut last_good: Option<ObservationEvent> = None;
    let mut corrupt = 0u64;
    for line in BufReader::new(file).lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match crate::event::read_event(trimmed) {
            Ok(event) => {
                last_good = Some(event.clone());
                fold.observe(&event);
            }
            Err(error) => {
                corrupt += 1;
                tracing::warn!(path = %path.display(), %error, "observation JSONL line is corrupt");
            }
        }
    }
    if corrupt > 0 {
        fold.observe(&jsonl_corrupt_gap(last_good.as_ref(), corrupt));
    }
    Ok(())
}

fn collect_observation_files(root: &Path) -> Vec<(SystemTime, u64, PathBuf)> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(children) = fs::read_dir(&path) {
                for child in children.flatten() {
                    let child_path = child.path();
                    if child_path.is_file() {
                        if let Some(meta) = file_size_mtime(&child_path) {
                            files.push((meta.0, meta.1, child_path));
                        }
                    }
                }
            }
        } else if path.is_file() {
            if let Some(meta) = file_size_mtime(&path) {
                files.push((meta.0, meta.1, path));
            }
        }
    }
    files
}

fn file_size_mtime(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn gc_quota_except(root: &Path, skip: &HashSet<PathBuf>, high_bytes: u64, low_bytes: u64) {
    let mut files = collect_observation_files(root);
    let mut total: u64 = files.iter().map(|(_, size, _)| *size).sum();
    if total <= high_bytes {
        return;
    }
    let target = low_bytes.min(high_bytes);
    files.sort_by_key(|(mtime, _, _)| *mtime);
    for (_, size, path) in files {
        if total <= target {
            break;
        }
        if skip.contains(&path) {
            continue;
        }
        if remove_observation_file(&path) {
            total = total.saturating_sub(size);
        }
    }
}

fn remove_observation_file(path: &Path) -> bool {
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "observation GC could not remove a file"
            );
            false
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::MAX_EVENT_BYTES;
    use crate::event::{EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE};
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc;

    fn ids(turn: &str) -> ObservationIds {
        ids_in("conv-a", turn)
    }

    fn ids_in(conversation: &str, turn: &str) -> ObservationIds {
        ObservationIds {
            conversation_id: Some(conversation.into()),
            root_turn_id: Some(turn.into()),
            msg_id: Some("m1".into()),
            ..ObservationIds::default()
        }
    }

    fn ids_with_call(turn: &str, model_call: &str) -> ObservationIds {
        ObservationIds {
            model_call_id: Some(model_call.into()),
            ..ids(turn)
        }
    }

    #[test]
    fn writes_are_enabled_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        assert!(recorder.is_enabled());
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"ok": true}))
            .unwrap();
        assert!(!recorder.read_events(Some("conv-a")).unwrap().is_empty());
    }

    #[test]
    fn set_enabled_false_skips_writes() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(false);
        let written = recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"ok": true}))
            .unwrap();
        assert!(written.is_none());
        assert!(recorder.read_events(Some("conv-a")).unwrap().is_empty());
    }

    #[test]
    fn shared_interns_same_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let a = ObservationRecorder::shared(dir.path());
        let b = ObservationRecorder::shared(dir.path());
        assert!(Arc::ptr_eq(&a, &b));
        let isolated = ObservationRecorder::isolated(dir.path());
        assert!(!Arc::ptr_eq(&a, &isolated));
    }

    #[test]
    fn persist_applies_capture_before_disk() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        recorder
            .emit(
                EVENT_LLM_REQUEST,
                &ids("t1"),
                json!({
                    "request": {
                        "messages": [{
                            "content": [{
                                "type": "image",
                                "media_type": "image/png",
                                "data": png
                            }]
                        }]
                    }
                }),
            )
            .unwrap();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(events.len(), 1);
        let disk = fs::read_to_string(
            recorder
                .root()
                .join("conv-a")
                .join(EVENTS_FILE),
        )
        .unwrap();
        assert!(!disk.contains(png));
        assert!(!disk.contains("iVBORw0KGgo"));
        assert!(disk.contains("binary_payload"));
    }

    #[test]
    fn rotate_moves_events_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated_with_rotate(dir.path(), 200);
        recorder.set_enabled(true);
        for i in 0..8 {
            recorder
                .emit(
                    EVENT_LLM_REQUEST,
                    &ids("t-rotate"),
                    json!({ "n": i, "pad": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx" }),
                )
                .unwrap();
        }
        recorder.flush_blocking().unwrap();
        let conv = recorder.root().join("conv-a");
        let rotated = fs::read_dir(&conv)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with("events.") && n != EVENTS_FILE)
            })
            .count();
        assert!(rotated >= 1, "expected at least one rotated file");
        let all = recorder.read_events(Some("conv-a")).unwrap();
        assert!(all.len() >= 8);
    }

    #[test]
    fn event_seq_is_per_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-a"), json!({}))
            .unwrap();
        recorder
            .emit(EVENT_LLM_RESPONSE, &ids("turn-a"), json!({}))
            .unwrap();
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-b"), json!({}))
            .unwrap();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        let a: Vec<u64> = events
            .iter()
            .filter(|e| ids_from_payload(&e.payload).root_turn_id.as_deref() == Some("turn-a"))
            .map(|e| e.event_seq)
            .collect();
        let b: Vec<u64> = events
            .iter()
            .filter(|e| ids_from_payload(&e.payload).root_turn_id.as_deref() == Some("turn-b"))
            .map(|e| e.event_seq)
            .collect();
        assert_eq!(a, vec![1, 2]);
        assert_eq!(b, vec![1]);
        let only_b = recorder
            .read_events_for_turn(Some("conv-a"), "turn-b")
            .unwrap();
        assert_eq!(only_b.len(), 1);
        assert_eq!(
            ids_from_payload(&only_b[0].payload).root_turn_id.as_deref(),
            Some("turn-b")
        );
    }

    #[test]
    fn quota_gc_skips_active_events_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"keep": true}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        let conv = recorder.root().join("conv-a");
        let active = conv.join(EVENTS_FILE);
        let rotated = conv.join("events.1.jsonl");
        fs::write(&rotated, "x".repeat(4096)).unwrap();
        recorder.gc_quota_with_limit(512);
        assert!(active.is_file(), "active segment must survive quota GC");
        assert!(!rotated.exists(), "oldest non-active segment should be deleted");
        assert!(!recorder.read_events(Some("conv-a")).unwrap().is_empty());
    }

    #[test]
    fn quota_constants_keep_200_mib_hysteresis() {
        const MIB: u64 = 1024 * 1024;
        assert_eq!(GC_QUOTA_HIGH_BYTES, MAX_TOTAL_OBSERVATION_BYTES);
        assert_eq!(GC_QUOTA_LOW_BYTES, GC_QUOTA_HIGH_BYTES - 200 * MIB);
        assert_eq!(GC_QUOTA_EMERGENCY_BYTES, GC_QUOTA_HIGH_BYTES + 200 * MIB);
        assert_eq!(GC_IDLE_SECS, 30);
    }

    #[test]
    fn gc_should_run_idle_interval_and_emergency() {
        assert!(
            !gc_should_run(false, false, false),
            "recent writes without emergency must not scan"
        );
        assert!(
            !gc_should_run(false, true, false),
            "interval alone is not enough while writes are in flight"
        );
        assert!(!gc_should_run(true, false, false), "idle without interval waits");
        assert!(gc_should_run(true, true, false), "idle plus interval scans");
        assert!(
            gc_should_run(false, false, true),
            "emergency scans even right after a write"
        );
        assert!(gc_should_run(true, false, true));
    }

    #[test]
    fn gc_is_idle_requires_quiet_window() {
        assert!(!gc_is_idle(10_000, 9_000, 30_000));
        assert!(gc_is_idle(40_000, 5_000, 30_000));
        assert!(gc_is_idle(30_000, 0, 30_000));
    }

    fn observation_total(root: &Path) -> u64 {
        collect_observation_files(root)
            .iter()
            .map(|(_, size, _)| *size)
            .sum()
    }

    fn set_mtime(path: &Path, mtime: SystemTime) {
        OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .expect("set mtime for quota ordering");
    }

    #[test]
    fn quota_gc_below_high_keeps_files() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"keep": true}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        let rotated = recorder.root().join("conv-a").join("events.1.jsonl");
        fs::write(&rotated, "x".repeat(4096)).unwrap();
        recorder.gc_quota_with_limits(10_000, 1_000);
        assert!(rotated.is_file(), "files under the high watermark must stay");
    }

    #[test]
    fn quota_gc_over_high_deletes_to_low_water() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"keep": true}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        let conv = recorder.root().join("conv-a");
        let oldest = conv.join("events.1.jsonl");
        let middle = conv.join("events.2.jsonl");
        let newest = conv.join("events.3.jsonl");
        fs::write(&oldest, "o".repeat(500)).unwrap();
        fs::write(&middle, "m".repeat(500)).unwrap();
        fs::write(&newest, "n".repeat(500)).unwrap();
        let now = SystemTime::now();
        set_mtime(&oldest, now - Duration::from_secs(30));
        set_mtime(&middle, now - Duration::from_secs(20));
        set_mtime(&newest, now - Duration::from_secs(10));
        recorder.gc_quota_with_limits(1_200, 900);
        assert!(!oldest.exists(), "oldest non-active segment should be deleted");
        assert!(!middle.exists(), "quota should keep deleting until the low watermark");
        assert!(newest.is_file(), "newest rotated segment should survive the hysteresis pass");
        assert!(
            observation_total(recorder.root()) <= 900,
            "quota GC must land at or below the low watermark, not just the high"
        );
    }

    #[test]
    fn maybe_gc_skips_recent_writes_until_idle_or_emergency() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder.set_gc_quota_override(1_000, 800, 5_000);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"keep": true}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        let rotated = recorder.root().join("conv-a").join("events.1.jsonl");
        fs::write(&rotated, "x".repeat(1_500)).unwrap();
        recorder.run_maybe_gc();
        assert!(
            rotated.is_file(),
            "must not trim to low water during a live write below the emergency watermark"
        );
        assert_eq!(recorder.last_gc_unix_secs(), 0);

        recorder.set_gc_idle_ms(0);
        recorder.run_maybe_gc();
        assert!(
            !rotated.exists(),
            "idle plus elapsed interval should run quota GC"
        );
        assert_ne!(recorder.last_gc_unix_secs(), 0);
    }

    #[test]
    fn maybe_gc_emergency_counts_existing_files_before_idle() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder.set_gc_quota_override(1_000, 800, 1_200);
        let conv = recorder.root().join("conv-a");
        fs::create_dir_all(&conv).unwrap();
        let oldest = conv.join("events.1.jsonl");
        let newer = conv.join("events.2.jsonl");
        fs::write(&oldest, "o".repeat(800)).unwrap();
        fs::write(&newer, "n".repeat(500)).unwrap();
        let now = SystemTime::now();
        set_mtime(&oldest, now - Duration::from_secs(30));
        set_mtime(&newer, now - Duration::from_secs(10));
        recorder.run_maybe_gc();
        assert!(
            !oldest.exists(),
            "first writer gap must count leftover files so emergency GC can run"
        );
        assert_ne!(recorder.last_gc_unix_secs(), 0);
        assert!(observation_total(recorder.root()) <= 800);
    }

    #[test]
    fn maybe_gc_emergency_after_clear_does_not_wait_for_idle() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder.set_gc_quota_override(1_000, 800, 1_200);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"keep": true}))
            .unwrap();
        recorder
            .emit(
                EVENT_LLM_REQUEST,
                &ids_in("conv-b", "t1"),
                json!({"keep": true}),
            )
            .unwrap();
        recorder.flush_blocking().unwrap();
        let keep = recorder.root().join("conv-a").join("events.1.jsonl");
        let oldest = recorder.root().join("conv-a").join("events.2.jsonl");
        let other = recorder.root().join("conv-b").join("events.1.jsonl");
        fs::write(&keep, "k".repeat(500)).unwrap();
        fs::write(&oldest, "o".repeat(800)).unwrap();
        fs::write(&other, "b".repeat(500)).unwrap();
        let now = SystemTime::now();
        set_mtime(&oldest, now - Duration::from_secs(30));
        set_mtime(&keep, now - Duration::from_secs(10));
        recorder.clear_conversation("conv-b").unwrap();
        assert!(
            recorder.reconciled_total() >= 1_200,
            "clear must recount remaining files instead of zeroing the estimate"
        );
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t2"), json!({"keep": true}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        recorder.run_maybe_gc();
        assert!(!oldest.exists(), "emergency GC must run without a 30s idle");
        assert_ne!(recorder.last_gc_unix_secs(), 0);
        assert!(observation_total(recorder.root()) <= 800);
    }

    #[test]
    fn read_summary_does_not_require_project_turns() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(
                EVENT_TURN_START,
                &ids("t1"),
                json!({ "prompt_preview": "hi" }),
            )
            .unwrap();
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({}))
            .unwrap();
        recorder
            .emit(
                crate::event::EVENT_TURN_END,
                &ids("t1"),
                json!({ "status": "completed", "elapsed_ms": 9 }),
            )
            .unwrap();
        let summary = recorder.read_summary(Some("conv-a")).unwrap();
        assert_eq!(summary.turn_count, 1);
        assert_eq!(summary.active_duration_ms, 9);
        assert_eq!(summary.coverage, crate::COVERAGE_RETAINED_OBSERVATION_HISTORY);
    }

    #[test]
    fn sanitize_path_segment_encodes_unsafe_chars() {
        assert_eq!(sanitize_path_segment("abc-DEF_09"), "abc-DEF_09");
        assert_eq!(sanitize_path_segment("../evil/x"), "%2E%2E%2Fevil%2Fx");
        assert_ne!(
            sanitize_path_segment("foo.bar"),
            sanitize_path_segment("foobar")
        );
        assert_eq!(sanitize_path_segment("!!!"), "%21%21%21");
        assert_eq!(sanitize_path_segment("   "), "unknown");
    }

    #[test]
    fn overflow_gap_lands_in_the_conversation_turn() {
        let dir = tempfile::tempdir().unwrap();
        let (recorder, gate) = ObservationRecorder::isolated_with_write_gate(dir.path());
        for _ in 0..(MAX_QUEUE_EVENTS + 8) {
            recorder
                .emit(EVENT_LLM_REQUEST, &ids("t-overflow"), json!({ "ok": true }))
                .unwrap();
        }
        assert_eq!(recorder.health().status, RecorderHealthStatus::QueueDropped);
        gate.release();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        let gaps: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == EVENT_OBSERVATION_GAP)
            .collect();
        assert!(
            !gaps.is_empty(),
            "overflow must write observation/gap into the conversation JSONL, got {events:?}"
        );
        assert_eq!(gaps[0].payload["reason"], "writer_queue_overflow");
        let gap_ids = ids_from_payload(&gaps[0].payload);
        assert_eq!(gap_ids.conversation_id.as_deref(), Some("conv-a"));
        assert_eq!(gap_ids.root_turn_id.as_deref(), Some("t-overflow"));
        let turns = crate::project::project_turns(&events);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].integrity, crate::event::Integrity::Degraded);
    }

    #[test]
    fn corrupt_jsonl_line_degrades_turn_integrity() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder
            .emit(
                EVENT_TURN_START,
                &ids("t-corrupt"),
                json!({ "prompt_preview": "hi" }),
            )
            .unwrap();
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-corrupt"), json!({}))
            .unwrap();
        recorder
            .emit(
                crate::event::EVENT_TURN_END,
                &ids("t-corrupt"),
                json!({ "status": "completed", "elapsed_ms": 4 }),
            )
            .unwrap();
        recorder.flush_blocking().unwrap();

        let jsonl = recorder.root().join("conv-a").join(EVENTS_FILE);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&jsonl)
            .unwrap()
            .write_all(b"{not-json\n")
            .unwrap();

        let events = recorder.read_events(Some("conv-a")).unwrap();
        assert!(
            events.iter().any(|event| {
                event.event_type == EVENT_OBSERVATION_GAP
                    && event.payload["reason"] == "jsonl_corrupt"
            }),
            "corrupt line must surface as observation/gap, got {events:?}"
        );
        let turns = crate::project::project_turns(&events);
        assert_eq!(turns[0].integrity, crate::event::Integrity::Degraded);
        let summary = recorder.read_summary(Some("conv-a")).unwrap();
        assert_eq!(summary.integrity, crate::event::Integrity::Degraded);
    }

    #[test]
    fn latest_turns_keeps_newest_limit() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        for index in 1..=5 {
            let turn = format!("turn-{index}");
            recorder
                .emit(EVENT_LLM_REQUEST, &ids(&turn), json!({}))
                .unwrap();
            recorder
                .emit(EVENT_LLM_RESPONSE, &ids(&turn), json!({}))
                .unwrap();
        }
        let events = recorder
            .read_events_for_latest_turns(Some("conv-a"), 2)
            .unwrap();
        let turns = crate::project::project_turns(&events);
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn.root_turn_id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-4", "turn-5"]
        );
    }

    #[test]
    fn idle_prune_drops_maps_and_seq_resumes_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-a"), json!({}))
            .unwrap();
        recorder
            .emit(EVENT_LLM_RESPONSE, &ids("turn-a"), json!({}))
            .unwrap();
        assert!(recorder.writer_count() >= 1);
        assert!(recorder.seq_count() >= 1);

        recorder.prune_idle_for_test(Duration::ZERO);
        assert_eq!(recorder.writer_count(), 0);
        assert_eq!(recorder.seq_count(), 0);

        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-a"), json!({}))
            .unwrap();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        let seqs: Vec<u64> = events
            .iter()
            .filter(|event| ids_from_payload(&event.payload).root_turn_id.as_deref() == Some("turn-a"))
            .map(|event| event.event_seq)
            .collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[test]
    fn writer_cap_evicts_oldest_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        for index in 0..(MAX_WRITERS + 8) {
            let ids = ObservationIds {
                conversation_id: Some(format!("conv-{index}")),
                root_turn_id: Some(format!("turn-{index}")),
                ..ObservationIds::default()
            };
            recorder
                .emit(EVENT_LLM_REQUEST, &ids, json!({}))
                .unwrap();
        }
        assert!(recorder.writer_count() <= MAX_WRITERS);
        assert!(recorder.seq_count() <= MAX_SEQ_BOUNDARIES);
    }

    #[test]
    fn remove_conversation_tombstones_and_rejects_later_writes() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-a"), json!({}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        let conv_dir = recorder.root().join("conv-a");
        assert!(conv_dir.join(EVENTS_FILE).is_file());

        recorder.remove_conversation("conv-a").unwrap();
        assert!(!conv_dir.exists());
        assert!(recorder.read_events(Some("conv-a")).unwrap().is_empty());

        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-b"), json!({}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        assert!(recorder.read_events(Some("conv-a")).unwrap().is_empty());
        assert!(!conv_dir.exists());
    }

    #[test]
    fn remove_conversation_ignores_blank_ids_and_process_folder() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(
                EVENT_LLM_REQUEST,
                &ObservationIds::default(),
                json!({}),
            )
            .unwrap();
        recorder.flush_blocking().unwrap();
        let process_dir = recorder.root().join(PROCESS_BOUNDARY_ID);
        assert!(process_dir.join(EVENTS_FILE).is_file());

        recorder.remove_conversation("").unwrap();
        recorder.remove_conversation("   ").unwrap();
        assert!(process_dir.join(EVENTS_FILE).is_file());
        assert_eq!(recorder.read_events(None).unwrap().len(), 1);
    }

    fn queued_event(event_type: &str, turn: &str) -> WriterCommand {
        WriterCommand::Event {
            event_type: event_type.to_owned(),
            ids: ids(turn),
            payload: json!({ "ids": ids(turn) }),
            generation: 0,
        }
    }

    fn drain_types(queue: &mut DualQueue) -> Vec<String> {
        let mut types = Vec::new();
        while let Some((_, command)) = queue.dequeue_next() {
            match command {
                WriterCommand::Event { event_type, .. } => types.push(event_type),
                other => types.push(format!("{other:?}")),
            }
        }
        types
    }

    #[test]
    fn enqueue_order_merge_persists_turn_end_after_prior_normal() {
        let mut queue = DualQueue::new(MAX_QUEUE_EVENTS, MAX_CONTROL_EVENTS);
        assert_eq!(
            queue.try_enqueue(queued_event(EVENT_LLM_REQUEST, "t1")),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            queue.try_enqueue(queued_event("turn/end", "t1")),
            EnqueueOutcome::Queued
        );
        let first = queue.dequeue_next().expect("normal first");
        let second = queue.dequeue_next().expect("turn/end second");
        match &first.1 {
            WriterCommand::Event { event_type, payload, .. } => {
                assert_eq!(event_type, EVENT_LLM_REQUEST);
                assert!(payload.get("enqueue_order").is_none());
            }
            other => panic!("expected llm/request, got {other:?}"),
        }
        match &second.1 {
            WriterCommand::Event { event_type, payload, .. } => {
                assert_eq!(event_type, "turn/end");
                assert!(payload.get("enqueue_order").is_none());
            }
            other => panic!("expected turn/end, got {other:?}"),
        }
        assert!(first.0 < second.0);
    }

    #[test]
    fn second_flush_coalesces_into_pending_flush() {
        let mut queue = DualQueue::new(8, 8);
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();
        assert_eq!(
            queue.try_enqueue(WriterCommand::Flush { ack: Some(tx1) }),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            queue.try_enqueue(WriterCommand::Flush { ack: Some(tx2) }),
            EnqueueOutcome::Queued
        );
        let first = queue.dequeue_next().expect("one flush command");
        assert!(
            queue.dequeue_next().is_none(),
            "a second Flush must merge into the pending ACK list"
        );
        match first.1 {
            WriterCommand::Flush { ack: Some(ack) } => {
                let _ = ack.send(());
            }
            other => panic!("expected Flush, got {other:?}"),
        }
        let extras = queue.take_extra_flush_acks();
        assert_eq!(extras.len(), 1);
        let _ = extras[0].send(());
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn restore_dropped_normal_returns_count_to_the_queue() {
        let mut queue = DualQueue::new(1, 1);
        assert_eq!(
            queue.try_enqueue(queued_event(EVENT_LLM_REQUEST, "kept")),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            queue.try_enqueue(queued_event(EVENT_LLM_REQUEST, "lost")),
            EnqueueOutcome::DroppedNormal
        );
        let dropped = queue.take_dropped_normal();
        assert_eq!(dropped, 1);
        assert_eq!(queue.take_dropped_normal(), 0);
        queue.restore_dropped_normal(dropped);
        assert_eq!(queue.take_dropped_normal(), 1);
    }

    #[test]
    fn control_enqueue_drops_normal_not_turn_end_when_normal_is_full() {
        let mut queue = DualQueue::new(2, 2);
        assert_eq!(
            queue.try_enqueue(queued_event(EVENT_LLM_REQUEST, "a")),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            queue.try_enqueue(queued_event(EVENT_LLM_RESPONSE, "a")),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            queue.try_enqueue(queued_event(EVENT_LLM_REQUEST, "dropped")),
            EnqueueOutcome::DroppedNormal
        );
        assert_eq!(
            queue.try_enqueue(queued_event("turn/end", "a")),
            EnqueueOutcome::Queued
        );
        let types = drain_types(&mut queue);
        assert!(types.contains(&"turn/end".to_owned()));
        assert!(!types.iter().any(|event_type| event_type == "dropped"));
        assert_eq!(queue.dropped_normal, 2);
    }

    fn oversized_request_payload() -> Value {
        let tools: Vec<Value> = (0..48)
            .map(|index| {
                json!({
                    "name": format!("tool_{index}"),
                    "description": "d".repeat(64),
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "a": { "type": "string", "description": "x".repeat(900) },
                            "b": { "type": "string", "description": "y".repeat(900) },
                            "c": { "type": "string", "description": "z".repeat(900) },
                        }
                    }
                })
            })
            .collect();
        json!({
            "call_kind": "agent_turn",
            "request": {
                "model": "coding-agent",
                "system": "sys",
                "messages": [{ "role": "user", "content": "hi" }],
                "tools": tools
            }
        })
    }

    #[test]
    fn event_size_limit_omit_enqueues_without_gap_or_degraded_integrity() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let payload = oversized_request_payload();
        let raw_len = serde_json::to_vec(&payload).unwrap().len();
        assert!(
            raw_len > MAX_EVENT_BYTES,
            "fixture must exceed MAX_EVENT_BYTES, got {raw_len}"
        );
        recorder
            .emit(EVENT_LLM_REQUEST, &ids_with_call("t-size", "mc-size"), payload)
            .unwrap();
        recorder
            .emit(
                EVENT_LLM_RESPONSE,
                &ids_with_call("t-size", "mc-size"),
                json!({ "text": "ok" }),
            )
            .unwrap();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        assert!(events.iter().all(|event| event.event_type != EVENT_OBSERVATION_GAP));
        let request = events
            .iter()
            .find(|event| event.event_type == EVENT_LLM_REQUEST)
            .expect("llm/request");
        let serialized = serde_json::to_vec(request).unwrap();
        assert!(serialized.len() <= MAX_EVENT_BYTES + 2048);
        let request_json = request.payload.to_string();
        assert!(request_json.contains("event_size_limit"));
        assert!(
            request.payload.get("enqueue_order").is_none(),
            "enqueue_order must not enter JSONL"
        );
        let turns = crate::project::project_turns(&events);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].integrity, crate::event::Integrity::Complete);
    }

    #[test]
    fn emit_does_not_wait_when_writer_is_latched() {
        let dir = tempfile::tempdir().unwrap();
        let (recorder, gate) = ObservationRecorder::isolated_with_write_gate(dir.path());
        recorder.set_enabled(true);
        let started = Instant::now();
        let written = recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-latch"), json!({ "ok": true }))
            .unwrap();
        let elapsed = started.elapsed();
        assert!(written.is_none(), "emit must not return a persisted event");
        assert!(
            elapsed < Duration::from_millis(50),
            "producer must not wait for a blocked writer, elapsed={elapsed:?}"
        );
        assert_eq!(recorder.health().status, RecorderHealthStatus::Healthy);
        gate.release();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EVENT_LLM_REQUEST);
        assert_eq!(events[0].event_seq, 1);
    }

    #[test]
    fn successful_persist_recovers_queue_dropped_health() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        set_health(
            &recorder.shared,
            RecorderHealthStatus::QueueDropped,
            Some("observation writer queue overflow".into()),
        );
        assert_eq!(recorder.health().status, RecorderHealthStatus::QueueDropped);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-recover"), json!({ "ok": true }))
            .unwrap();
        let _ = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(recorder.health().status, RecorderHealthStatus::Healthy);
        assert_eq!(recorder.health().last_error, None);
    }

    #[test]
    fn tombstoned_persist_does_not_recover_queue_dropped_health() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-before"), json!({}))
            .unwrap();
        recorder.remove_conversation("conv-a").unwrap();
        set_health(
            &recorder.shared,
            RecorderHealthStatus::QueueDropped,
            Some("observation writer queue overflow".into()),
        );
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-after"), json!({}))
            .unwrap();
        let _ = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(recorder.health().status, RecorderHealthStatus::QueueDropped);
    }

    #[test]
    fn shutdown_joins_writer_thread() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-stop"), json!({}))
            .unwrap();
        recorder.shutdown_and_join();
        assert!(recorder.shared.shutdown.load(Ordering::SeqCst));
        assert!(
            recorder
                .writer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        );
    }

    #[test]
    fn pending_event_cannot_resurrect_deleted_directory() {
        let dir = tempfile::tempdir().unwrap();
        let (recorder, gate) = ObservationRecorder::isolated_with_write_gate(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(
                EVENT_LLM_REQUEST,
                &ObservationIds {
                    conversation_id: Some("conv-blocker".into()),
                    root_turn_id: Some("t-blocker".into()),
                    ..ObservationIds::default()
                },
                json!({ "block": true }),
            )
            .unwrap();
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-pending"), json!({ "pending": true }))
            .unwrap();
        let recorder_for_delete = Arc::clone(&recorder);
        let delete = std::thread::spawn(move || recorder_for_delete.remove_conversation("conv-a"));
        std::thread::sleep(Duration::from_millis(30));
        gate.release();
        delete.join().expect("delete thread").unwrap();
        recorder.flush_blocking().unwrap();
        assert!(!recorder.root().join("conv-a").exists());
        assert!(recorder.read_events(Some("conv-a")).unwrap().is_empty());
        let blocker = recorder.read_events(Some("conv-blocker")).unwrap();
        assert_eq!(blocker.len(), 1);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t-after-delete"), json!({}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        assert!(recorder.read_events(Some("conv-a")).unwrap().is_empty());
        assert!(!recorder.root().join("conv-a").exists());
    }

    #[test]
    fn clear_conversation_allows_same_id_to_write_again() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-old"), json!({ "old": true }))
            .unwrap();
        recorder.flush_blocking().unwrap();
        assert_eq!(recorder.read_events(Some("conv-a")).unwrap().len(), 1);

        recorder.clear_conversation("conv-a").unwrap();
        assert!(recorder.read_events(Some("conv-a")).unwrap().is_empty());

        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-new"), json!({ "new": true }))
            .unwrap();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            ids_from_payload(&events[0].payload).root_turn_id.as_deref(),
            Some("turn-new")
        );
        assert_eq!(events[0].event_seq, 1);
    }

    #[test]
    fn clear_conversation_resets_seq_for_reused_turn_id() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-reuse"), json!({ "n": 1 }))
            .unwrap();
        recorder
            .emit(EVENT_LLM_RESPONSE, &ids("turn-reuse"), json!({ "n": 2 }))
            .unwrap();
        let before = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(before.last().map(|event| event.event_seq), Some(2));
        recorder.clear_conversation("conv-a").unwrap();
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-reuse"), json!({ "n": 3 }))
            .unwrap();
        let after = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].event_seq, 1);
    }

    #[test]
    fn turn_start_and_end_claims_are_shared_and_reset_on_clear() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        let turn = ids("turn-claim");
        assert!(recorder.claim_turn_start(&turn));
        assert!(!recorder.claim_turn_start(&turn));
        assert!(recorder.claim_turn_end(&turn));
        assert!(!recorder.claim_turn_end(&turn));
        recorder.clear_conversation("conv-a").unwrap();
        assert!(recorder.claim_turn_start(&turn));
        assert!(recorder.claim_turn_end(&turn));
    }

    #[test]
    fn reset_all_wipes_root_and_keeps_writer() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-a"), json!({}))
            .unwrap();
        recorder.flush_blocking().unwrap();
        assert!(recorder.root().join("conv-a").exists());
        recorder.reset_all().unwrap();
        assert!(!recorder.root().exists());
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("turn-b"), json!({}))
            .unwrap();
        let events = recorder.read_events(Some("conv-a")).unwrap();
        assert_eq!(events.len(), 1);
    }
}

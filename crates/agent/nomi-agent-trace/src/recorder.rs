//! JSONL observation writer under `{data_dir}/diagnostics/observation/`.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use serde_json::Value;

use crate::capture::capture_canonical_request;
use crate::event::{
    ids_from_payload, ObservationEvent, ObservationIds, EVENT_OBSERVATION_GAP,
    OBSERVATION_SCHEMA_VERSION, PROCESS_BOUNDARY_ID,
};
use crate::project::{event_belongs_to_turn, project_event_refs};

fn sanitize_path_segment(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(128)
        .collect();
    if filtered.is_empty() {
        "unknown".to_owned()
    } else {
        filtered
    }
}

pub const OBSERVATION_DIR: &str = "diagnostics/observation";
pub const ROTATE_BYTES: u64 = 48 * 1024 * 1024;
pub const GC_MAX_AGE_DAYS: u64 = 14;
/// Emit path only: skip a full directory walk when GC ran recently.
const GC_INTERVAL_SECS: u64 = 60 * 60;
const WRITER_IDLE: Duration = Duration::from_secs(10 * 60);
const SEQ_IDLE: Duration = Duration::from_secs(10 * 60);
const MAX_WRITERS: usize = 64;
const MAX_SEQ_BOUNDARIES: usize = 4096;
const EVENTS_FILE: &str = "events.jsonl";

#[derive(Debug, thiserror::Error)]
pub enum RecorderError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("lock poisoned")]
    LockPoisoned,
}

struct ConversationWriter {
    dir: PathBuf,
    file: File,
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

/// Process-wide interned JSONL recorder. `shared(data_dir)` returns the same Arc
/// for the same data directory (factory and Hub). Not a business context.
pub struct ObservationRecorder {
    data_dir: PathBuf,
    root: PathBuf,
    rotate_bytes: u64,
    enabled: AtomicBool,
    last_gc_unix_secs: AtomicU64,
    inner: Mutex<RecorderInner>,
}

impl ObservationRecorder {
    /// Interned recorder for `data_dir`. Writes stay off until [`set_enabled`].
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
        Self {
            data_dir,
            root,
            rotate_bytes,
            enabled: AtomicBool::new(false),
            last_gc_unix_secs: AtomicU64::new(0),
            inner: Mutex::new(RecorderInner {
                seq_by_boundary: HashMap::new(),
                writers: HashMap::new(),
            }),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Persist one event after applying capture. No-op when disabled.
    pub fn emit(
        &self,
        event_type: &str,
        ids: &ObservationIds,
        payload: Value,
    ) -> Result<Option<ObservationEvent>, RecorderError> {
        if !self.is_enabled() {
            return Ok(None);
        }
        self.maybe_gc();
        let payload = prepare_payload(ids, payload);
        let boundary = ids_from_payload(&payload).boundary_id();
        let now = Utc::now();
        let mut inner = self.inner.lock().map_err(|_| RecorderError::LockPoisoned)?;
        prune_idle_maps(&mut inner, Instant::now());
        let event_seq = next_seq(&mut inner, &self.root, ids, &boundary)?;
        let event = ObservationEvent {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            event_type: event_type.to_owned(),
            event_seq,
            timestamp: now.to_rfc3339_opts(SecondsFormat::Millis, true),
            timestamp_ms: u64::try_from(now.timestamp_millis()).unwrap_or(0),
            payload,
        };
        write_event(&mut inner, &self.root, ids, &event, self.rotate_bytes)?;
        prune_idle_maps(&mut inner, Instant::now());
        Ok(Some(event))
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
        self.gc();
        let dir = conversation_dir(&self.root, conversation_id);
        read_dir_events(&dir, None)
    }

    pub fn read_events_for_turn(
        &self,
        conversation_id: Option<&str>,
        root_turn_id: &str,
    ) -> Result<Vec<ObservationEvent>, RecorderError> {
        self.gc();
        let dir = conversation_dir(&self.root, conversation_id);
        read_dir_events_for_turn(&dir, root_turn_id)
    }

    /// Newest `limit` projected turns, reading JSONL files newest-first and
    /// dropping the oldest in-window turn when older files remain unread.
    pub fn read_events_for_latest_turns(
        &self,
        conversation_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ObservationEvent>, RecorderError> {
        self.gc();
        let dir = conversation_dir(&self.root, conversation_id);
        read_dir_events_latest_turns(&dir, limit.max(1))
    }

    pub fn gc(&self) {
        let ttl = Duration::from_secs(GC_MAX_AGE_DAYS * 24 * 60 * 60);
        let cutoff = SystemTime::now().checked_sub(ttl).unwrap_or(SystemTime::UNIX_EPOCH);
        gc_older_than(&self.root, cutoff);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        self.last_gc_unix_secs.store(now, Ordering::Relaxed);
        if let Ok(mut inner) = self.inner.lock() {
            prune_idle_maps(&mut inner, Instant::now());
        }
    }

    fn maybe_gc(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let prev = self.last_gc_unix_secs.load(Ordering::Relaxed);
        if prev != 0 && now.saturating_sub(prev) < GC_INTERVAL_SECS {
            return;
        }
        if self
            .last_gc_unix_secs
            .compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            let ttl = Duration::from_secs(GC_MAX_AGE_DAYS * 24 * 60 * 60);
            let cutoff = SystemTime::now()
                .checked_sub(ttl)
                .unwrap_or(SystemTime::UNIX_EPOCH);
            gc_older_than(&self.root, cutoff);
            if let Ok(mut inner) = self.inner.lock() {
                prune_idle_maps(&mut inner, Instant::now());
            }
        }
    }

    #[cfg(test)]
    fn gc_with_cutoff(&self, cutoff: SystemTime) {
        gc_older_than(&self.root, cutoff);
    }

    #[cfg(test)]
    fn writer_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .writers
            .len()
    }

    #[cfg(test)]
    fn seq_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .seq_by_boundary
            .len()
    }

    #[cfg(test)]
    fn prune_idle_for_test(&self, idle: Duration) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        prune_idle_maps_with(&mut inner, Instant::now(), idle, idle, usize::MAX, usize::MAX);
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
    capture_canonical_request(payload)
}

fn next_seq(
    inner: &mut RecorderInner,
    root: &Path,
    ids: &ObservationIds,
    boundary: &str,
) -> Result<u64, RecorderError> {
    if !inner.seq_by_boundary.contains_key(boundary) {
        let dir = conversation_dir(root, ids.conversation_id.as_deref());
        let loaded = load_max_seqs(&dir)?;
        let loaded_at = Instant::now();
        for (key, seq) in loaded {
            inner.seq_by_boundary.entry(key).or_insert(SeqEntry {
                seq,
                last_used: loaded_at,
            });
        }
    }
    let now = Instant::now();
    let entry = inner
        .seq_by_boundary
        .entry(boundary.to_owned())
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
) -> Result<(), RecorderError> {
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
    writer.file.flush()?;
    writer.current_size += line.len() as u64;
    if writer.current_size >= rotate_bytes {
        rotate_writer(writer, rotate_bytes)?;
    }
    Ok(())
}

fn open_writer(dir: &Path) -> Result<ConversationWriter, RecorderError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(EVENTS_FILE);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let current_size = file.metadata()?.len();
    Ok(ConversationWriter {
        dir: dir.to_path_buf(),
        file,
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
    // Close before rename (Windows).
    writer.file = File::create(writer.dir.join(".rotate-placeholder"))?;
    if current.exists() {
        fs::rename(&current, &dest)?;
    }
    let _ = fs::remove_file(writer.dir.join(".rotate-placeholder"));
    writer.file = OpenOptions::new().create(true).append(true).open(&current)?;
    writer.current_size = writer.file.metadata()?.len();
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

fn read_jsonl_file(
    path: &Path,
    out: &mut Vec<ObservationEvent>,
    root_turn_id: Option<&str>,
) -> Result<(), RecorderError> {
    let file = File::open(path)?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match crate::event::read_event(trimmed) {
            Ok(event) => {
                if let Some(turn_id) = root_turn_id {
                    if !event_belongs_to_turn(&event, turn_id) {
                        continue;
                    }
                }
                out.push(event);
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skipping corrupt observation line");
            }
        }
    }
    Ok(())
}

fn gc_older_than(root: &Path, cutoff: SystemTime) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            gc_dir_files(&path, cutoff);
            if dir_is_empty(&path) {
                let _ = fs::remove_dir(&path);
            }
        } else if file_mtime_is_old(&path, cutoff) {
            let _ = fs::remove_file(&path);
        }
    }
}

fn gc_dir_files(dir: &Path, cutoff: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && file_mtime_is_old(&path, cutoff) {
            let _ = fs::remove_file(&path);
        }
    }
}

fn file_mtime_is_old(path: &Path, cutoff: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(|mtime| mtime <= cutoff)
        .unwrap_or(false)
}

fn dir_is_empty(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE};
    use serde_json::json;
    use std::sync::Arc;

    fn ids(turn: &str) -> ObservationIds {
        ObservationIds {
            conversation_id: Some("conv-a".into()),
            root_turn_id: Some(turn.into()),
            msg_id: Some("m1".into()),
            ..ObservationIds::default()
        }
    }

    #[test]
    fn set_enabled_defaults_false_and_skips_writes() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        assert!(!recorder.is_enabled());
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
    fn gc_deletes_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        recorder
            .emit(EVENT_LLM_REQUEST, &ids("t1"), json!({"keep": false}))
            .unwrap();
        let future = SystemTime::now() + Duration::from_secs(60);
        recorder.gc_with_cutoff(future);
        assert!(recorder.read_events(Some("conv-a")).unwrap().is_empty());
    }

    #[test]
    fn sanitize_path_segment_filters() {
        assert_eq!(sanitize_path_segment("abc-DEF_09"), "abc-DEF_09");
        assert_eq!(sanitize_path_segment("../evil/x"), "evilx");
        assert_eq!(sanitize_path_segment("!!!"), "unknown");
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
}

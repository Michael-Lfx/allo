//! File-backed persistence for agent turn traces.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use uuid::Uuid;

use crate::types::{TraceArtifactIndexEntry, TraceIndexEntry, TurnTrace, SCHEMA_VERSION};

/// Errors from [`FileTraceStore`] I/O. Callers may log and continue.
#[derive(Debug, thiserror::Error)]
pub enum TraceStoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("index lock poisoned")]
    LockPoisoned,
}

/// Append-only index + per-turn JSON files under
/// `{data_dir}/diagnostics/agent-traces/`.
///
/// Layout:
/// ```text
/// agent-traces/
///   index.jsonl
///   turns/{safe_conversation_id}/{trace_id}.json
/// ```
#[derive(Debug)]
pub struct FileTraceStore {
    root: PathBuf,
    index_lock: Mutex<()>,
}

impl FileTraceStore {
    /// Root is `{data_dir}/diagnostics/agent-traces/`.
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            root: data_dir
                .as_ref()
                .join("diagnostics")
                .join("agent-traces"),
            index_lock: Mutex::new(()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Persist a finished turn: atomic JSON write + append index line.
    pub fn persist(&self, trace: &TurnTrace) -> Result<(), TraceStoreError> {
        let safe_conv = sanitize_path_segment(&trace.conversation_id);
        let safe_trace = sanitize_path_segment(&trace.trace_id);
        let rel = format!("turns/{safe_conv}/{safe_trace}.json");
        let abs = self.root.join(&rel);

        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(trace)?;
        atomic_write(&abs, &bytes)?;

        let entry = TraceIndexEntry::from_trace(trace, rel);
        self.append_index(&entry)?;
        Ok(())
    }

    /// Newest-first entries for a conversation, capped at `limit`.
    pub fn list_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<TraceIndexEntry>, TraceStoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let all = self.read_index()?;
        let mut matched: Vec<TraceIndexEntry> = all
            .into_iter()
            .filter(|e| e.conversation_id == conversation_id)
            .collect();
        // Append-only → last entries are newest.
        if matched.len() > limit {
            matched = matched.split_off(matched.len() - limit);
        }
        matched.reverse();
        Ok(matched)
    }

    /// Load a turn by `trace_id` via index lookup (falls back to directory scan).
    pub fn get(&self, trace_id: &str) -> Result<Option<TurnTrace>, TraceStoreError> {
        for entry in self.read_index()?.into_iter().rev() {
            if entry.trace_id == trace_id {
                return self.load_relative(&entry.relative_path).map(Some);
            }
        }
        // Fallback: scan turns/*/{trace_id}.json
        let safe = sanitize_path_segment(trace_id);
        let turns_dir = self.root.join("turns");
        if !turns_dir.is_dir() {
            return Ok(None);
        }
        for conv in fs::read_dir(&turns_dir)? {
            let conv = conv?;
            if !conv.file_type()?.is_dir() {
                continue;
            }
            let candidate = conv.path().join(format!("{safe}.json"));
            if candidate.is_file() {
                let bytes = fs::read(&candidate)?;
                let trace: TurnTrace = serde_json::from_slice(&bytes)?;
                if trace.trace_id == trace_id {
                    return Ok(Some(trace));
                }
            }
        }
        Ok(None)
    }

    /// Newest-first index entries across all conversations.
    pub fn list_recent(&self, limit: usize) -> Result<Vec<TraceIndexEntry>, TraceStoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut all = self.read_index()?;
        if all.len() > limit {
            all = all.split_off(all.len() - limit);
        }
        all.reverse();
        Ok(all)
    }

    /// Absolute paths of turn JSON files for a conversation (from index).
    pub fn export_paths_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<PathBuf>, TraceStoreError> {
        let entries = self.list_for_conversation(conversation_id, usize::MAX)?;
        Ok(entries
            .into_iter()
            .map(|e| self.root.join(e.relative_path))
            .collect())
    }

    /// Session-scoped verified artifact metadata across recent turns.
    ///
    /// Newest-first. Prefer turns that already advertise `artifact_count > 0`
    /// in the index; fall back to loading turn JSON when the index predates
    /// that field. Caps the number of turn files scanned at `turn_limit`.
    pub fn list_artifacts_for_conversation(
        &self,
        conversation_id: &str,
        turn_limit: usize,
    ) -> Result<Vec<TraceArtifactIndexEntry>, TraceStoreError> {
        let entries = self.list_for_conversation(conversation_id, turn_limit)?;
        let mut out = Vec::new();
        for entry in entries {
            // Skip turn files that are known empty when the field is present.
            // Older index lines default `artifact_count` to 0, so still load when
            // tools ran — they may carry artifacts without the new counter.
            let likely_empty = entry.artifact_count == 0 && entry.tool_call_count == 0;
            if likely_empty {
                continue;
            }
            let trace = match self.load_relative(&entry.relative_path) {
                Ok(t) => t,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        relative_path = %entry.relative_path,
                        "skipping unreadable agent-trace turn while listing artifacts"
                    );
                    continue;
                }
            };
            let artifacts = if !trace.summary.artifacts.is_empty() {
                trace.summary.artifacts.clone()
            } else {
                // Recover from span attributes for older traces / partial writes.
                collect_artifacts_from_spans(&trace)
            };
            for artifact in artifacts {
                out.push(TraceArtifactIndexEntry {
                    schema_version: SCHEMA_VERSION,
                    trace_id: trace.trace_id.clone(),
                    conversation_id: trace.conversation_id.clone(),
                    msg_id: trace.msg_id.clone(),
                    started_at_ms: trace.started_at_ms,
                    artifact,
                });
            }
        }
        Ok(out)
    }

    /// Delete traces with `started_at_ms < older_than_ms`. Rewrites the index.
    /// Returns the number of traces removed.
    pub fn delete_older_than(&self, older_than_ms: u64) -> Result<usize, TraceStoreError> {
        let _guard = self.index_lock.lock().map_err(|_| TraceStoreError::LockPoisoned)?;
        let all = self.read_index_unlocked()?;
        let mut kept = Vec::with_capacity(all.len());
        let mut removed = 0usize;
        for entry in all {
            if entry.started_at_ms < older_than_ms {
                let path = self.root.join(&entry.relative_path);
                if path.is_file() {
                    let _ = fs::remove_file(&path);
                }
                removed += 1;
            } else {
                kept.push(entry);
            }
        }
        self.rewrite_index_unlocked(&kept)?;
        // Best-effort: remove empty conversation directories.
        let turns = self.root.join("turns");
        if turns.is_dir() {
            if let Ok(convs) = fs::read_dir(&turns) {
                for conv in convs.flatten() {
                    if conv.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let path = conv.path();
                        if fs::read_dir(&path)
                            .map(|mut d| d.next().is_none())
                            .unwrap_or(false)
                        {
                            let _ = fs::remove_dir(&path);
                        }
                    }
                }
            }
        }
        Ok(removed)
    }

    fn append_index(&self, entry: &TraceIndexEntry) -> Result<(), TraceStoreError> {
        let _guard = self.index_lock.lock().map_err(|_| TraceStoreError::LockPoisoned)?;
        fs::create_dir_all(&self.root)?;
        let path = self.index_path();
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    fn read_index(&self) -> Result<Vec<TraceIndexEntry>, TraceStoreError> {
        let _guard = self.index_lock.lock().map_err(|_| TraceStoreError::LockPoisoned)?;
        self.read_index_unlocked()
    }

    fn read_index_unlocked(&self) -> Result<Vec<TraceIndexEntry>, TraceStoreError> {
        let path = self.index_path();
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<TraceIndexEntry>(trimmed) {
                Ok(entry) => out.push(entry),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "skipping corrupt agent-trace index line"
                    );
                }
            }
        }
        Ok(out)
    }

    fn rewrite_index_unlocked(&self, entries: &[TraceIndexEntry]) -> Result<(), TraceStoreError> {
        fs::create_dir_all(&self.root)?;
        let path = self.index_path();
        let mut body = String::new();
        for entry in entries {
            body.push_str(&serde_json::to_string(entry)?);
            body.push('\n');
        }
        atomic_write(&path, body.as_bytes())?;
        Ok(())
    }

    fn load_relative(&self, relative: &str) -> Result<TurnTrace, TraceStoreError> {
        let path = self.root.join(relative);
        let bytes = fs::read(&path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.jsonl")
    }
}

fn collect_artifacts_from_spans(trace: &TurnTrace) -> Vec<crate::types::TraceArtifactMeta> {
    use crate::reported::reported_artifacts_from_tool_span;
    use crate::types::{SpanKind, TraceArtifactMeta};

    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for span in &trace.spans {
        let mut from_attrs: Vec<TraceArtifactMeta> = Vec::new();
        if let Some(value) = span.attributes.get("artifacts") {
            match serde_json::from_value::<Vec<TraceArtifactMeta>>(value.clone()) {
                Ok(items) => from_attrs = items,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        span_id = %span.span_id,
                        "skipping corrupt artifacts attribute on span"
                    );
                }
            }
        }

        let recovered = if span.kind == SpanKind::Tool {
            let tool_name = span
                .attributes
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or(span.name.as_str());
            let call_id = span
                .attributes
                .get("call_id")
                .and_then(|v| v.as_str());
            reported_artifacts_from_tool_span(
                tool_name,
                call_id,
                span.preview.as_deref(),
                &from_attrs,
            )
        } else {
            from_attrs
        };

        for artifact in recovered {
            if seen.insert(artifact.relative_path.clone()) {
                out.push(artifact);
            }
        }
    }
    out
}

/// Keep only ASCII alphanumeric, `-`, and `_`. Empty → `"unknown"`.
pub fn sanitize_path_segment(raw: &str) -> String {
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

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), TraceStoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("trace"),
        Uuid::now_v7().simple()
    ));
    fs::write(&tmp_path, bytes)?;
    if path.exists() {
        // Windows rename will not replace an existing file.
        let _ = fs::remove_file(path);
    }
    match fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_err) => match fs::copy(&tmp_path, path) {
            Ok(_) => {
                let _ = fs::remove_file(&tmp_path);
                Ok(())
            }
            Err(copy_err) => {
                let _ = fs::remove_file(&tmp_path);
                Err(TraceStoreError::Io(std::io::Error::new(
                    copy_err.kind(),
                    format!("atomic publish failed: rename={rename_err}; copy={copy_err}"),
                )))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{TokenCounts, TurnTraceBuilder, TurnTraceMeta};
    use crate::types::{SpanKind, SpanStatus, TraceArtifactMeta, SCHEMA_VERSION};
    use std::collections::BTreeMap;

    fn sample_trace(conversation_id: &str, msg_id: &str) -> TurnTrace {
        let mut b = TurnTraceBuilder::new(TurnTraceMeta {
            conversation_id: conversation_id.into(),
            msg_id: msg_id.into(),
            root_turn_id: "root".into(),
            session_kind: "session_dialogue".into(),
            origin: None,
            companion: false,
            channel_platform: None,
            provider: None,
            model: None,
        });
        let s = b.start_span(SpanKind::System, "sys");
        b.end_span(&s, SpanStatus::Ok, Some("done"), BTreeMap::new());
        b.apply_turn_completed(Some(10), TokenCounts::default(), Some("end_turn".into()));
        b.finalize()
    }

    #[test]
    fn sanitize_path_segment_filters() {
        assert_eq!(sanitize_path_segment("abc-DEF_09"), "abc-DEF_09");
        assert_eq!(sanitize_path_segment("../evil/x"), "evilx");
        assert_eq!(sanitize_path_segment("!!!"), "unknown");
    }

    #[test]
    fn persist_roundtrip_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileTraceStore::new(dir.path());

        let t1 = sample_trace("conv/one", "m1");
        let t2 = sample_trace("conv/one", "m2");
        let other = sample_trace("other", "m3");

        store.persist(&t1).unwrap();
        store.persist(&t2).unwrap();
        store.persist(&other).unwrap();

        let loaded = store.get(&t1.trace_id).unwrap().expect("t1");
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.trace_id, t1.trace_id);
        assert_eq!(loaded.msg_id, "m1");

        let listed = store.list_for_conversation("conv/one", 10).unwrap();
        assert_eq!(listed.len(), 2);
        // Newest first
        assert_eq!(listed[0].msg_id, "m2");
        assert_eq!(listed[1].msg_id, "m1");

        let recent = store.list_recent(2).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].conversation_id, "other");

        let paths = store.export_paths_for_conversation("conv/one").unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|p| p.is_file()));

        // Safe path segments: no slash in directory name
        let abs = store.root().join("turns").join("convone");
        assert!(abs.is_dir());
    }

    #[test]
    fn delete_older_than_rewrites_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileTraceStore::new(dir.path());
        let mut old = sample_trace("c", "old");
        old.started_at_ms = 100;
        let mut new = sample_trace("c", "new");
        new.started_at_ms = 9_000;
        store.persist(&old).unwrap();
        store.persist(&new).unwrap();

        let removed = store.delete_older_than(1_000).unwrap();
        assert_eq!(removed, 1);
        assert!(store.get(&old.trace_id).unwrap().is_none());
        assert!(store.get(&new.trace_id).unwrap().is_some());
        let recent = store.list_recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].trace_id, new.trace_id);
    }

    #[test]
    fn list_artifacts_for_conversation_aggregates_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileTraceStore::new(dir.path());
        let mut b = TurnTraceBuilder::new(TurnTraceMeta {
            conversation_id: "conv-art".into(),
            msg_id: "m-art".into(),
            root_turn_id: "root".into(),
            session_kind: "session_dialogue".into(),
            origin: None,
            companion: false,
            channel_platform: None,
            provider: None,
            model: None,
        });
        b.note_tool_start("c1", "generate_image", None);
        b.note_tool_end(
            "c1",
            SpanStatus::Ok,
            Some("ok"),
            &[TraceArtifactMeta {
                id: "art-1".into(),
                kind: "image".into(),
                mime_type: "image/png".into(),
                relative_path: "nomifun-artifacts/a.png".into(),
                size_bytes: 9,
                sha256: "aa".into(),
                call_id: Some("c1".into()),
                tool_name: Some("generate_image".into()),
                source: Some("receipt".into()),
            }],
        );
        let trace = b.finalize();
        store.persist(&trace).unwrap();

        let arts = store.list_artifacts_for_conversation("conv-art", 20).unwrap();
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].msg_id, "m-art");
        assert_eq!(arts[0].artifact.relative_path, "nomifun-artifacts/a.png");
        assert_eq!(arts[0].trace_id, trace.trace_id);
    }

    #[test]
    fn list_artifacts_recovers_reported_paths_from_write_preview() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileTraceStore::new(dir.path());
        let mut b = TurnTraceBuilder::new(TurnTraceMeta {
            conversation_id: "conv-write".into(),
            msg_id: "m-write".into(),
            root_turn_id: "root".into(),
            session_kind: "session_dialogue".into(),
            origin: None,
            companion: false,
            channel_platform: None,
            provider: None,
            model: None,
        });
        b.note_tool_start("cw", "Write", Some(r#"{"file_path":"scripts/app.py"}"#));
        // Historical traces often only kept the tool output as preview (no artifacts attr).
        b.note_tool_end(
            "cw",
            SpanStatus::Ok,
            Some("Created scripts/app.py (4 lines)"),
            &[],
        );
        let trace = b.finalize();
        assert_eq!(trace.summary.artifact_count, 0);
        store.persist(&trace).unwrap();

        let arts = store
            .list_artifacts_for_conversation("conv-write", 20)
            .unwrap();
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].artifact.relative_path, "scripts/app.py");
        assert_eq!(arts[0].artifact.source.as_deref(), Some("reported"));
    }
}

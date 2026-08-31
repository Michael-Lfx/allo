//! Best-effort JSON-lines log of concept-graph generation sessions. Every
//! event carries `ts`, `session`, and `event`; callers add stage fields.
//! The FULL model replies are recorded verbatim — the log exists to answer
//! "what did the model actually return" for failures that are otherwise
//! opaque (unparseable JSON, truncated output, audit rejects). Log writes
//! never fail the generation: a broken log file must not break the feature.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use nomifun_common::now_ms;

/// Appends JSON-lines records to `{dir}/concept-graph-generation.log`.
/// Writes are serialized with a mutex so concurrent sessions never
/// interleave lines.
pub(crate) struct ConceptGraphLogger {
    path: std::path::PathBuf,
    session: String,
    lock: Mutex<()>,
}

impl ConceptGraphLogger {
    /// New logger appending to `concept-graph-generation.log` inside `dir`.
    /// The directory must already exist (the caller creates it).
    pub(crate) fn new(dir: &Path, session: &str) -> Self {
        Self::with_file(dir, "concept-graph-generation.log", session)
    }

    /// New logger appending to an explicit log file inside `dir` — the same
    /// JSONL event format under a feature-specific name (course outline
    /// generation reuses it for `course-outline-generation.log`). The
    /// directory must already exist (the caller creates it).
    pub(crate) fn with_file(dir: &Path, file_name: &str, session: &str) -> Self {
        Self {
            path: dir.join(file_name),
            session: session.to_owned(),
            lock: Mutex::new(()),
        }
    }

    /// Append one event: `ts`, `session`, `event`, plus the caller's fields.
    /// Best-effort: any write failure is swallowed.
    pub(crate) fn log(&self, event: &str, fields: serde_json::Value) {
        let mut record = serde_json::Map::new();
        record.insert("ts".to_owned(), serde_json::json!(now_ms()));
        record.insert("session".to_owned(), serde_json::json!(self.session));
        record.insert("event".to_owned(), serde_json::json!(event));
        if let Some(object) = fields.as_object() {
            for (key, value) in object {
                record.insert(key.clone(), value.clone());
            }
        }
        let Ok(line) = serde_json::to_string(&serde_json::Value::Object(record)) else {
            return;
        };
        let Ok(_guard) = self.lock.lock() else {
            return;
        };
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// Structural shape of a raw model reply: byte length and brace/bracket
/// balance, so a log reader can tell a truncated JSON (opens > closes) from
/// a non-JSON reply (no braces at all) at a glance.
pub(crate) fn reply_shape(raw: &str) -> serde_json::Value {
    serde_json::json!({
        "chars": raw.len(),
        "open_brace": raw.matches('{').count(),
        "close_brace": raw.matches('}').count(),
        "open_bracket": raw.matches('[').count(),
        "close_bracket": raw.matches(']').count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cg-log-test-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn logger_appends_one_json_line_per_event_with_session_and_ts() {
        let dir = temp_dir();
        let logger = ConceptGraphLogger::new(&dir, "sess-1");
        logger.log("ping", serde_json::json!({"note": "hello"}));
        logger.log("pong", serde_json::json!({"n": 2}));
        let contents = std::fs::read_to_string(dir.join("concept-graph-generation.log")).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2, "one JSON object per line");
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["event"], "ping");
        assert_eq!(first["session"], "sess-1");
        assert_eq!(first["note"], "hello");
        assert!(first["ts"].is_number());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reply_shape_distinguishes_truncated_json_from_plain_text() {
        let truncated = reply_shape(r#"{"concepts": [{"name": "A""#);
        assert_eq!(truncated["chars"], 26);
        assert_eq!(truncated["open_brace"], 2);
        assert_eq!(truncated["close_brace"], 0);
        assert_eq!(truncated["open_bracket"], 1);
        assert_eq!(truncated["close_bracket"], 0);
        let plain = reply_shape("just some words");
        assert_eq!(plain["open_brace"], 0);
        assert_eq!(plain["close_bracket"], 0);
    }
}

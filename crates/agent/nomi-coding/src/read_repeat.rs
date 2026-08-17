//! Detect repeated Read of the same path (the common "busy but not finishing" loop).

use std::collections::HashMap;
use std::path::{Component, Path};

/// Soft nudge after this many successful Reads of the same path in one request.
pub const DEFAULT_READ_REPEAT_SOFT: usize = 2;
/// Hard stop after this many successful Reads of the same path.
pub const DEFAULT_READ_REPEAT_HARD: usize = 3;

pub const CODING_READ_REPEAT_NUDGE: &str = "Coding read-repeat: you already Read this file earlier \
in this request. Do **not** Read it again. Use the earlier Read/Grep output (including line:hash \
anchors). Either Edit/Write now, or stop with a short status. Re-reading unchanged content is not progress.";

pub const CODING_READ_REPEAT_HARD_STOP: &str = "Coding read-repeat hard-stop: the same file was Read \
repeatedly without a file mutation. Stop now. Summarize what you already know and either apply the \
edit or report the blocker. Do not call Read again on this request.";

pub const CODING_UNCHANGED_STUB_NUDGE: &str = "Coding: a Read returned \"File unchanged since last \
read\". That means the earlier tool_result is still authoritative — do **not** Read again. Edit \
with the anchors/text you already have, or stop.";

/// Normalize path strings so `./a.rs` and `a.rs` collide.
pub fn normalize_read_path(raw: &str) -> String {
    let path = Path::new(raw);
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::RootDir | Component::Prefix(_) => {
                // Keep absolute/drive markers as a single leading token.
                parts.clear();
                parts.push(c.as_os_str().to_string_lossy().into_owned());
            }
        }
    }
    if parts.is_empty() {
        raw.replace('\\', "/").to_ascii_lowercase()
    } else {
        parts.join("/").to_ascii_lowercase()
    }
}

pub fn is_unchanged_stub(content: &str) -> bool {
    content.contains("File unchanged since last read")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadRepeatAction {
    None,
    SoftNudge,
    HardStop,
    UnchangedStubNudge,
}

#[derive(Debug, Default)]
pub struct ReadRepeatTracker {
    /// Successful Read counts per normalized path this root request.
    counts: HashMap<String, usize>,
    soft_nudge_sent: HashMap<String, bool>,
    hard_stop_sent: bool,
    unchanged_nudge_sent: bool,
}

impl ReadRepeatTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Observe one successful Read. `result_content` detects dedup stubs.
    pub fn observe_read(
        &mut self,
        path: Option<&str>,
        result_content: Option<&str>,
        soft_threshold: usize,
        hard_threshold: usize,
    ) -> ReadRepeatAction {
        let soft_threshold = soft_threshold.max(2);
        let hard_threshold = hard_threshold.max(soft_threshold);

        if result_content.is_some_and(is_unchanged_stub) && !self.unchanged_nudge_sent {
            self.unchanged_nudge_sent = true;
            // Still count the path if present.
            if let Some(p) = path.filter(|p| !p.is_empty()) {
                let key = normalize_read_path(p);
                *self.counts.entry(key).or_insert(0) += 1;
            }
            return ReadRepeatAction::UnchangedStubNudge;
        }

        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return ReadRepeatAction::None;
        };
        let key = normalize_read_path(path);
        let count = {
            let e = self.counts.entry(key.clone()).or_insert(0);
            *e = e.saturating_add(1);
            *e
        };

        if count >= hard_threshold && !self.hard_stop_sent {
            self.hard_stop_sent = true;
            return ReadRepeatAction::HardStop;
        }
        if count >= soft_threshold && !self.soft_nudge_sent.get(&key).copied().unwrap_or(false) {
            self.soft_nudge_sent.insert(key, true);
            return ReadRepeatAction::SoftNudge;
        }
        ReadRepeatAction::None
    }

    pub fn count_for(&self, path: &str) -> usize {
        self.counts
            .get(&normalize_read_path(path))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_then_hard_on_same_path() {
        let mut t = ReadRepeatTracker::default();
        assert_eq!(
            t.observe_read(Some("src/a.rs"), Some("content"), 2, 3),
            ReadRepeatAction::None
        );
        assert_eq!(
            t.observe_read(Some("./src/a.rs"), Some("content"), 2, 3),
            ReadRepeatAction::SoftNudge
        );
        assert_eq!(
            t.observe_read(Some("src\\a.rs"), Some("content"), 2, 3),
            ReadRepeatAction::HardStop
        );
    }

    #[test]
    fn unchanged_stub_nudges_immediately() {
        let mut t = ReadRepeatTracker::default();
        let stub = "File unchanged since last read. The content from the earlier Read";
        assert_eq!(
            t.observe_read(Some("a.rs"), Some(stub), 2, 3),
            ReadRepeatAction::UnchangedStubNudge
        );
    }

    #[test]
    fn different_paths_independent() {
        let mut t = ReadRepeatTracker::default();
        assert_eq!(
            t.observe_read(Some("a.rs"), Some("x"), 2, 3),
            ReadRepeatAction::None
        );
        assert_eq!(
            t.observe_read(Some("b.rs"), Some("y"), 2, 3),
            ReadRepeatAction::None
        );
    }
}

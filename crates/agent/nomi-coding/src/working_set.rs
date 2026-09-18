//! Session working set: which file ranges the model has actually seen.
//!
//! Transcript dumps are truncated and compacted; this index is the mechanical
//! guarantee that a long request still knows what was read and edited.

use std::collections::BTreeMap;

use crate::read_repeat::normalize_read_path;

/// Inclusive 1-based line range, or 0-based slice converted at record time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSpan {
    pub start: usize,
    pub end: usize,
}

impl LineSpan {
    pub fn from_offset_limit(offset: usize, limit: usize, total_lines: usize) -> Self {
        let start = offset.min(total_lines);
        let end = start.saturating_add(limit).min(total_lines);
        Self { start, end }
    }

    pub fn contains(self, other: Self) -> bool {
        other.start >= self.start && other.end <= self.end
    }
}

#[derive(Debug, Clone)]
pub struct WorkingSetEntry {
    pub path: String,
    pub mtime_ms: Option<u64>,
    pub covered: Vec<LineSpan>,
    pub last_anchor: Option<String>,
    pub edited_this_request: bool,
    pub total_lines: Option<usize>,
}

impl WorkingSetEntry {
    fn merge_span(&mut self, span: LineSpan) {
        if span.end <= span.start {
            return;
        }
        self.covered.push(span);
        self.covered.sort_by_key(|s| (s.start, s.end));
        let mut merged: Vec<LineSpan> = Vec::new();
        for span in self.covered.drain(..) {
            if let Some(last) = merged.last_mut()
                && span.start <= last.end
            {
                last.end = last.end.max(span.end);
            } else {
                merged.push(span);
            }
        }
        self.covered = merged;
    }

    pub fn covers(&self, span: LineSpan, mtime_ms: Option<u64>) -> bool {
        if let (Some(have), Some(now)) = (self.mtime_ms, mtime_ms)
            && have != now
        {
            return false;
        }
        self.covered.iter().any(|have| have.contains(span))
    }

    pub fn unread_ranges(&self) -> Vec<LineSpan> {
        let Some(total) = self.total_lines.filter(|n| *n > 0) else {
            return Vec::new();
        };
        let mut unread = Vec::new();
        let mut cursor = 0usize;
        for span in &self.covered {
            if span.start > cursor {
                unread.push(LineSpan {
                    start: cursor,
                    end: span.start,
                });
            }
            cursor = cursor.max(span.end);
        }
        if cursor < total {
            unread.push(LineSpan {
                start: cursor,
                end: total,
            });
        }
        unread
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkingSet {
    entries: BTreeMap<String, WorkingSetEntry>,
}

impl WorkingSet {
    pub fn reset(&mut self) {
        self.entries.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn record_read(
        &mut self,
        path: &str,
        offset: usize,
        limit: usize,
        total_lines: usize,
        mtime_ms: Option<u64>,
    ) {
        let key = normalize_read_path(path);
        let span = LineSpan::from_offset_limit(offset, limit, total_lines);
        let entry = self.entries.entry(key.clone()).or_insert_with(|| WorkingSetEntry {
            path: key,
            mtime_ms,
            covered: Vec::new(),
            last_anchor: None,
            edited_this_request: false,
            total_lines: Some(total_lines),
        });
        entry.mtime_ms = mtime_ms.or(entry.mtime_ms);
        entry.total_lines = Some(total_lines.max(entry.total_lines.unwrap_or(0)));
        entry.merge_span(span);
    }

    pub fn record_edit(&mut self, path: &str, anchor: Option<&str>) {
        let key = normalize_read_path(path);
        let entry = self.entries.entry(key.clone()).or_insert_with(|| WorkingSetEntry {
            path: key,
            mtime_ms: None,
            covered: Vec::new(),
            last_anchor: None,
            edited_this_request: false,
            total_lines: None,
        });
        entry.edited_this_request = true;
        if let Some(anchor) = anchor.filter(|s| !s.is_empty()) {
            entry.last_anchor = Some(anchor.to_string());
        }
    }

    pub fn covers_range(
        &self,
        path: &str,
        offset: usize,
        limit: usize,
        total_lines: usize,
        mtime_ms: Option<u64>,
    ) -> bool {
        let key = normalize_read_path(path);
        let Some(entry) = self.entries.get(&key) else {
            return false;
        };
        entry.covers(LineSpan::from_offset_limit(offset, limit, total_lines), mtime_ms)
    }

    pub fn index_block(&self) -> String {
        if self.entries.is_empty() {
            return String::from(
                "[WorkingSet]\nNo files recorded this request. Prefer Grep/Glob/explore_code before a tour.",
            );
        }
        let mut lines = vec!["[WorkingSet] Files already in context — do not re-Read covered ranges; use returned Edit anchors.".to_string()];
        for entry in self.entries.values() {
            let ranges = entry
                .covered
                .iter()
                .map(|s| format!("{}-{}", s.start, s.end))
                .collect::<Vec<_>>()
                .join(",");
            let unread = entry
                .unread_ranges()
                .iter()
                .map(|s| format!("{}-{}", s.start, s.end))
                .collect::<Vec<_>>()
                .join(",");
            let edited = if entry.edited_this_request { " edited" } else { "" };
            let anchor = entry
                .last_anchor
                .as_deref()
                .map(|a| format!(" last_anchor={a}"))
                .unwrap_or_default();
            let unread_note = if unread.is_empty() {
                String::new()
            } else {
                format!(" unread={unread}")
            };
            lines.push(format!(
                "- {} covered=[{ranges}]{unread_note}{edited}{anchor}",
                entry.path
            ));
        }
        lines.join("\n")
    }

    pub fn edited_paths(&self) -> Vec<&str> {
        self.entries
            .values()
            .filter(|e| e.edited_this_request)
            .map(|e| e.path.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_ranges_after_partial_read() {
        let mut set = WorkingSet::default();
        set.record_read("src/a.rs", 0, 10, 40, Some(1));
        let unread = set.entries.get("src/a.rs").unwrap().unread_ranges();
        assert_eq!(unread, vec![LineSpan { start: 10, end: 40 }]);
        assert!(set.covers_range("src/a.rs", 0, 10, 40, Some(1)));
        assert!(!set.covers_range("src/a.rs", 10, 10, 40, Some(1)));
    }

    #[test]
    fn mtime_change_invalidates_cover() {
        let mut set = WorkingSet::default();
        set.record_read("a.rs", 0, 20, 20, Some(1));
        assert!(!set.covers_range("a.rs", 0, 20, 20, Some(2)));
    }

    #[test]
    fn index_mentions_edit_anchor() {
        let mut set = WorkingSet::default();
        set.record_read("lib.rs", 0, 5, 5, Some(1));
        set.record_edit("lib.rs", Some("12:h7x2"));
        let block = set.index_block();
        assert!(block.contains("lib.rs"));
        assert!(block.contains("last_anchor=12:h7x2"));
        assert!(block.contains("edited"));
    }
}

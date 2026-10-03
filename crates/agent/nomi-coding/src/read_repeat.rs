//! Detect repeated Read of the same path (the common "busy but not finishing" loop).

use std::collections::HashMap;

/// Soft nudge after this many successful Reads of the same path in one request.
pub const DEFAULT_READ_REPEAT_SOFT: usize = 2;
/// Hard stop after this many successful Reads of the same path.
pub const DEFAULT_READ_REPEAT_HARD: usize = 3;

pub const CODING_READ_REPEAT_NUDGE: &str = "Coding read-repeat: you already Read this file **range** earlier \
in this request. Do **not** Read the same offset/limit again. Unread ranges are listed in the Read footer \
and WorkingSet. Use earlier line:hash anchors. Either Edit/Write now, or Read only an uncovered range.";

pub const CODING_READ_REPEAT_HARD_STOP: &str = "Coding read-repeat hard-stop: the same file was Read \
repeatedly without a file mutation. Stop now. Summarize what you already know and either apply the \
edit or report the blocker. Do not call Read again on this request.";

pub const CODING_UNCHANGED_STUB_NUDGE: &str = "Coding: a Read returned \"File unchanged since last \
read\". That means the earlier tool_result is still authoritative — do **not** Read again. Edit \
with the anchors/text you already have, or stop.";

/// Comparison key for a path string: separators unified to `/`, `.` and `..`
/// resolved, drive/UNC/root preserved, ASCII case folded. `./a.rs`, `a.rs` and
/// `A.RS` collide; `C:\a.rs` and `D:\a.rs` do not.
///
/// Pure string logic so a Windows-style path from the model compares the same
/// way on every host.
pub fn normalize_read_path(raw: &str) -> String {
    canonical_path(raw, true)
}

/// Same canonical form as [`normalize_read_path`] but keeping the spelling the
/// model used, for text shown back to it.
pub(crate) fn display_read_path(raw: &str) -> String {
    canonical_path(raw, false)
}

fn canonical_path(raw: &str, fold_case: bool) -> String {
    let unified = raw.trim().replace('\\', "/");
    let (root, rest) = split_root(&unified);
    let anchored = root.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for segment in rest.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if !anchored {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = format!("{root}{}", parts.join("/"));
    let canonical = if joined.is_empty() { unified } else { joined };
    if fold_case {
        canonical.to_ascii_lowercase()
    } else {
        canonical
    }
}

fn split_root(unified: &str) -> (String, &str) {
    let mut path = unified;
    if let Some(verbatim) = path
        .strip_prefix("//?/")
        .or_else(|| path.strip_prefix("//./"))
    {
        path = verbatim;
        if path
            .get(..4)
            .is_some_and(|head| head.eq_ignore_ascii_case("unc/"))
        {
            return unc_root(&path[4..]);
        }
    } else if let Some(unc) = path.strip_prefix("//")
        && !unc.starts_with('/')
    {
        return unc_root(unc);
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let (drive, rest) = path.split_at(2);
        return match rest.strip_prefix('/') {
            Some(rest) => (format!("{drive}/"), rest),
            None => (drive.to_string(), rest),
        };
    }
    match path.strip_prefix('/') {
        Some(rest) => ("/".to_string(), rest),
        None => (String::new(), path),
    }
}

fn unc_root(after_slashes: &str) -> (String, &str) {
    let mut fields = after_slashes.splitn(3, '/');
    let host = fields.next().unwrap_or("");
    let share = fields.next().unwrap_or("");
    let rest = fields.next().unwrap_or("");
    if share.is_empty() {
        (format!("//{host}/"), rest)
    } else {
        (format!("//{host}/{share}/"), rest)
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

fn range_key(path: &str, offset: Option<usize>, limit: Option<usize>) -> String {
    format!(
        "{}#{}:{}",
        normalize_read_path(path),
        offset.unwrap_or(0),
        limit
            .map(|n| n.to_string())
            .unwrap_or_else(|| "end".to_string())
    )
}

#[derive(Debug, Default)]
pub struct ReadRepeatTracker {
    /// Successful Read counts per normalized path+range this root request.
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
    /// Repeat is keyed by path **and** offset/limit so paging a file is not a loop.
    pub fn observe_read(
        &mut self,
        path: Option<&str>,
        result_content: Option<&str>,
        soft_threshold: usize,
        hard_threshold: usize,
    ) -> ReadRepeatAction {
        self.observe_read_range(path, result_content, None, None, soft_threshold, hard_threshold)
    }

    pub fn observe_read_range(
        &mut self,
        path: Option<&str>,
        result_content: Option<&str>,
        offset: Option<usize>,
        limit: Option<usize>,
        soft_threshold: usize,
        hard_threshold: usize,
    ) -> ReadRepeatAction {
        let soft_threshold = soft_threshold.max(2);
        let hard_threshold = hard_threshold.max(soft_threshold);

        if result_content.is_some_and(is_unchanged_stub) && !self.unchanged_nudge_sent {
            self.unchanged_nudge_sent = true;
            // Still count the range if present.
            if let Some(p) = path.filter(|p| !p.is_empty()) {
                let key = range_key(p, offset, limit);
                *self.counts.entry(key).or_insert(0) += 1;
            }
            return ReadRepeatAction::UnchangedStubNudge;
        }

        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return ReadRepeatAction::None;
        };
        let key = range_key(path, offset, limit);
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
        self.count_for_range(path, None, None)
    }

    pub fn count_for_range(&self, path: &str, offset: Option<usize>, limit: Option<usize>) -> usize {
        self.counts
            .get(&range_key(path, offset, limit))
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

    #[test]
    fn different_ranges_of_same_path_are_independent() {
        let mut t = ReadRepeatTracker::default();
        assert_eq!(
            t.observe_read_range(Some("a.rs"), Some("head"), Some(0), Some(50), 2, 3),
            ReadRepeatAction::None
        );
        assert_eq!(
            t.observe_read_range(Some("a.rs"), Some("tail"), Some(50), Some(50), 2, 3),
            ReadRepeatAction::None
        );
        assert_eq!(t.count_for_range("a.rs", Some(0), Some(50)), 1);
    }

    #[test]
    fn windows_absolute_path_keeps_drive_and_unifies_spellings() {
        let canonical = "c:/users/admin/docs/readme.md";
        for spelling in [
            "C:\\Users\\Admin\\Docs\\README.md",
            "c:/users/admin/docs/readme.md",
            "C:/Users/Admin/./Docs/README.md",
            "C:\\Users\\Admin\\Docs\\sub\\..\\README.md",
            "  C:/Users/Admin/Docs/README.md ",
        ] {
            assert_eq!(normalize_read_path(spelling), canonical, "{spelling}");
        }
    }

    #[test]
    fn different_drives_do_not_collide() {
        assert_ne!(normalize_read_path("C:\\a.rs"), normalize_read_path("D:\\a.rs"));
        assert_ne!(normalize_read_path("C:\\a.rs"), normalize_read_path("a.rs"));
    }

    #[test]
    fn posix_absolute_path_has_a_single_root_slash() {
        assert_eq!(normalize_read_path("/home/u/a.rs"), "/home/u/a.rs");
        assert_eq!(normalize_read_path("/home/u/../v/a.rs"), "/home/v/a.rs");
        assert_ne!(normalize_read_path("/a.rs"), normalize_read_path("a.rs"));
    }

    #[test]
    fn unc_and_verbatim_paths_are_recognised() {
        assert_eq!(
            normalize_read_path("\\\\Srv\\Share\\dir\\a.rs"),
            "//srv/share/dir/a.rs"
        );
        assert_eq!(
            normalize_read_path("\\\\?\\C:\\x\\a.rs"),
            normalize_read_path("C:/x/a.rs")
        );
        assert_eq!(
            normalize_read_path("\\\\?\\UNC\\srv\\share\\a.rs"),
            normalize_read_path("//srv/share/a.rs")
        );
    }

    #[test]
    fn parent_segments_never_escape_an_anchored_root() {
        assert_eq!(normalize_read_path("C:/../a.rs"), "c:/a.rs");
        assert_eq!(normalize_read_path("/../a.rs"), "/a.rs");
    }

    #[test]
    fn leading_parent_segments_of_relative_paths_are_kept() {
        assert_eq!(normalize_read_path("../a.rs"), "../a.rs");
        assert_eq!(normalize_read_path("a/../../b.rs"), "../b.rs");
        assert_ne!(normalize_read_path("../a.rs"), normalize_read_path("a.rs"));
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        for raw in ["", ".", "..", "/", "C:", "C:\\", "//", "\\\\?\\", "\\\\server", "é:/x"] {
            let _ = normalize_read_path(raw);
            let _ = display_read_path(raw);
        }
        assert_eq!(normalize_read_path(""), "");
        assert_eq!(normalize_read_path("."), ".");
    }

    #[test]
    fn display_path_keeps_the_spelling_but_not_the_separators() {
        assert_eq!(
            display_read_path("C:\\Users\\Admin\\Docs\\README.md"),
            "C:/Users/Admin/Docs/README.md"
        );
        assert_eq!(display_read_path("./Src/Main.rs"), "Src/Main.rs");
    }

    #[test]
    fn repeated_reads_of_a_windows_absolute_path_escalate() {
        let mut t = ReadRepeatTracker::default();
        assert_eq!(
            t.observe_read(Some("C:\\work\\docs\\a.md"), Some("content"), 2, 3),
            ReadRepeatAction::None
        );
        assert_eq!(
            t.observe_read(Some("c:/work/docs/A.md"), Some("content"), 2, 3),
            ReadRepeatAction::SoftNudge
        );
        assert_eq!(
            t.observe_read(Some("C:/work/./docs/a.md"), Some("content"), 2, 3),
            ReadRepeatAction::HardStop
        );
    }

    #[test]
    fn same_name_on_different_drives_is_not_a_repeat() {
        let mut t = ReadRepeatTracker::default();
        assert_eq!(
            t.observe_read(Some("C:\\a.rs"), Some("x"), 2, 3),
            ReadRepeatAction::None
        );
        assert_eq!(
            t.observe_read(Some("D:\\a.rs"), Some("x"), 2, 3),
            ReadRepeatAction::None
        );
    }
}

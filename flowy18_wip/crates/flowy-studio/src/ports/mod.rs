//! UI-side extensible adapter ports, dirty-cached board snapshots, local
//! environment checks, and redacted trace projections.
//!
//! Boundary invariant: this module only reads through `flowy_agent::AgentHandle`
//! public APIs and `crate::projects::Project`. It never modifies kernel crates
//! or parses kernel disk files (`sessions/*.json`, `delivery/*.jsonl`,
//! `acceptance/journal.jsonl`) directly.

mod board;
mod local_env;
mod trace;

use std::path::Path;

pub use board::{
    BoardDataSource, BoardKpiSummary, BranchPhase, DeliverableFormat,
    SessionBoardGroup, SkillChip, SkillLabel, UNASSIGNED_QUEUE_SESSION_ID,
    WorkBoardSnapshot, WorkBranchSummary, WorkContractProgress,
};
pub use local_env::{
    EnvSeverity, LocalEnvCheck, LocalEnvDetail, LocalEnvSnapshot,
    probe_local_env,
};
pub use trace::{
    RedactedTraceEntry, TraceLevel, collect_session_traces,
    format_traces_jsonl, redact_trace_text,
};

/// Strips Windows extended-length path prefixes (`\\?\` / `\\?\UNC\`) for clean
/// UI rendering across breadcrumbs, composer chips, approval cards, and drawers.
#[must_use]
pub fn display_path(path: &Path) -> String {
    display_path_str(&path.to_string_lossy())
}

/// String variant of [`display_path`] for persisted artifact or tool paths.
#[must_use]
pub fn display_path_str(raw: &str) -> String {
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_owned()
    } else {
        raw.to_owned()
    }
}

/// Last few characters of an id, by **character** (never by byte) so
/// human-edited UTF-8 journal ids cannot panic the UI.
#[must_use]
pub(crate) fn short_id(id: &str) -> String {
    let count = id.chars().count();
    id.chars().skip(count.saturating_sub(8)).collect()
}

pub(super) fn clip_label(text: &str, max_chars: usize) -> String {
    let collapsed: String =
        text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}\u{2026}")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_strips_windows_extended_prefixes() {
        assert_eq!(
            display_path_str(r"\\?\C:\Users\RiKa\Docs\Q3.xlsx"),
            r"C:\Users\RiKa\Docs\Q3.xlsx"
        );
        assert_eq!(
            display_path_str(r"\\?\UNC\fileserver\share\report.pptx"),
            r"\\fileserver\share\report.pptx"
        );
        assert_eq!(display_path_str("/tmp/demo.docx"), "/tmp/demo.docx");
    }

    #[test]
    fn short_id_never_splits_a_character() {
        assert_eq!(short_id("accepted-1a208c0bc80-0"), "c0bc80-0");
        assert_eq!(short_id("short"), "short");
        assert_eq!(short_id(""), "");
        assert_eq!(short_id("中文中文"), "中文中文");
        assert_eq!(short_id("排队中排队中中文测试"), "中排队中中文测试");
        assert_eq!(short_id("短"), "短");
    }
}

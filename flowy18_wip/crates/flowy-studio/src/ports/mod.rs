//! UI-side extensible adapter ports, dirty-cached board snapshots, local
//! environment checks, path safety guards, and redacted trace projections.
//!
//! Boundary invariant: this module only reads through `flowy_agent::AgentHandle`
//! public APIs and `crate::projects::Project`. It never modifies kernel crates
//! or parses kernel disk files (`sessions/*.json`, `delivery/*.jsonl`,
//! `acceptance/journal.jsonl`) directly.

mod board;
mod local_env;
mod local_path;
mod trace;

pub use board::{
    BoardDataSource, BoardKpiSummary, BranchPhase, DailyActivityBucket,
    DeliverableFormat, LocalDateKey, SEVEN_DAYS_MS, SessionBoardGroup,
    SkillChip, SkillLabel, UNASSIGNED_QUEUE_SESSION_ID, WorkBoardSnapshot,
    WorkBranchSummary, WorkContractProgress, civil_month_day_from_epoch_ms,
    civil_month_day_in_tz, compute_daily_7d, compute_daily_7d_in_tz,
    is_older_than_7d, is_older_than_7d_in_tz, local_date_from_epoch_ms,
    local_days_ago, local_tz_offset_secs, matches_day_filter,
    normalize_epoch_ms, now_epoch_ms,
};
pub use local_env::{
    EnvSeverity, LocalEnvCheck, LocalEnvDetail, LocalEnvSnapshot,
    probe_local_env,
};
pub use local_path::{display_path, display_path_str};
pub(crate) use local_path::{
    is_link_or_unsafe_reparse, is_same_or_overlapping_dir,
    is_unsafe_local_path, resolve_safe_local_dir, resolve_safe_local_file,
    resolve_safe_parent_dir,
};
pub use trace::{
    RedactedTraceEntry, TraceLevel, collect_session_traces,
    format_traces_jsonl, redact_trace_text,
};

/// Last few characters of an id, by **character** (never by byte) so
/// human-edited UTF-8 journal ids cannot panic the UI.
#[must_use]
pub(crate) fn short_id(id: &str) -> String {
    let count = id.chars().count();
    id.chars().skip(count.saturating_sub(8)).collect()
}

#[must_use]
pub(crate) fn basename(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
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
    use super::short_id;

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

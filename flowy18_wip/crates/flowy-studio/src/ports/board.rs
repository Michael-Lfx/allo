//! Dirty-cached Visual Board snapshots and work-contract branch projections.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{Datelike, Duration, FixedOffset, Local, NaiveDate, TimeZone};
use flowy_agent::{
    AcceptedMessage, AgentHandle, ChatLine, ReceiptState, RequiredDeliverable,
    SessionSummary, SubmissionOutcome, TurnStatus, VerificationIssue,
    WorkStatus,
};

use super::{
    basename, clip_label, display_path, display_path_str, redact_trace_text,
    short_id,
};
use crate::projects::Project;

/// Synthetic session group id used to surface unassigned queued/failed receipts.
pub const UNASSIGNED_QUEUE_SESSION_ID: &str = "unassigned-queue";

/// Milliseconds in one 24-hour day.
pub const DAY_MS: u64 = 86_400_000;

/// Normalizes Unix timestamps to epoch milliseconds.
///
/// Legacy fixtures or older records may store Unix seconds (`< 10^10`); modern
/// `SessionSummary::updated_at` and `AcceptedMessage::created_at_ms` store
/// milliseconds (`~1.7 × 10^12`).
#[must_use]
pub const fn normalize_epoch_ms(ts: u64) -> u64 {
    if ts > 0 && ts < 10_000_000_000 {
        ts.saturating_mul(1_000)
    } else {
        ts
    }
}

/// Current wall-clock epoch time in milliseconds.
#[must_use]
pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Calendar date key in local timezone: `(year, month, day)`.
pub type LocalDateKey = (i32, u8, u8);

/// Current local timezone offset in seconds east of UTC.
#[must_use]
pub fn local_tz_offset_secs() -> i32 {
    Local::now().offset().local_minus_utc()
}

fn local_naive_date(ts_ms: u64, tz_offset_secs: i32) -> Option<NaiveDate> {
    let ts = normalize_epoch_ms(ts_ms);
    if ts == 0 {
        return None;
    }
    let millis = i64::try_from(ts).ok()?;
    let offset = FixedOffset::east_opt(tz_offset_secs)
        .or_else(|| FixedOffset::east_opt(0))?;
    offset
        .timestamp_millis_opt(millis)
        .single()
        .map(|dt| dt.date_naive())
}

fn naive_date_key(date: NaiveDate) -> LocalDateKey {
    (date.year(), date.month() as u8, date.day() as u8)
}

/// Returns `(month, day)` in the given timezone offset, falling back to UTC
/// civil conversion if the timestamp is outside `chrono`'s valid range.
#[must_use]
pub fn civil_month_day_in_tz(ts_ms: u64, tz_offset_secs: i32) -> (u8, u8) {
    if let Some(date) = local_naive_date(ts_ms, tz_offset_secs) {
        let (_, month, day) = naive_date_key(date);
        (month, day)
    } else {
        civil_month_day_utc_fallback(normalize_epoch_ms(ts_ms))
    }
}

/// Number of whole local calendar days between `ts_ms` and `now_ms`.
/// Returns `None` when either timestamp is zero/unrepresentable.
#[must_use]
pub fn local_days_ago(
    ts_ms: u64,
    now_ms: u64,
    tz_offset_secs: i32,
) -> Option<i64> {
    let today = local_naive_date(now_ms, tz_offset_secs)?;
    let date = local_naive_date(ts_ms, tz_offset_secs)?;
    Some(today.signed_duration_since(date).num_days())
}

/// Returns true when a timestamp falls strictly earlier than the 7-day local
/// calendar window (`today - 6 days` through `today`). Zero or future clock-skew
/// timestamps are never hidden behind the 7-day fold.
#[must_use]
pub fn is_older_than_7d_in_tz(
    updated_at_ms: u64,
    now_ms: u64,
    tz_offset_secs: i32,
) -> bool {
    local_days_ago(updated_at_ms, now_ms, tz_offset_secs)
        .is_some_and(|days| days >= 7)
}

/// Single source of truth mapping a session's `updated_at` timestamp to its
/// local calendar-day bucket key (clamping minor future clock skew to Today).
fn bucket_date_for_session(
    updated_at_ms: u64,
    now_ms: u64,
    tz_offset_secs: i32,
) -> Option<LocalDateKey> {
    let today = local_naive_date(now_ms, tz_offset_secs)?;
    let date = local_naive_date(updated_at_ms, tz_offset_secs)?;
    if today.signed_duration_since(date).num_days() < 0 {
        Some(naive_date_key(today))
    } else {
        Some(naive_date_key(date))
    }
}

/// Whether a session timestamp belongs to the user-selected local calendar day
/// on the 7-day Progress Board chart.
#[must_use]
pub fn matches_day_filter(
    updated_at_ms: u64,
    now_ms: u64,
    tz_offset_secs: i32,
    selected_day: LocalDateKey,
) -> bool {
    bucket_date_for_session(updated_at_ms, now_ms, tz_offset_secs)
        == Some(selected_day)
}

const fn civil_month_day_utc_fallback(ts_ms: u64) -> (u8, u8) {
    let z = (ts_ms / DAY_MS).cast_signed() + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097).cast_unsigned();
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    (month, day)
}

/// High-level deliverable format bucket for KPI visualization and file pills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeliverableFormat {
    Sheet,
    Slides,
    Doc,
    Pdf,
    Markdown,
    CodeWeb,
    #[default]
    Other,
}

impl DeliverableFormat {
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("xlsx" | "xls" | "csv" | "tsv" | "ods") => Self::Sheet,
            Some("pptx" | "ppt" | "key" | "odp") => Self::Slides,
            Some("docx" | "doc" | "rtf" | "odt") => Self::Doc,
            Some("pdf") => Self::Pdf,
            Some("md" | "markdown" | "txt") => Self::Markdown,
            Some(
                "html" | "htm" | "css" | "scss" | "less" | "js" | "mjs" | "cjs"
                | "ts" | "tsx" | "jsx" | "py" | "rs" | "go" | "java" | "kt"
                | "c" | "cc" | "cpp" | "h" | "hpp" | "cs" | "rb" | "php"
                | "swift" | "json" | "yaml" | "yml" | "toml" | "xml" | "sql"
                | "sh" | "bash" | "ps1" | "vue" | "svelte",
            ) => Self::CodeWeb,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Sheet => "XLSX",
            Self::Slides => "PPTX",
            Self::Doc => "DOCX",
            Self::Pdf => "PDF",
            Self::Markdown => "MD",
            Self::CodeWeb => "CODE",
            Self::Other => "FILE",
        }
    }
}

/// User-facing installed skill option pushed from `flowy-app` into Studio.
///
/// `id` uses the canonical `<owner_handle>/<slug>` invocation identity
/// expected by `flowy-agent`'s `/skill:<owner>/<slug>` protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillChip {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub official: bool,
}

/// Lifecycle phase for one branch inside a session flowchart.
#[derive(Clone, Debug, PartialEq)]
pub enum BranchPhase {
    /// Verification failed and awaits explicit two-step user confirmation.
    NeedsApproval {
        report_id: String,
        artifact_path: String,
        summary: String,
        issues: Vec<VerificationIssue>,
        semantic_hint: Option<String>,
        provenance_warnings: Vec<String>,
    },
    /// Durable delivery commit failed after turn completion; user can retry.
    DeliveryFailed,
    /// Actively running a turn or tool call.
    Running { step_label: String, percent: f32 },
    /// Deferred acceptance receipt tracked in `AcceptanceQueue`.
    Queued {
        receipt_id: String,
        state: ReceiptState,
    },
    /// Delivered and verified (or explicitly overridden with audit trail).
    Delivered {
        override_used: bool,
        hash_short: String,
        provenance_warnings: Vec<String>,
    },
    /// Idle conversation without pending deliverables.
    Idle,
}

impl BranchPhase {
    #[must_use]
    pub const fn is_blocker(&self) -> bool {
        matches!(
            self,
            Self::NeedsApproval { .. }
                | Self::DeliveryFailed
                | Self::Queued {
                    state: ReceiptState::Dead | ReceiptState::Quarantined,
                    ..
                }
        )
    }

    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(
            self,
            Self::Running { .. }
                | Self::Queued {
                    state: ReceiptState::Queued
                        | ReceiptState::Running
                        | ReceiptState::Retrying,
                    ..
                }
        )
    }
}

/// Localizable skill/stage label shown on board cards and flowchart nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillLabel {
    Key(&'static str),
    Literal(String),
}

impl SkillLabel {
    #[must_use]
    pub fn resolve<F: Fn(&str) -> String>(&self, t: F) -> String {
        match self {
            Self::Key(key) => t(key),
            Self::Literal(text) => text.clone(),
        }
    }
}

/// Read-only projection of a kernel `WorkStatus` contract for one work branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkContractProgress {
    pub work_id: String,
    pub contract_revision: Option<u32>,
    pub required_labels: Vec<String>,
    pub delivered_count: usize,
    pub missing_items: Vec<String>,
    pub outcome: Option<SubmissionOutcome>,
}

impl WorkContractProgress {
    #[must_use]
    pub fn from_status(status: &WorkStatus) -> Option<Self> {
        let has_contract = !status.required_items.is_empty()
            || !status.missing_items.is_empty()
            || !status.delivered_artifact_ids.is_empty();
        if !has_contract {
            return None;
        }
        Some(Self {
            work_id: status.work_id.clone(),
            contract_revision: status.contract_revision,
            required_labels: status
                .required_items
                .iter()
                .map(RequiredDeliverable::label)
                .collect(),
            delivered_count: status.delivered_artifact_ids.len(),
            missing_items: status.missing_items.clone(),
            outcome: status.outcome.clone(),
        })
    }
}

/// Concise delegation summary for one child Worker owned by a Router session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildWorkDelegation {
    pub session_id: String,
    pub title: String,
    pub is_running: bool,
    pub has_blocker: bool,
    pub deliverable_count: usize,
    pub status_hint: String,
}

/// One visual flow branch inside a session swimlane / modal flowchart.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkBranchSummary {
    pub id: String,
    /// Owning session ID when projected from a child Worker onto a parent
    /// Router group (`None` means owned directly by `SessionBoardGroup.session_id`).
    pub owner_session_id: Option<String>,
    pub skill_label: SkillLabel,
    pub phase: BranchPhase,
    pub deliverable_name: String,
    pub deliverable_path: Option<String>,
    pub format: DeliverableFormat,
    /// True for unverified/uncontracted intermediate workspace file changes
    /// (`CodeWeb` or `Other`) moved into `SessionBoardGroup::workspace_changes`.
    pub is_workspace_change: bool,
    pub progress_pct: f32,
    pub detail: String,
    pub work_progress: Option<WorkContractProgress>,
}

/// Session-layered aggregation displayed on the Global Visual Board.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionBoardGroup {
    pub session_id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub workspace_display: String,
    pub updated_at: u64,
    pub pinned: bool,
    pub archived: bool,
    pub preview: Option<String>,
    pub assistant_summary: Option<String>,
    pub branches: Vec<WorkBranchSummary>,
    pub workspace_changes: Vec<WorkBranchSummary>,
    pub child_workers: Vec<ChildWorkDelegation>,
    /// Total formal branch count (retained without visual truncation).
    pub total_branch_count: usize,
    /// Total non-idle / file deliverable count across formal branches.
    pub total_file_count: usize,
    pub session_contract: Option<WorkContractProgress>,
    pub has_blocker: bool,
    pub has_delivered: bool,
    pub is_running: bool,
    pub is_active: bool,
    /// Pre-truncation KPI contribution of this group so project-filtered views
    /// compute exact KPIs without re-reading kernel state.
    pub group_kpi: BoardKpiSummary,
}

impl SessionBoardGroup {
    /// Synthetic bucket for receipts that are not yet attached to a real session.
    #[must_use]
    pub fn is_synthetic(&self) -> bool {
        self.session_id == UNASSIGNED_QUEUE_SESSION_ID
    }

    /// True when the session is non-archived, settled (neither blocked nor
    /// running), and has at least one formal delivered branch.
    #[must_use]
    pub const fn is_delivered_settled(&self) -> bool {
        !self.archived
            && !self.has_blocker
            && !self.is_running
            && self.has_delivered
    }

    /// True when any formal branch carries a deliverable file of format `fmt`.
    #[must_use]
    pub fn has_formal_format(&self, fmt: DeliverableFormat) -> bool {
        self.branches.iter().any(|b| {
            !b.is_workspace_change
                && b.deliverable_path.is_some()
                && b.format == fmt
        })
    }
}

/// One day bucket inside the 7-day Progress Board activity bar chart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DailyActivityBucket {
    /// Days ago (`6` = oldest visible day, `0` = Today in local calendar).
    pub day_offset: u8,
    pub date_key: LocalDateKey,
    pub month: u8,
    pub day: u8,
    pub active_or_blocked: usize,
    pub completed_or_delivered: usize,
}

impl DailyActivityBucket {
    #[must_use]
    pub const fn total(&self) -> usize {
        self.active_or_blocked + self.completed_or_delivered
    }
}

/// Buckets non-archived sessions into 7 local calendar-day columns
/// (`day_offset` 6 down to 0) aligned with the 7-day visual fold boundary.
#[must_use]
pub fn compute_daily_7d_in_tz(
    groups: &[&SessionBoardGroup],
    now_ms: u64,
    tz_offset_secs: i32,
) -> Vec<DailyActivityBucket> {
    let today = local_naive_date(now_ms, tz_offset_secs);
    let mut buckets: Vec<DailyActivityBucket> = (0..7_u8)
        .rev()
        .map(|offset| {
            let (date_key, month, day) = if let Some(today_date) = today {
                let d = today_date - Duration::days(i64::from(offset));
                let key = naive_date_key(d);
                (key, key.1, key.2)
            } else {
                let bucket_ts =
                    now_ms.saturating_sub(u64::from(offset) * DAY_MS);
                let (m, d) = civil_month_day_utc_fallback(bucket_ts);
                ((0, m, d), m, d)
            };
            DailyActivityBucket {
                day_offset: offset,
                date_key,
                month,
                day,
                active_or_blocked: 0,
                completed_or_delivered: 0,
            }
        })
        .collect();

    for group in groups {
        if group.archived {
            continue;
        }
        let Some(key) =
            bucket_date_for_session(group.updated_at, now_ms, tz_offset_secs)
        else {
            continue;
        };
        if let Some(bucket) = buckets.iter_mut().find(|b| b.date_key == key) {
            if group.is_active {
                bucket.active_or_blocked += 1;
            } else {
                bucket.completed_or_delivered += 1;
            }
        }
    }
    buckets
}

/// Pre-aggregated KPI metrics painted by `flowy-ui::charts::donut_chart` and bars.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardKpiSummary {
    pub total_branches: usize,
    pub approval_count: usize,
    pub running_count: usize,
    pub delivered_count: usize,
    pub sheet_count: usize,
    pub slides_count: usize,
    pub doc_count: usize,
    pub pdf_count: usize,
    pub markdown_count: usize,
    pub code_count: usize,
    pub other_count: usize,
    pub formal_deliverable_count: usize,
    pub workspace_change_count: usize,
    pub active_sessions: usize,
    pub total_sessions: usize,
    pub archived_sessions: usize,
    pub blocker_sessions: usize,
    pub running_sessions: usize,
    pub delivered_sessions: usize,
    pub today_completed_sessions: usize,
}

impl BoardKpiSummary {
    /// Aggregates KPI totals across a filtered slice of session groups using
    /// each group's pre-computed counts. Archived sessions are excluded from
    /// active/delivered KPI totals and tallied separately in `archived_sessions`.
    #[must_use]
    pub fn from_groups(groups: &[&SessionBoardGroup]) -> Self {
        Self::from_groups_in_tz(groups, now_epoch_ms(), local_tz_offset_secs())
    }

    /// Aggregates KPI totals across `groups` using explicit `now_ms` and
    /// `tz_offset_secs` so `today_completed_sessions` matches the local
    /// calendar Today bucket.
    #[must_use]
    pub fn from_groups_in_tz(
        groups: &[&SessionBoardGroup],
        now_ms: u64,
        tz_offset_secs: i32,
    ) -> Self {
        let today_key = bucket_date_for_session(now_ms, now_ms, tz_offset_secs);
        let mut out = Self::default();
        for group in groups {
            if group.archived {
                out.archived_sessions += 1;
                continue;
            }
            out.total_sessions += 1;
            if group.is_active {
                out.active_sessions += 1;
            }
            if group.has_blocker {
                out.blocker_sessions += 1;
            }
            if group.is_running {
                out.running_sessions += 1;
            }
            if group.is_delivered_settled() {
                out.delivered_sessions += 1;
            }
            if !group.is_active
                && today_key.is_some_and(|key| {
                    matches_day_filter(
                        group.updated_at,
                        now_ms,
                        tz_offset_secs,
                        key,
                    )
                })
            {
                out.today_completed_sessions += 1;
            }
            out.total_branches += group.group_kpi.total_branches;
            out.approval_count += group.group_kpi.approval_count;
            out.running_count += group.group_kpi.running_count;
            out.delivered_count += group.group_kpi.delivered_count;
            out.sheet_count += group.group_kpi.sheet_count;
            out.slides_count += group.group_kpi.slides_count;
            out.doc_count += group.group_kpi.doc_count;
            out.pdf_count += group.group_kpi.pdf_count;
            out.markdown_count += group.group_kpi.markdown_count;
            out.code_count += group.group_kpi.code_count;
            out.other_count += group.group_kpi.other_count;
            out.formal_deliverable_count +=
                group.group_kpi.formal_deliverable_count;
            out.workspace_change_count +=
                group.group_kpi.workspace_change_count;
        }
        out
    }
}

/// Dirty-cached snapshot owned by `StudioPanel`.
///
/// Recomputed strictly when `AgentUpdate` or local project/approval changes
/// occur — never on idle 33ms pump ticks or inside `Render::render`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkBoardSnapshot {
    pub kpi: BoardKpiSummary,
    pub sessions: Vec<SessionBoardGroup>,
    pub daily_7d: Vec<DailyActivityBucket>,
    pub generated_at_ms: u64,
    pub tz_offset_secs: i32,
    pub generation: u64,
}

/// Abstraction for board data sources so future kernel snapshots can plug in
/// without altering `overview`.
pub trait BoardDataSource {
    fn build_board_snapshot(
        &self,
        projects: &[Project],
        visible_receipts: &[(AcceptedMessage, ReceiptState)],
        default_space_label: &str,
        unassigned_queue_title: &str,
        generation: u64,
        localize: &dyn Fn(&str) -> String,
    ) -> WorkBoardSnapshot;
}

const fn receipt_progress_pct(state: ReceiptState) -> f32 {
    match state {
        ReceiptState::Queued => 15.0,
        ReceiptState::Retrying => 25.0,
        ReceiptState::Running => 45.0,
        ReceiptState::Quarantined | ReceiptState::Dead => 10.0,
        ReceiptState::Completed => 100.0,
        ReceiptState::Cancelled => 0.0,
    }
}

const fn branch_sort_rank(phase: &BranchPhase) -> u8 {
    if phase.is_blocker() {
        0
    } else if phase.is_running() {
        1
    } else if matches!(phase, BranchPhase::Delivered { .. }) {
        2
    } else {
        3
    }
}

/// Extracts and redacts the latest non-empty `ChatLine::Assistant` reply,
/// clipped cleanly to at most 480 characters for board Q&A / summary previews.
#[must_use]
fn extract_assistant_summary(lines: &[ChatLine]) -> Option<String> {
    lines.iter().rev().find_map(|line| match line {
        ChatLine::Assistant { text } => {
            let stripped = flowy_agent::strip_markdown_inline(text);
            let trimmed = stripped.trim();
            if trimmed.is_empty() {
                return None;
            }
            let safe = redact_trace_text(trimmed);
            let collapsed: String =
                safe.split_whitespace().collect::<Vec<_>>().join(" ");
            if collapsed.is_empty() {
                return None;
            }
            if collapsed.chars().count() <= 480 {
                Some(collapsed)
            } else {
                Some(clip_label(&collapsed, 479))
            }
        }
        _ => None,
    })
}

/// Folds a Router conversation's `updated_at` with all owned child Workers'
/// `updated_at` timestamps so PR #497's 7-day local calendar buckets and day
/// drill-down include today's child-work activity on older Routers.
#[must_use]
fn fold_group_updated_at(
    parent_updated_at: u64,
    child_works: &[flowy_agent::ChildWorkSummary],
) -> u64 {
    child_works
        .iter()
        .fold(normalize_epoch_ms(parent_updated_at), |acc, work| {
            acc.max(normalize_epoch_ms(work.updated_at))
        })
}

#[expect(
    clippy::too_many_arguments,
    reason = "pure snapshot assembly helper shared by BoardDataSource and unit tests"
)]
fn finalize_session_group(
    summary: &SessionSummary,
    lines: &[ChatLine],
    mut all_branches: Vec<WorkBranchSummary>,
    child_works: &[flowy_agent::ChildWorkSummary],
    child_workers: Vec<ChildWorkDelegation>,
    is_busy: bool,
    session_contract: Option<WorkContractProgress>,
    raw_preview: Option<String>,
    project: Option<&Project>,
    default_space_label: &str,
) -> SessionBoardGroup {
    if !child_works.is_empty() {
        disambiguate_deliverable_names(&mut all_branches);
    }

    let mut branches = Vec::new();
    let mut workspace_changes = Vec::new();
    for branch in all_branches {
        if branch.is_workspace_change {
            workspace_changes.push(branch);
        } else {
            branches.push(branch);
        }
    }

    if branches.is_empty() {
        branches.push(build_idle_branch(summary, lines));
    }

    let has_blocker = branches.iter().any(|branch| branch.phase.is_blocker());
    let has_delivered = branches
        .iter()
        .any(|branch| matches!(branch.phase, BranchPhase::Delivered { .. }));
    let is_running =
        is_busy || branches.iter().any(|branch| branch.phase.is_running());
    let is_active = has_blocker || is_running;

    let mut group_kpi = BoardKpiSummary::default();
    for branch in &branches {
        accumulate_kpi(&mut group_kpi, branch);
    }
    for change in &workspace_changes {
        accumulate_kpi(&mut group_kpi, change);
    }

    branches.sort_by_key(|branch| branch_sort_rank(&branch.phase));
    let total_branch_count = branches.len();
    let total_file_count = branches
        .iter()
        .filter(|b| {
            b.deliverable_path.is_some()
                || !matches!(b.phase, BranchPhase::Idle)
        })
        .count();

    let raw_title = if summary.title.trim().is_empty() {
        raw_preview.clone().unwrap_or_else(|| short_id(&summary.id))
    } else {
        summary.title.clone()
    };
    let title = redact_trace_text(&raw_title);
    let preview = raw_preview
        .map(|text| clip_label(&redact_trace_text(&text), 72))
        .filter(|text| !text.is_empty() && !is_redundant_preview(&title, text));
    let assistant_summary = extract_assistant_summary(lines);
    let workspace_display = project.map_or_else(
        || default_space_label.to_owned(),
        |proj| display_path(&proj.root),
    );
    let latest_updated_at =
        fold_group_updated_at(summary.updated_at, child_works);

    SessionBoardGroup {
        session_id: summary.id.clone(),
        title,
        project_id: summary.project_id.clone(),
        project_name: project.map(|proj| proj.name.clone()),
        workspace_display,
        updated_at: latest_updated_at,
        pinned: summary.pinned,
        archived: summary.archived,
        preview,
        assistant_summary,
        branches,
        workspace_changes,
        child_workers,
        total_branch_count,
        total_file_count,
        session_contract,
        has_blocker,
        has_delivered,
        is_running,
        is_active,
        group_kpi,
    }
}

impl BoardDataSource for AgentHandle {
    fn build_board_snapshot(
        &self,
        projects: &[Project],
        visible_receipts: &[(AcceptedMessage, ReceiptState)],
        default_space_label: &str,
        unassigned_queue_title: &str,
        generation: u64,
        localize: &dyn Fn(&str) -> String,
    ) -> WorkBoardSnapshot {
        let sessions = self.list_sessions();
        let busy_ids: HashSet<String> =
            self.busy_session_ids().into_iter().collect();
        let overridden_hashes = self.overridden_artifact_hashes();
        let (mut session_contracts, work_contracts) =
            self.board_work_contracts(visible_receipts);
        let project_map: HashMap<&str, &Project> = projects
            .iter()
            .map(|project| (project.id.as_str(), project))
            .collect();

        // Group all non-terminal visible receipts (`Queued`/`Running`/`Retrying`/
        // `Quarantined`/`Dead`) by optional session owner without re-reading disk.
        let mut receipts_by_session: HashMap<
            Option<String>,
            Vec<(AcceptedMessage, ReceiptState)>,
        > = HashMap::new();
        let mut seen_receipt_ids = HashSet::new();
        for (receipt, state) in visible_receipts {
            if matches!(
                state,
                ReceiptState::Completed | ReceiptState::Cancelled
            ) {
                continue;
            }
            if seen_receipt_ids.insert(receipt.id.clone()) {
                let owner = receipt
                    .target_session_id
                    .clone()
                    .or_else(|| receipt.session_hint.clone());
                receipts_by_session
                    .entry(owner)
                    .or_default()
                    .push((receipt.clone(), *state));
            }
        }

        let mut groups = Vec::with_capacity(sessions.len());

        for summary in &sessions {
            let lines = self.session_lines(&summary.id);
            let is_router = self.session_role(&summary.id)
                == flowy_agent::SessionRole::Router;
            let child_works = if is_router {
                self.child_works(&summary.id)
            } else {
                Vec::new()
            };
            let is_busy = busy_ids.contains(&summary.id)
                || child_works.iter().any(|w| busy_ids.contains(&w.work_id));
            let status = self.session_status(&summary.id);
            let delivery_failed = matches!(status, TurnStatus::DeliveryFailed);
            let live_heartbeat = busy_ids
                .contains(&summary.id)
                .then(|| self.session_heartbeat(&summary.id))
                .flatten();
            let session_receipts = receipts_by_session
                .remove(&Some(summary.id.clone()))
                .unwrap_or_default();
            let session_contract = session_contracts
                .remove(&summary.id)
                .and_then(|status| WorkContractProgress::from_status(&status));

            let project = summary
                .project_id
                .as_deref()
                .and_then(|id| project_map.get(id).copied());

            let mut all_branches = project_session_branches(
                summary,
                &lines,
                busy_ids.contains(&summary.id),
                delivery_failed,
                live_heartbeat.as_ref(),
                &session_receipts,
                &overridden_hashes,
                session_contract.clone(),
                localize,
                |work_id| work_contracts.get(work_id).cloned(),
            );

            let child_workers = if child_works.is_empty() {
                Vec::new()
            } else {
                let (child_branches, child_workers) =
                    super::router_projection::project_router_children(
                        summary,
                        &child_works,
                        &overridden_hashes,
                        localize,
                        |wid| self.session_lines(wid),
                        |wid| {
                            receipts_by_session
                                .remove(&Some(wid.to_owned()))
                                .unwrap_or_default()
                        },
                        |wid| {
                            session_contracts
                                .get(wid)
                                .cloned()
                                .or_else(|| work_contracts.get(wid).cloned())
                                .or_else(|| self.session_work_status(wid))
                        },
                    );
                if !child_branches.is_empty() {
                    all_branches
                        .retain(|b| !matches!(b.phase, BranchPhase::Idle));
                    all_branches.extend(child_branches);
                }
                child_workers
            };

            let raw_preview = self.session_preview(&summary.id);
            groups.push(finalize_session_group(
                summary,
                &lines,
                all_branches,
                &child_works,
                child_workers,
                is_busy,
                session_contract,
                raw_preview,
                project,
                default_space_label,
            ));
        }

        // Surface all remaining unassigned or orphaned receipts (no session
        // owner, or owner session was deleted/not yet created) as one group so
        // nothing in AcceptanceQueue goes invisible on the board.
        let mut unassigned: Vec<(AcceptedMessage, ReceiptState)> =
            receipts_by_session.into_values().flatten().collect();
        unassigned.sort_by_key(|(r, _)| r.created_at_ms);
        if !unassigned.is_empty() {
            let mut branches: Vec<WorkBranchSummary> = unassigned
                .iter()
                .map(|(receipt, state)| {
                    let work_progress = work_contracts
                        .get(&receipt.id)
                        .and_then(WorkContractProgress::from_status);
                    let safe_text = redact_trace_text(&receipt.text);
                    WorkBranchSummary {
                        id: format!("receipt-{}", receipt.id),
                        owner_session_id: None,
                        skill_label: SkillLabel::Key("board.skill_queue"),
                        phase: BranchPhase::Queued {
                            receipt_id: receipt.id.clone(),
                            state: *state,
                        },
                        deliverable_name: clip_label(&safe_text, 28),
                        deliverable_path: None,
                        format: DeliverableFormat::Other,
                        is_workspace_change: false,
                        progress_pct: receipt_progress_pct(*state),
                        detail: clip_label(&safe_text, 96),
                        work_progress,
                    }
                })
                .collect();
            let has_blocker =
                branches.iter().any(|branch| branch.phase.is_blocker());
            let is_running =
                branches.iter().any(|branch| branch.phase.is_running());
            let is_active = has_blocker || is_running;
            let mut group_kpi = BoardKpiSummary::default();
            for branch in &branches {
                accumulate_kpi(&mut group_kpi, branch);
            }
            branches.sort_by_key(|branch| branch_sort_rank(&branch.phase));
            let total_branch_count = branches.len();
            groups.insert(
                0,
                SessionBoardGroup {
                    session_id: UNASSIGNED_QUEUE_SESSION_ID.to_owned(),
                    title: unassigned_queue_title.to_owned(),
                    project_id: None,
                    project_name: None,
                    workspace_display: default_space_label.to_owned(),
                    updated_at: unassigned
                        .iter()
                        .map(|(r, _)| normalize_epoch_ms(r.created_at_ms))
                        .max()
                        .unwrap_or(0),
                    pinned: false,
                    archived: false,
                    preview: None,
                    assistant_summary: None,
                    branches,
                    workspace_changes: Vec::new(),
                    child_workers: Vec::new(),
                    total_branch_count,
                    total_file_count: total_branch_count,
                    session_contract: None,
                    has_blocker,
                    has_delivered: false,
                    is_running,
                    is_active,
                    group_kpi,
                },
            );
        }

        // Priority sort: pinned first, then blockers, then running, then newest updated_at.
        groups.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.has_blocker.cmp(&a.has_blocker))
                .then_with(|| b.is_running.cmp(&a.is_running))
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });

        let generated_at_ms = now_epoch_ms();
        let tz_offset_secs = local_tz_offset_secs();
        let group_refs: Vec<&SessionBoardGroup> = groups.iter().collect();
        let kpi = BoardKpiSummary::from_groups_in_tz(
            &group_refs,
            generated_at_ms,
            tz_offset_secs,
        );
        let daily_7d = compute_daily_7d_in_tz(
            &group_refs,
            generated_at_ms,
            tz_offset_secs,
        );

        WorkBoardSnapshot {
            kpi,
            sessions: groups,
            daily_7d,
            generated_at_ms,
            tz_offset_secs,
            generation,
        }
    }
}

const fn accumulate_kpi(kpi: &mut BoardKpiSummary, branch: &WorkBranchSummary) {
    if branch.is_workspace_change {
        kpi.workspace_change_count += 1;
        return;
    }
    match &branch.phase {
        BranchPhase::NeedsApproval { .. }
        | BranchPhase::DeliveryFailed
        | BranchPhase::Queued {
            state: ReceiptState::Dead | ReceiptState::Quarantined,
            ..
        } => {
            kpi.total_branches += 1;
            kpi.approval_count += 1;
        }
        BranchPhase::Running { .. }
        | BranchPhase::Queued {
            state:
                ReceiptState::Queued
                | ReceiptState::Running
                | ReceiptState::Retrying,
            ..
        } => {
            kpi.total_branches += 1;
            kpi.running_count += 1;
        }
        BranchPhase::Delivered { .. } => {
            kpi.total_branches += 1;
            kpi.delivered_count += 1;
            kpi.formal_deliverable_count += 1;
            match branch.format {
                DeliverableFormat::Sheet => kpi.sheet_count += 1,
                DeliverableFormat::Slides => kpi.slides_count += 1,
                DeliverableFormat::Doc => kpi.doc_count += 1,
                DeliverableFormat::Pdf => kpi.pdf_count += 1,
                DeliverableFormat::Markdown => kpi.markdown_count += 1,
                DeliverableFormat::CodeWeb => kpi.code_count += 1,
                DeliverableFormat::Other => kpi.other_count += 1,
            }
        }
        BranchPhase::Queued {
            state: ReceiptState::Completed | ReceiptState::Cancelled,
            ..
        }
        | BranchPhase::Idle => {}
    }
}

fn matches_required_label(
    file_name: &str,
    contract: Option<&WorkContractProgress>,
) -> bool {
    let Some(contract) = contract else {
        return false;
    };
    contract.required_labels.iter().any(|label| {
        let label = label.trim();
        if label.is_empty() {
            return false;
        }
        if let Some(ext) = label.strip_prefix("*.") {
            Path::new(file_name)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        } else {
            basename(label).eq_ignore_ascii_case(file_name)
        }
    })
}

fn build_idle_branch(
    summary: &SessionSummary,
    lines: &[ChatLine],
) -> WorkBranchSummary {
    let preview = lines
        .iter()
        .rev()
        .find_map(|line| match line {
            ChatLine::Assistant { text } | ChatLine::User { text } => {
                let stripped = flowy_agent::strip_markdown_inline(text);
                let trimmed = stripped.trim();
                (!trimmed.is_empty()).then(|| {
                    let safe = redact_trace_text(trimmed);
                    clip_label(&safe, 48)
                })
            }
            _ => None,
        })
        .unwrap_or_else(|| redact_trace_text(&summary.title));
    WorkBranchSummary {
        id: format!("{}-idle", summary.id),
        owner_session_id: None,
        skill_label: SkillLabel::Key("chat.router_name"),
        phase: BranchPhase::Idle,
        deliverable_name: clip_label(&redact_trace_text(&summary.title), 24),
        deliverable_path: None,
        format: DeliverableFormat::Other,
        is_workspace_change: false,
        progress_pct: 100.0,
        detail: preview,
        work_progress: None,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "pure snapshot projection helper keeping BoardDataSource testable"
)]
pub(super) fn project_session_branches<F>(
    summary: &SessionSummary,
    lines: &[ChatLine],
    is_busy: bool,
    delivery_failed: bool,
    live_heartbeat: Option<&flowy_agent::BranchHeartbeat>,
    queued_receipts: &[(AcceptedMessage, ReceiptState)],
    overridden_hashes: &std::collections::BTreeSet<String>,
    session_contract: Option<WorkContractProgress>,
    localize: &dyn Fn(&str) -> String,
    work_status_fn: F,
) -> Vec<WorkBranchSummary>
where
    F: Fn(&str) -> Option<WorkStatus>,
{
    let mut branches = Vec::new();
    let mut seen_paths = HashSet::new();

    // 1. Collect explicit verification & delivery artifacts (newest first, dedup by normalized path).
    for (idx, line) in lines.iter().enumerate().rev() {
        if let ChatLine::Verification {
            report_id,
            artifact_path,
            artifact_hash,
            ok,
            issues,
            semantic_hint,
            override_used,
            provenance_warnings,
            ..
        } = line
        {
            let norm_path = display_path_str(artifact_path);
            if !seen_paths.insert(norm_path.clone()) {
                continue;
            }
            let overridden =
                *override_used || overridden_hashes.contains(artifact_hash);
            let name = basename(artifact_path);
            let format = DeliverableFormat::from_path(artifact_path);
            let hash_short: String = artifact_hash.chars().take(8).collect();
            let safe_path = redact_trace_text(&norm_path);
            let safe_hint = semantic_hint.as_deref().map(redact_trace_text);
            let safe_warnings: Vec<String> = provenance_warnings
                .iter()
                .map(|w| redact_trace_text(w))
                .collect();
            let (phase, progress_pct, detail) = if *ok || overridden {
                (
                    BranchPhase::Delivered {
                        override_used: overridden,
                        hash_short,
                        provenance_warnings: safe_warnings,
                    },
                    100.0,
                    safe_hint.unwrap_or_else(|| safe_path.clone()),
                )
            } else {
                let issue_count = issues.len();
                let detail_msg = format!("{safe_path} ({issue_count})");
                (
                    BranchPhase::NeedsApproval {
                        report_id: report_id.clone(),
                        artifact_path: artifact_path.clone(),
                        summary: detail_msg.clone(),
                        issues: issues.clone(),
                        semantic_hint: safe_hint,
                        provenance_warnings: safe_warnings,
                    },
                    85.0,
                    detail_msg,
                )
            };
            branches.push(WorkBranchSummary {
                id: format!("{}-v-{idx}", summary.id),
                owner_session_id: None,
                skill_label: infer_skill_label(format, None),
                phase,
                deliverable_name: name,
                deliverable_path: Some(artifact_path.clone()),
                format,
                is_workspace_change: false,
                progress_pct,
                detail,
                work_progress: None,
            });
        }
    }

    // 2. Include file changes not already covered by Verification rows.
    for (idx, line) in lines.iter().enumerate().rev() {
        match line {
            ChatLine::FileChanges { files } => {
                for (f_ix, file) in files.iter().enumerate() {
                    let norm_path = display_path_str(&file.path);
                    if !seen_paths.insert(norm_path.clone()) {
                        continue;
                    }
                    let file_name = basename(&file.path);
                    let format = DeliverableFormat::from_path(&file.path);
                    let is_workspace_change = matches!(
                        format,
                        DeliverableFormat::CodeWeb | DeliverableFormat::Other
                    ) && !matches_required_label(
                        &file_name,
                        session_contract.as_ref(),
                    );
                    branches.push(WorkBranchSummary {
                        id: format!("{}-fc-{idx}-{f_ix}", summary.id),
                        owner_session_id: None,
                        skill_label: infer_skill_label(format, None),
                        phase: BranchPhase::Delivered {
                            override_used: false,
                            hash_short: String::new(),
                            provenance_warnings: Vec::new(),
                        },
                        deliverable_name: file_name,
                        deliverable_path: Some(file.path.clone()),
                        format,
                        is_workspace_change,
                        progress_pct: 100.0,
                        detail: redact_trace_text(&norm_path),
                        work_progress: None,
                    });
                }
            }
            ChatLine::FileChange { path, .. } => {
                let norm_path = display_path_str(path);
                if seen_paths.insert(norm_path.clone()) {
                    let file_name = basename(path);
                    let format = DeliverableFormat::from_path(path);
                    let is_workspace_change = matches!(
                        format,
                        DeliverableFormat::CodeWeb | DeliverableFormat::Other
                    ) && !matches_required_label(
                        &file_name,
                        session_contract.as_ref(),
                    );
                    branches.push(WorkBranchSummary {
                        id: format!("{}-f-{idx}", summary.id),
                        owner_session_id: None,
                        skill_label: infer_skill_label(format, None),
                        phase: BranchPhase::Delivered {
                            override_used: false,
                            hash_short: String::new(),
                            provenance_warnings: Vec::new(),
                        },
                        deliverable_name: file_name,
                        deliverable_path: Some(path.clone()),
                        format,
                        is_workspace_change,
                        progress_pct: 100.0,
                        detail: redact_trace_text(&norm_path),
                        work_progress: None,
                    });
                }
            }
            _ => {}
        }
    }

    disambiguate_deliverable_names(&mut branches);

    // 2.5 Durable delivery commit failed after turn completion.
    if delivery_failed {
        let safe_title = redact_trace_text(&summary.title);
        branches.insert(
            0,
            WorkBranchSummary {
                id: format!("{}-delivery-failed", summary.id),
                owner_session_id: None,
                skill_label: SkillLabel::Key("chat.status_delivery_failed"),
                phase: BranchPhase::DeliveryFailed,
                deliverable_name: clip_label(&safe_title, 28),
                deliverable_path: None,
                format: DeliverableFormat::Other,
                is_workspace_change: false,
                progress_pct: 90.0,
                detail: clip_label(&safe_title, 96),
                work_progress: None,
            },
        );
    }

    // 3. Running live turn (surface latest tool call / reasoning step).
    // Also absorb any ReceiptState::Running receipt's work_progress onto `-live`
    // so busy queued runs never emit duplicate running branches (P0-4).
    if is_busy {
        let last_user = lines
            .iter()
            .rposition(|line| matches!(line, ChatLine::User { .. }))
            .unwrap_or(0);
        let turn_lines = &lines[last_user..];
        let total_tools = turn_lines
            .iter()
            .filter(|line| matches!(line, ChatLine::ToolCall { .. }))
            .count();
        let done_tools = turn_lines
            .iter()
            .filter(|line| {
                matches!(line, ChatLine::ToolCall { running: false, .. })
            })
            .count();
        let active_tool = turn_lines.iter().rev().find_map(|line| match line {
            ChatLine::ToolCall {
                name,
                target,
                running: true,
                ..
            } => Some((name.as_str(), target.as_str())),
            _ => None,
        });
        let pct = if total_tools == 0 {
            35.0
        } else {
            ((done_tools as f32 / (total_tools as f32 + 0.5)) * 90.0)
                .clamp(20.0, 92.0)
        };
        let safe_title = redact_trace_text(&summary.title);
        let heartbeat_detail = live_heartbeat.and_then(|hb| {
            crate::pending_card::format_heartbeat_line_localized(hb, localize)
                .map(|text| redact_trace_text(&text))
        });
        let (skill_label, step_label, detail) =
            if let Some((name, target)) = active_tool {
                (
                    infer_skill_label(DeliverableFormat::Other, Some(name)),
                    name.to_owned(),
                    heartbeat_detail.unwrap_or_else(|| {
                        redact_trace_text(&display_path_str(target))
                    }),
                )
            } else {
                (
                    SkillLabel::Key("chat.router_name"),
                    String::new(),
                    heartbeat_detail.unwrap_or_else(|| safe_title.clone()),
                )
            };
        let absorbed_work_progress = queued_receipts
            .iter()
            .filter(|(_, state)| *state == ReceiptState::Running)
            .find_map(|(receipt, _)| {
                work_status_fn(&receipt.id)
                    .and_then(|s| WorkContractProgress::from_status(&s))
            });
        branches.insert(
            0,
            WorkBranchSummary {
                id: format!("{}-live", summary.id),
                owner_session_id: None,
                skill_label,
                phase: BranchPhase::Running {
                    step_label: step_label.clone(),
                    percent: pct,
                },
                deliverable_name: if step_label.is_empty() {
                    clip_label(&safe_title, 24)
                } else {
                    step_label
                },
                deliverable_path: None,
                format: DeliverableFormat::Other,
                is_workspace_change: false,
                progress_pct: pct,
                detail,
                work_progress: absorbed_work_progress,
            },
        );
    }

    // 4. Deferred / retrying / dead receipts associated with this session.
    for (receipt, state) in queued_receipts {
        if is_busy && *state == ReceiptState::Running {
            continue;
        }
        let work_progress = work_status_fn(&receipt.id)
            .and_then(|s| WorkContractProgress::from_status(&s));
        let safe_text = super::redact_trace_text(&receipt.text);
        branches.push(WorkBranchSummary {
            id: format!("{}-q-{}", summary.id, receipt.id),
            owner_session_id: None,
            skill_label: SkillLabel::Key("board.skill_queue"),
            phase: BranchPhase::Queued {
                receipt_id: receipt.id.clone(),
                state: *state,
            },
            deliverable_name: clip_label(&safe_text, 24),
            deliverable_path: None,
            format: DeliverableFormat::Other,
            is_workspace_change: false,
            progress_pct: receipt_progress_pct(*state),
            detail: clip_label(&safe_text, 96),
            work_progress,
        });
    }

    // 5. Fallback single idle branch if the session had pure Q&A without files.
    if branches.is_empty() {
        branches.push(build_idle_branch(summary, lines));
    }

    if let Some(contract) = session_contract {
        let target_idx = branches
            .iter()
            .position(|b| matches!(b.phase, BranchPhase::NeedsApproval { .. }))
            .or_else(|| {
                branches.iter().position(|b| {
                    matches!(b.phase, BranchPhase::Running { .. })
                })
            })
            .or_else(|| {
                branches.iter().position(|b| {
                    !matches!(b.phase, BranchPhase::Queued { .. })
                })
            });
        if let Some(idx) = target_idx
            && branches[idx].work_progress.is_none()
        {
            branches[idx].work_progress = Some(contract);
        }
    }

    branches
}

/// Progressively disambiguates colliding deliverable basenames across all file
/// branches (including monorepo `crates/a/src/lib.rs` vs `crates/b/src/lib.rs`,
/// cross-drive `C:\report.xlsx` vs `D:\report.xlsx`, and blocker+delivered
/// pairs) without ever dropping distinct real deliverables before KPI totals.
fn disambiguate_deliverable_names(branches: &mut [WorkBranchSummary]) {
    let mut depth: usize = 1;
    loop {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, branch) in branches.iter().enumerate() {
            if branch.deliverable_path.is_some() {
                groups
                    .entry(branch.deliverable_name.to_ascii_lowercase())
                    .or_default()
                    .push(idx);
            }
        }
        let mut widened_any = false;
        for indices in groups.into_values() {
            if indices.len() <= 1 {
                continue;
            }
            for idx in indices {
                if let Some(path) = branches[idx].deliverable_path.as_deref()
                    && let Some(wider) = path_suffix_label(path, depth + 1)
                    && wider != branches[idx].deliverable_name
                {
                    branches[idx].deliverable_name = wider;
                    widened_any = true;
                }
            }
        }
        depth += 1;
        if !widened_any || depth >= 6 {
            break;
        }
    }
}

fn is_redundant_preview(title: &str, preview: &str) -> bool {
    let t = title.trim_end_matches(['…', '.', ' ']);
    let p = preview.trim_end_matches(['…', '.', ' ']);
    if t.is_empty() || p.is_empty() || t == p {
        return true;
    }
    if p.starts_with(t) || t.starts_with(p) {
        return true;
    }
    let prefix_len = t.chars().count().min(p.chars().count()).min(12);
    prefix_len >= 8 && t.chars().take(prefix_len).eq(p.chars().take(prefix_len))
}

fn path_suffix_label(path: &str, segments: usize) -> Option<String> {
    let clean = display_path_str(path);
    let trimmed = clean.trim_end_matches(['/', '\\']);
    let parts: Vec<&str> = trimmed
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() <= 1 {
        return None;
    }
    let take = segments.min(parts.len());
    let formatted: Vec<String> = parts[parts.len() - take..]
        .iter()
        .map(|part| shorten_internal_segment(part))
        .collect();
    Some(formatted.join("/"))
}

fn shorten_internal_segment(part: &str) -> String {
    if let Some(rest) = part.strip_prefix("submission-")
        && rest.len() > 8
    {
        let short: String = rest.chars().take(6).collect();
        return format!("sub-{short}");
    }
    part.to_owned()
}

fn infer_skill_label(
    format: DeliverableFormat,
    tool: Option<&str>,
) -> SkillLabel {
    if let Some(name) = tool {
        return match name {
            "create_deck" | "verify_deck" => {
                SkillLabel::Key("library.category.slides")
            }
            "read_file" | "write_file" | "edit_file" => {
                SkillLabel::Key("deliverables.kind_file")
            }
            "glob" | "grep" => SkillLabel::Key("dock.search"),
            "bash" | "shell" => SkillLabel::Key("board.skill_shell"),
            other => SkillLabel::Literal(other.to_owned()),
        };
    }
    match format {
        DeliverableFormat::Sheet => SkillLabel::Key("deliverables.kind_data"),
        DeliverableFormat::Slides => SkillLabel::Key("library.category.slides"),
        DeliverableFormat::Doc
        | DeliverableFormat::Pdf
        | DeliverableFormat::Markdown => {
            SkillLabel::Key("deliverables.kind_document")
        }
        DeliverableFormat::CodeWeb => SkillLabel::Key("deliverables.kind_code"),
        DeliverableFormat::Other => SkillLabel::Key("board.skill_work"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowy_agent::VerificationIssue;

    #[test]
    fn projection_classifies_failed_verification_as_blocker_with_report_id() {
        let summary = SessionSummary {
            id: "s1".into(),
            title: "Q3 Report".into(),
            updated_at: 42,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let lines = vec![
            ChatLine::Verification {
                report_id: "rep-001".into(),
                artifact_path: r"\\?\D:\Work\Q3.xlsx".into(),
                artifact_hash: "8f41c9e2aabbccdd".into(),
                ok: false,
                issues: vec![VerificationIssue {
                    code: "FORMULA_ERR".into(),
                    field: "Sheet1!B4".into(),
                }],
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            },
            ChatLine::Verification {
                report_id: "rep-002".into(),
                artifact_path: r"D:\Work\Deck.pptx".into(),
                artifact_hash: "3c92a01b99887766".into(),
                ok: true,
                issues: vec![],
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            },
        ];
        let contract = WorkContractProgress {
            work_id: "sub-01".into(),
            contract_revision: Some(1),
            required_labels: vec!["Q3.xlsx".into(), "Deck.pptx".into()],
            delivered_count: 1,
            missing_items: vec!["Q3.xlsx".into()],
            outcome: Some(SubmissionOutcome::Partial {
                missing: vec!["Q3.xlsx".into()],
            }),
        };
        let branches = project_session_branches(
            &summary,
            &lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            Some(contract.clone()),
            &|k| k.to_owned(),
            |_| None,
        );
        assert_eq!(branches.len(), 2);
        assert!(branches.iter().any(|b| matches!(
            &b.phase,
            BranchPhase::NeedsApproval { report_id, issues, .. }
                if report_id == "rep-001" && issues.len() == 1
        )));
        assert!(branches.iter().any(|b| matches!(
            &b.phase,
            BranchPhase::Delivered { override_used: false, hash_short, .. }
                if hash_short == "3c92a01b"
        )));
        let blocker = branches
            .iter()
            .find(|b| matches!(b.phase, BranchPhase::NeedsApproval { .. }))
            .unwrap();
        assert!(!blocker.detail.contains(r"\\?\"));
        assert_eq!(blocker.work_progress, Some(contract));
    }

    #[test]
    fn work_contract_progress_projects_partial_missing_and_dead_receipts_as_blockers()
     {
        let status = WorkStatus {
            work_id: "rcpt-42".into(),
            session_id: Some("s1".into()),
            submission_id: Some("sub-42".into()),
            contract_revision: Some(2),
            receipt_state: Some(ReceiptState::Dead),
            outcome: Some(SubmissionOutcome::Partial {
                missing: vec!["summary.docx".into()],
            }),
            required_items: vec![
                RequiredDeliverable {
                    filename: Some("deck.pptx".into()),
                    extension: "pptx".into(),
                },
                RequiredDeliverable {
                    filename: Some("summary.docx".into()),
                    extension: "docx".into(),
                },
            ],
            delivered_artifact_ids: vec!["art-1".into()],
            missing_items: vec!["summary.docx".into()],
        };
        let progress = WorkContractProgress::from_status(&status)
            .expect("non-empty contract produces progress");
        assert_eq!(progress.delivered_count, 1);
        assert_eq!(progress.required_labels, vec!["deck.pptx", "summary.docx"]);
        assert_eq!(progress.missing_items, vec!["summary.docx"]);

        let dead_phase = BranchPhase::Queued {
            receipt_id: "rcpt-42".into(),
            state: ReceiptState::Dead,
        };
        assert!(dead_phase.is_blocker());
        assert!(!dead_phase.is_running());

        let retrying_phase = BranchPhase::Queued {
            receipt_id: "rcpt-43".into(),
            state: ReceiptState::Retrying,
        };
        assert!(!retrying_phase.is_blocker());
        assert!(retrying_phase.is_running());
        assert!(BranchPhase::DeliveryFailed.is_blocker());
    }

    fn temp_board_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-board-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn idle_qa_session_keeps_kpi_donut_counts_consistent() {
        let summary = SessionSummary {
            id: "qa-1".into(),
            title: "Pure Q&A".into(),
            updated_at: 100,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let lines = vec![
            ChatLine::User {
                text: "What is 1 + 1?".into(),
            },
            ChatLine::Assistant { text: "2".into() },
        ];
        let branches = project_session_branches(
            &summary,
            &lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &|k| k.to_owned(),
            |_| None,
        );
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].phase, BranchPhase::Idle);

        let mut kpi = BoardKpiSummary {
            total_sessions: 1,
            ..BoardKpiSummary::default()
        };
        for branch in &branches {
            accumulate_kpi(&mut kpi, branch);
        }
        assert_eq!(kpi.total_branches, 0);
        assert_eq!(
            kpi.total_branches,
            kpi.approval_count + kpi.running_count + kpi.delivered_count
        );
    }

    #[test]
    fn orphan_receipt_for_deleted_session_surfaces_in_unassigned_group() {
        let root = temp_board_root("orphan-receipt");
        let queue = flowy_agent::AcceptanceQueue::new(root.clone());
        let orphan = queue
            .accept(Some("deleted-session-404"), "orphan task", 10)
            .unwrap();
        let unowned = queue.accept(None, "unassigned task", 20).unwrap();
        let handle = AgentHandle::with_config(
            root.clone(),
            flowy_agent::LlmConfig::default(),
        );
        let visible = vec![
            (orphan, ReceiptState::Queued),
            (unowned, ReceiptState::Dead),
        ];
        let snap = handle.build_board_snapshot(
            &[],
            &visible,
            "Default space",
            "Background tasks",
            1,
            &|k| k.to_owned(),
        );
        let group = snap
            .sessions
            .iter()
            .find(|g| g.session_id == UNASSIGNED_QUEUE_SESSION_ID)
            .expect("orphan and unowned receipts surface in unassigned group");
        assert_eq!(group.title, "Background tasks");
        assert_eq!(group.workspace_display, "Default space");
        assert_eq!(group.branches.len(), 2);
        assert_eq!(group.total_branch_count, 2);
        assert_eq!(snap.kpi.total_branches, 2);
        assert_eq!(snap.kpi.running_count, 1);
        assert_eq!(snap.kpi.approval_count, 1);
        assert_eq!(
            snap.kpi.total_branches,
            snap.kpi.approval_count
                + snap.kpi.running_count
                + snap.kpi.delivered_count
        );
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn kpi_counts_all_branches_and_retains_more_than_six_formal_deliverables() {
        let summary = SessionSummary {
            id: "many-1".into(),
            title: "Bulk Deliverables".into(),
            updated_at: 200,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let mut lines = Vec::new();
        for i in 0..8 {
            lines.push(ChatLine::FileChange {
                path: format!(r"D:\Work\report_{i}.docx"),
                change: flowy_agent::FileChangeKind::Add,
                added: 10,
                removed: 0,
                binary: false,
            });
        }
        lines.push(ChatLine::Verification {
            report_id: "rep-blocker".into(),
            artifact_path: r"D:\Work\summary.xlsx".into(),
            artifact_hash: "1122334455667788".into(),
            ok: false,
            issues: vec![VerificationIssue {
                code: "ERR".into(),
                field: "A1".into(),
            }],
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        });
        let all_branches = project_session_branches(
            &summary,
            &lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &|k| k.to_owned(),
            |_| None,
        );
        let group = finalize_session_group(
            &summary,
            &lines,
            all_branches,
            &[],
            Vec::new(),
            false,
            None,
            None,
            None,
            "Default",
        );

        assert_eq!(group.group_kpi.total_branches, 9);
        assert_eq!(group.group_kpi.delivered_count, 8);
        assert_eq!(group.group_kpi.formal_deliverable_count, 8);
        assert_eq!(group.group_kpi.approval_count, 1);
        assert_eq!(group.group_kpi.doc_count, 8);
        assert_eq!(group.branches.len(), 9);
        assert_eq!(group.total_branch_count, 9);
        assert_eq!(group.total_file_count, 9);
        assert!(matches!(
            group.branches[0].phase,
            BranchPhase::NeedsApproval { .. }
        ));
    }

    #[test]
    fn work_contract_progress_ignores_empty_completed_qa_outcome() {
        let status = WorkStatus {
            work_id: "qa-empty".into(),
            session_id: Some("s-qa".into()),
            submission_id: Some("sub-qa".into()),
            contract_revision: None,
            receipt_state: Some(ReceiptState::Completed),
            outcome: Some(SubmissionOutcome::Completed),
            required_items: Vec::new(),
            delivered_artifact_ids: Vec::new(),
            missing_items: Vec::new(),
        };
        assert!(
            WorkContractProgress::from_status(&status).is_none(),
            "pure Q&A with 0 required/delivered items must not render a 0/0 contract badge"
        );
    }

    #[test]
    fn board_projection_redacts_secrets_in_receipt_and_qa_previews() {
        let summary = SessionSummary {
            id: "s-sec".into(),
            title: "check api_key=sk-secret-do-not-leak".into(),
            updated_at: 42,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let lines = vec![ChatLine::User {
            text: "Bearer tok_1234567890abcdef".into(),
        }];
        let branches = project_session_branches(
            &summary,
            &lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &|k| k.to_owned(),
            |_| None,
        );
        assert!(!branches[0].deliverable_name.contains("sk-secret"));
        assert!(!branches[0].detail.contains("tok_1234567890abcdef"));

        // Running tool call target & DeliveryFailed projection.
        let running_lines = vec![ChatLine::ToolCall {
            call_id: "t1".into(),
            name: "shell".into(),
            target: "API_KEY=sk-live-secret-999999 curl https://user:pass@host"
                .into(),
            running: true,
        }];
        let en_localize = |k: &str| match k {
            "chat.status_running" => "Running".into(),
            "chat.thinking" => "Thinking".into(),
            other => other.to_owned(),
        };
        let running_branches = project_session_branches(
            &summary,
            &running_lines,
            true,
            true,
            Some(&flowy_agent::BranchHeartbeat {
                last_completed: Some("read config".into()),
                last_completed_at_ms: Some(123),
                current_action: "running shell".into(),
                next_check_hint: None,
                awaiting_evidence: false,
            }),
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &en_localize,
            |_| None,
        );
        assert!(
            running_branches
                .iter()
                .any(|b| matches!(b.phase, BranchPhase::DeliveryFailed))
        );
        let live = running_branches
            .iter()
            .find(|b| matches!(b.phase, BranchPhase::Running { .. }))
            .expect("live branch");
        assert!(live.detail.contains("read config · running shell"));
        assert!(!live.detail.contains("sk-live-secret-999999"));
        assert!(!live.detail.contains("user:pass"));

        let zh_localize = |k: &str| match k {
            "chat.status_running" => "运行中".into(),
            "chat.thinking" => "思考中".into(),
            other => other.to_owned(),
        };
        let zh_branches = project_session_branches(
            &summary,
            &running_lines,
            true,
            false,
            Some(&flowy_agent::BranchHeartbeat {
                last_completed: Some("read config".into()),
                last_completed_at_ms: Some(123),
                current_action: "evaluating tool evidence".into(),
                next_check_hint: Some("raw hint".into()),
                awaiting_evidence: false,
            }),
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &zh_localize,
            |_| None,
        );
        let zh_live = zh_branches
            .iter()
            .find(|b| matches!(b.phase, BranchPhase::Running { .. }))
            .expect("zh live branch");
        assert_eq!(zh_live.detail, "read config · 运行中");
        assert!(!zh_live.detail.contains("evaluating tool evidence"));

        let thinking_branches = project_session_branches(
            &summary,
            &[],
            true,
            false,
            Some(&flowy_agent::BranchHeartbeat {
                last_completed: None,
                last_completed_at_ms: None,
                current_action: "thinking".into(),
                next_check_hint: None,
                awaiting_evidence: false,
            }),
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &zh_localize,
            |_| None,
        );
        let thinking_live = thinking_branches
            .iter()
            .find(|b| matches!(b.phase, BranchPhase::Running { .. }))
            .expect("thinking live branch");
        assert_eq!(thinking_live.detail, "思考中");
    }

    #[test]
    fn normalize_epoch_ms_and_daily_7d_handle_ms_seconds_and_archived_exclusion()
     {
        let now_ms = 1_760_000_000_000_u64;
        // Legacy Unix seconds are upgraded to ms; ms timestamps stay unchanged.
        assert_eq!(normalize_epoch_ms(1_760_000_000), 1_760_000_000_000);
        assert_eq!(normalize_epoch_ms(now_ms), now_ms);
        assert_eq!(normalize_epoch_ms(0), 0);

        assert!(!is_older_than_7d_in_tz(now_ms - DAY_MS, now_ms, 0));
        assert!(is_older_than_7d_in_tz(now_ms - 8 * DAY_MS, now_ms, 0));
        assert!(!is_older_than_7d_in_tz(0, now_ms, 0));

        let make_group =
            |id: &str, updated_at: u64, active: bool, archived: bool| {
                SessionBoardGroup {
                    session_id: id.into(),
                    title: id.into(),
                    project_id: None,
                    project_name: None,
                    workspace_display: String::new(),
                    updated_at,
                    pinned: false,
                    archived,
                    preview: None,
                    assistant_summary: None,
                    branches: Vec::new(),
                    workspace_changes: Vec::new(),
                    child_workers: Vec::new(),
                    total_branch_count: 0,
                    total_file_count: 0,
                    session_contract: None,
                    has_blocker: false,
                    has_delivered: false,
                    is_running: active,
                    is_active: active,
                    group_kpi: BoardKpiSummary::default(),
                }
            };
        let g_today = make_group("today", now_ms - 60_000, true, false);
        let g_legacy_2d =
            make_group("two-days", (now_ms - 2 * DAY_MS) / 1_000, false, false);
        let g_old = make_group("old", now_ms - 9 * DAY_MS, false, false);
        let g_archived = make_group("arch", now_ms - 10_000, false, true);
        let refs = vec![&g_today, &g_legacy_2d, &g_old, &g_archived];

        let kpi = BoardKpiSummary::from_groups(&refs);
        assert_eq!(kpi.total_sessions, 3);
        assert_eq!(kpi.active_sessions, 1);
        assert_eq!(kpi.archived_sessions, 1);

        let buckets = compute_daily_7d_in_tz(&refs, now_ms, 0);
        assert_eq!(buckets.len(), 7);
        assert_eq!(buckets[6].day_offset, 0);
        assert_eq!(buckets[6].active_or_blocked, 1);
        assert_eq!(buckets[6].completed_or_delivered, 0);
        assert_eq!(buckets[4].day_offset, 2);
        assert_eq!(buckets[4].completed_or_delivered, 1);
        let total: usize = buckets.iter().map(DailyActivityBucket::total).sum();
        assert_eq!(
            total, 2,
            "older-than-7d and archived sessions stay out of 7d chart"
        );
    }

    #[test]
    fn duplicate_basenames_disambiguate_and_classify_code_web_deliverables() {
        let summary = SessionSummary {
            id: "web-1".into(),
            title: "Portfolio Site".into(),
            updated_at: 1_760_000_000_000,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let lines = vec![
            ChatLine::FileChange {
                path: r"D:\Site\src\styles\tokens.css".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 40,
                removed: 0,
                binary: false,
            },
            ChatLine::FileChange {
                path: r"D:\Site\dist\tokens.css".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 40,
                removed: 0,
                binary: false,
            },
            // Monorepo same-parent paths & cross-drive files must both be
            // preserved and disambiguated without dropping real deliverables.
            ChatLine::FileChange {
                path: r"D:\Backup\dist\tokens.css".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 40,
                removed: 0,
                binary: false,
            },
            ChatLine::FileChange {
                path: r"C:\index.html".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 20,
                removed: 0,
                binary: false,
            },
            ChatLine::FileChange {
                path: r"D:\index.html".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 20,
                removed: 0,
                binary: false,
            },
        ];
        let contract = WorkContractProgress {
            work_id: "web-contract".into(),
            contract_revision: Some(1),
            required_labels: vec!["tokens.css".into(), "index.html".into()],
            delivered_count: 2,
            missing_items: Vec::new(),
            outcome: Some(SubmissionOutcome::Completed),
        };
        let branches = project_session_branches(
            &summary,
            &lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            Some(contract),
            &|k| k.to_owned(),
            |_| None,
        );
        let names: Vec<&str> = branches
            .iter()
            .map(|b| b.deliverable_name.as_str())
            .collect();
        assert_eq!(
            branches.len(),
            5,
            "no distinct deliverable may be dropped: {names:?}"
        );
        assert!(names.contains(&"styles/tokens.css"), "{names:?}");
        assert!(names.contains(&"Site/dist/tokens.css"), "{names:?}");
        assert!(names.contains(&"Backup/dist/tokens.css"), "{names:?}");
        assert!(names.contains(&"C:/index.html"), "{names:?}");
        assert!(names.contains(&"D:/index.html"), "{names:?}");
        assert!(
            branches
                .iter()
                .all(|b| b.format == DeliverableFormat::CodeWeb
                    && !b.is_workspace_change)
        );

        let mut kpi = BoardKpiSummary::default();
        for b in &branches {
            accumulate_kpi(&mut kpi, b);
        }
        assert_eq!(kpi.code_count, 5);
        assert_eq!(kpi.formal_deliverable_count, 5);
        assert_eq!(kpi.workspace_change_count, 0);
        assert_eq!(kpi.other_count, 0);

        // Without a contract, unverified CodeWeb changes still disambiguate
        // cleanly but bucket as workspace changes.
        let uncontracted = project_session_branches(
            &summary,
            &lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &|k| k.to_owned(),
            |_| None,
        );
        assert!(uncontracted.iter().all(|b| b.is_workspace_change));
        let mut wc_kpi = BoardKpiSummary::default();
        for b in &uncontracted {
            accumulate_kpi(&mut wc_kpi, b);
        }
        assert_eq!(wc_kpi.workspace_change_count, 5);
        assert_eq!(wc_kpi.code_count, 0);
        assert_eq!(wc_kpi.formal_deliverable_count, 0);
    }

    #[test]
    fn local_calendar_7d_buckets_respect_tz_midnight_and_match_day_filter() {
        // UTC+8 = +28800s.
        // now = 2026-11-26 01:00:00 +08:00 (2026-11-25 17:00:00Z).
        let tz = 8 * 3600;
        let utc_ms = |y: i32, m: u32, d: u32, h: u32, min: u32| -> u64 {
            let dt = chrono::FixedOffset::east_opt(tz)
                .unwrap()
                .with_ymd_and_hms(y, m, d, h, min, 0)
                .single()
                .unwrap();
            u64::try_from(dt.timestamp_millis()).unwrap()
        };
        let now_ms = utc_ms(2026, 11, 26, 1, 0);
        let make_group =
            |id: &str, updated_at: u64, active: bool| SessionBoardGroup {
                session_id: id.into(),
                title: id.into(),
                project_id: None,
                project_name: None,
                workspace_display: String::new(),
                updated_at,
                pinned: false,
                archived: false,
                preview: None,
                assistant_summary: None,
                branches: Vec::new(),
                workspace_changes: Vec::new(),
                child_workers: Vec::new(),
                total_branch_count: 0,
                total_file_count: 0,
                session_contract: None,
                has_blocker: false,
                has_delivered: false,
                is_running: active,
                is_active: active,
                group_kpi: BoardKpiSummary::default(),
            };

        // 23:30 yesterday vs 00:30 today (1h apart) MUST land in distinct local days.
        let g_yesterday =
            make_group("yest", utc_ms(2026, 11, 25, 23, 30), false);
        let g_today = make_group("today", utc_ms(2026, 11, 26, 0, 30), true);
        // Minor future clock skew clamps to Today so chart and list never disagree.
        let g_skew = make_group("skew", utc_ms(2026, 11, 26, 3, 0), false);
        // Exactly 6 local days ago (11/20) stays in 7d window; 7 days ago (11/19) folds out.
        let g_day6 = make_group("day6", utc_ms(2026, 11, 20, 9, 0), false);
        let g_day7 = make_group("day7", utc_ms(2026, 11, 19, 23, 59), false);

        let refs = vec![&g_yesterday, &g_today, &g_skew, &g_day6, &g_day7];
        let buckets = compute_daily_7d_in_tz(&refs, now_ms, tz);
        assert_eq!(buckets.len(), 7);
        assert_eq!(buckets[6].date_key, (2026, 11, 26));
        assert_eq!((buckets[6].month, buckets[6].day), (11, 26));
        assert_eq!(buckets[6].active_or_blocked, 1);
        assert_eq!(buckets[6].completed_or_delivered, 1);

        assert_eq!(buckets[5].date_key, (2026, 11, 25));
        assert_eq!(buckets[5].completed_or_delivered, 1);

        assert_eq!(buckets[0].date_key, (2026, 11, 20));
        assert_eq!(buckets[0].completed_or_delivered, 1);

        // Invariant: for every bucket, matches_day_filter count == bucket.total().
        for bucket in &buckets {
            let matched = refs
                .iter()
                .filter(|g| {
                    matches_day_filter(
                        g.updated_at,
                        now_ms,
                        tz,
                        bucket.date_key,
                    )
                })
                .count();
            assert_eq!(
                matched,
                bucket.total(),
                "day {:?} filter count must equal chart bucket count",
                bucket.date_key
            );
        }

        assert!(!is_older_than_7d_in_tz(g_day6.updated_at, now_ms, tz));
        assert!(is_older_than_7d_in_tz(g_day7.updated_at, now_ms, tz));
        assert!(!is_older_than_7d_in_tz(g_skew.updated_at, now_ms, tz));

        // Cross-year boundary: 2026-01-02 looks back cleanly into 2025-12-27.
        let jan2_ms = utc_ms(2026, 1, 2, 10, 0);
        let dec27 = make_group("dec27", utc_ms(2025, 12, 27, 15, 0), false);
        let year_buckets = compute_daily_7d_in_tz(&[&dec27], jan2_ms, tz);
        assert_eq!(year_buckets[0].date_key, (2025, 12, 27));
        assert_eq!(year_buckets[0].completed_or_delivered, 1);
    }

    #[test]
    fn pr2b_board_folds_router_child_works_and_strict_receipts_without_unassigned_leak()
     {
        let parent = SessionSummary {
            id: "r-parent".into(),
            title: "Qinghe Router".into(),
            updated_at: 1_760_000_000_000,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let w1_id = "w-delivered-1".to_owned();
        let w2_id = "w-pending-2".to_owned();

        let child_works = vec![
            flowy_agent::ChildWorkSummary {
                work_id: w1_id.clone(),
                root_session_id: parent.id.clone(),
                title: "Build slides".into(),
                state: "delivered".into(),
                access_mode: flowy_agent::WorkAccessMode::ReadWrite,
                origin: flowy_agent::SessionOrigin::Home,
                workspace_root: None,
                turn_status: TurnStatus::Completed,
                updated_at: 1_760_000_001_000,
                contract_revision: Some(1),
                delivered_artifact_ids: vec!["art-1".into()],
                missing_items: Vec::new(),
                objects: Vec::new(),
                last_stop_handoff: None,
                heartbeat: None,
                pending_cards: Vec::new(),
            },
            flowy_agent::ChildWorkSummary {
                work_id: w2_id.clone(),
                root_session_id: parent.id.clone(),
                title: "Update quote".into(),
                state: "awaiting_approval".into(),
                access_mode: flowy_agent::WorkAccessMode::ReadWrite,
                origin: flowy_agent::SessionOrigin::Home,
                workspace_root: None,
                turn_status: TurnStatus::WaitingForUser,
                updated_at: 1_760_000_002_000,
                contract_revision: Some(1),
                delivered_artifact_ids: Vec::new(),
                missing_items: Vec::new(),
                objects: Vec::new(),
                last_stop_handoff: None,
                heartbeat: None,
                pending_cards: vec![flowy_agent::PendingCard::ToolApproval {
                    card_id: "card-w2".into(),
                    tool_name: "powershell".into(),
                    command_preview: "Remove-Item ./tmp".into(),
                    args_hash: "deadbeef".into(),
                    risk_reason: "deletes files".into(),
                    target_summary: "workspace".into(),
                    created_at_ms: 1_760_000_002_000,
                }],
            },
        ];

        // Child W1 delivered a verified pptx; child W2 has a queued strict receipt + approval card.
        let mut child_lines: HashMap<&str, Vec<ChatLine>> = HashMap::new();
        child_lines.insert(
            w1_id.as_str(),
            vec![ChatLine::Verification {
                report_id: "rep-w1".into(),
                artifact_path: r"D:\Work\Qinghe.pptx".into(),
                artifact_hash: "aabbccdd11223344".into(),
                ok: true,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: Some(w1_id.clone()),
            }],
        );
        child_lines.insert(w2_id.as_str(), Vec::new());

        let root = temp_board_root("pr2b-router-fold");
        let queue = flowy_agent::AcceptanceQueue::new(root.clone());
        let w2_strict_receipt = queue
            .accept_strict(
                &w2_id,
                "worker 2 strict work",
                Some("req-w2"),
                1_760_000_002_000,
            )
            .unwrap();
        let legacy_unassigned_receipt = queue
            .accept(None, "legacy unassigned task", 1_760_000_003_000)
            .unwrap();

        let mut receipts_by_session: HashMap<
            Option<String>,
            Vec<(AcceptedMessage, ReceiptState)>,
        > = HashMap::new();
        receipts_by_session.insert(
            Some(w2_id.clone()),
            vec![(w2_strict_receipt, ReceiptState::Queued)],
        );
        receipts_by_session.insert(
            None,
            vec![(legacy_unassigned_receipt, ReceiptState::Queued)],
        );

        let (child_branches, _) =
            crate::ports::router_projection::project_router_children(
                &parent,
                &child_works,
                &std::collections::BTreeSet::new(),
                &|k| k.to_owned(),
                |wid| child_lines.get(wid).cloned().unwrap_or_default(),
                |wid| {
                    receipts_by_session
                        .remove(&Some(wid.to_owned()))
                        .unwrap_or_default()
                },
                |_| None,
            );
        let _ = std::fs::remove_dir_all(&root);

        // Child W2's strict receipt was drained into parent Router and never left in unassigned!
        assert!(
            !receipts_by_session.contains_key(&Some(w2_id.clone())),
            "child strict_target receipt must be consumed by parent Router group"
        );
        assert_eq!(
            receipts_by_session.get(&None).map(Vec::len),
            Some(1),
            "truly unassigned legacy receipt must remain in unassigned bucket"
        );

        // Verify folded branches carry W1's delivered PPTX, W2's queued receipt,
        // W2's approval blocker, and preserve each child's `owner_session_id`.
        assert!(child_branches.iter().any(|b| {
            b.id.starts_with(&format!("work:{w1_id}:"))
                && b.owner_session_id.as_deref() == Some(w1_id.as_str())
                && matches!(b.phase, BranchPhase::Delivered { .. })
                && b.format == DeliverableFormat::Slides
        }));
        assert!(child_branches.iter().any(|b| {
            b.id.starts_with(&format!("work:{w2_id}:"))
                && b.owner_session_id.as_deref() == Some(w2_id.as_str())
                && matches!(b.phase, BranchPhase::Queued { .. })
        }));
        assert!(child_branches.iter().any(|b| {
            b.id == format!("work:{w2_id}:approval")
                && b.owner_session_id.as_deref() == Some(w2_id.as_str())
                && matches!(b.phase, BranchPhase::NeedsApproval { .. })
        }));

        // KPI counts each folded branch strictly once.
        let mut kpi = BoardKpiSummary::default();
        for b in &child_branches {
            accumulate_kpi(&mut kpi, b);
        }
        assert_eq!(kpi.delivered_count, 1);
        assert_eq!(kpi.slides_count, 1);
        assert_eq!(kpi.running_count, 1);
        assert_eq!(kpi.approval_count, 1);
        assert_eq!(kpi.total_branches, 3);

        // PR #497 coupling: an 8-day-old Router whose child Worker updated today
        // must land inside the 7-day local calendar window ("today" bucket),
        // never folded behind "older than 7 days".
        let tz = 8 * 3600;
        let now_ms = 1_760_000_000_000_u64;
        let old_parent_ms = now_ms - 8 * DAY_MS;
        let mut today_child = child_works[0].clone();
        today_child.updated_at = now_ms - 60_000;
        let folded_ts = fold_group_updated_at(old_parent_ms, &[today_child]);
        assert_eq!(folded_ts, now_ms - 60_000);
        assert!(!is_older_than_7d_in_tz(folded_ts, now_ms, tz));
        assert!(is_older_than_7d_in_tz(old_parent_ms, now_ms, tz));
        let group = SessionBoardGroup {
            session_id: parent.id,
            title: parent.title,
            project_id: None,
            project_name: None,
            workspace_display: String::new(),
            updated_at: folded_ts,
            pinned: false,
            archived: false,
            preview: None,
            assistant_summary: None,
            branches: child_branches,
            workspace_changes: Vec::new(),
            child_workers: Vec::new(),
            total_branch_count: 3,
            total_file_count: 1,
            session_contract: None,
            has_blocker: true,
            has_delivered: true,
            is_running: true,
            is_active: true,
            group_kpi: kpi,
        };
        let buckets = compute_daily_7d_in_tz(&[&group], now_ms, tz);
        let today = buckets.last().unwrap();
        assert_eq!(today.active_or_blocked, 1);
        assert!(matches_day_filter(folded_ts, now_ms, tz, today.date_key));
        assert!(!matches_day_filter(
            old_parent_ms,
            now_ms,
            tz,
            today.date_key
        ));
    }

    #[test]
    fn deliverable_format_splits_pdf_and_markdown_from_doc() {
        assert_eq!(
            DeliverableFormat::from_path(r"D:\Docs\notes_q4.md"),
            DeliverableFormat::Markdown
        );
        assert_eq!(DeliverableFormat::Markdown.tag(), "MD");
        assert_eq!(
            DeliverableFormat::from_path("/tmp/readme.markdown"),
            DeliverableFormat::Markdown
        );
        assert_eq!(
            DeliverableFormat::from_path("/tmp/log.txt"),
            DeliverableFormat::Markdown
        );

        assert_eq!(
            DeliverableFormat::from_path(r"D:\Docs\brief.pdf"),
            DeliverableFormat::Pdf
        );
        assert_eq!(DeliverableFormat::Pdf.tag(), "PDF");

        assert_eq!(
            DeliverableFormat::from_path(r"D:\Docs\report.docx"),
            DeliverableFormat::Doc
        );
        assert_eq!(DeliverableFormat::Doc.tag(), "DOCX");

        assert_eq!(
            infer_skill_label(DeliverableFormat::Pdf, None),
            SkillLabel::Key("deliverables.kind_document")
        );
        assert_eq!(
            infer_skill_label(DeliverableFormat::Markdown, None),
            SkillLabel::Key("deliverables.kind_document")
        );
    }

    #[test]
    fn busy_turn_with_running_receipt_emits_only_one_running_branch_and_absorbs_contract()
     {
        let root = temp_board_root("p04-running-dedup");
        let queue = flowy_agent::AcceptanceQueue::new(root.clone());
        let running_receipt = queue
            .accept(Some("s-busy"), "generate quarterly sheet", 100)
            .unwrap();
        let queued_receipt = queue
            .accept(Some("s-busy"), "follow-up check", 200)
            .unwrap();
        let dead_receipt =
            queue.accept(Some("s-busy"), "stale failure", 300).unwrap();

        let summary = SessionSummary {
            id: "s-busy".into(),
            title: "Busy Task".into(),
            updated_at: 400,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let running_status = WorkStatus {
            work_id: running_receipt.id.clone(),
            session_id: Some("s-busy".into()),
            submission_id: Some("sub-running".into()),
            contract_revision: Some(1),
            receipt_state: Some(ReceiptState::Running),
            outcome: None,
            required_items: vec![RequiredDeliverable {
                filename: Some("sales_q4.xlsx".into()),
                extension: "xlsx".into(),
            }],
            delivered_artifact_ids: Vec::new(),
            missing_items: vec!["sales_q4.xlsx".into()],
        };

        let branches = project_session_branches(
            &summary,
            &[],
            true,
            false,
            None,
            &[
                (running_receipt.clone(), ReceiptState::Running),
                (queued_receipt.clone(), ReceiptState::Queued),
                (dead_receipt.clone(), ReceiptState::Dead),
            ],
            &std::collections::BTreeSet::new(),
            None,
            &|k| k.to_owned(),
            |wid| (wid == running_receipt.id).then(|| running_status.clone()),
        );
        let _ = std::fs::remove_dir_all(&root);

        let live_running: Vec<&WorkBranchSummary> = branches
            .iter()
            .filter(|b| {
                matches!(b.phase, BranchPhase::Running { .. })
                    || matches!(
                        b.phase,
                        BranchPhase::Queued {
                            state: ReceiptState::Running,
                            ..
                        }
                    )
            })
            .collect();
        assert_eq!(
            live_running.len(),
            1,
            "busy turn + ReceiptState::Running receipt must emit ONLY 1 running branch"
        );
        assert_eq!(live_running[0].id, "s-busy-live");
        assert_eq!(
            live_running[0]
                .work_progress
                .as_ref()
                .map(|wp| wp.work_id.as_str()),
            Some(running_receipt.id.as_str()),
            "live branch must absorb work_progress from the Running receipt"
        );

        assert!(branches.iter().any(|b| matches!(
            &b.phase,
            BranchPhase::Queued { receipt_id, state: ReceiptState::Queued }
                if receipt_id == &queued_receipt.id
        )));
        assert!(branches.iter().any(|b| matches!(
            &b.phase,
            BranchPhase::Queued { receipt_id, state: ReceiptState::Dead }
                if receipt_id == &dead_receipt.id
        )));
    }

    #[test]
    fn heuristic_separates_workspace_changes_from_formal_deliverables_and_preserves_over_six()
     {
        let summary = SessionSummary {
            id: "s-split".into(),
            title: "Q4 Financials".into(),
            updated_at: 1_760_000_000_000,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let mut lines = vec![
            ChatLine::FileChange {
                path: r"D:\Work\_build_xlsx.py".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 80,
                removed: 0,
                binary: false,
            },
            ChatLine::FileChanges {
                files: vec![
                    flowy_agent::FileEntry {
                        path: r"D:\Work\sales_q4.xlsx".into(),
                        change: flowy_agent::FileChangeKind::Add,
                        added: 1,
                        removed: 0,
                        binary: true,
                    },
                    flowy_agent::FileEntry {
                        path: r"D:\Work\notes_q4.md".into(),
                        change: flowy_agent::FileChangeKind::Add,
                        added: 20,
                        removed: 0,
                        binary: false,
                    },
                    flowy_agent::FileEntry {
                        path: r"D:\Work\brief_q4.pdf".into(),
                        change: flowy_agent::FileChangeKind::Add,
                        added: 1,
                        removed: 0,
                        binary: true,
                    },
                    flowy_agent::FileEntry {
                        path: r"D:\Work\dashboard.html".into(),
                        change: flowy_agent::FileChangeKind::Add,
                        added: 50,
                        removed: 0,
                        binary: false,
                    },
                ],
            },
        ];
        // Add 4 more formal markdown notes so total formal deliverables > 6.
        for i in 0..4 {
            lines.push(ChatLine::FileChange {
                path: format!(r"D:\Work\appendix_{i}.md"),
                change: flowy_agent::FileChangeKind::Add,
                added: 5,
                removed: 0,
                binary: false,
            });
        }
        // Explicitly verified script stays formal even though it's CodeWeb.
        lines.push(ChatLine::Verification {
            report_id: "rep-py".into(),
            artifact_path: r"D:\Work\verified_chart.py".into(),
            artifact_hash: "deadbeef12345678".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        });

        let contract = WorkContractProgress {
            work_id: "sub-q4".into(),
            contract_revision: Some(1),
            required_labels: vec![
                "sales_q4.xlsx".into(),
                "DASHBOARD.HTML".into(),
            ],
            delivered_count: 2,
            missing_items: Vec::new(),
            outcome: Some(SubmissionOutcome::Completed),
        };

        let all_branches = project_session_branches(
            &summary,
            &lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            Some(contract.clone()),
            &|k| k.to_owned(),
            |_| None,
        );
        let group = finalize_session_group(
            &summary,
            &lines,
            all_branches,
            &[],
            Vec::new(),
            false,
            Some(contract),
            None,
            None,
            "Default",
        );

        // `_build_xlsx.py` (unverified, uncontracted CodeWeb) goes to workspace_changes.
        assert_eq!(group.workspace_changes.len(), 1);
        assert_eq!(
            group.workspace_changes[0].deliverable_name,
            "_build_xlsx.py"
        );
        assert!(group.workspace_changes[0].is_workspace_change);

        // All 9 formal deliverables (`sales_q4.xlsx`, 5 `.md`, `brief_q4.pdf`,
        // contracted `dashboard.html`, verified `verified_chart.py`) remain
        // in `group.branches` without capping at 6.
        assert_eq!(group.branches.len(), 9);
        assert_eq!(group.total_branch_count, 9);
        assert!(
            group
                .branches
                .iter()
                .any(|b| b.deliverable_name == "sales_q4.xlsx")
        );
        assert!(
            group
                .branches
                .iter()
                .any(|b| b.deliverable_name == "notes_q4.md")
        );
        assert!(
            group
                .branches
                .iter()
                .any(|b| b.deliverable_name == "dashboard.html")
        );
        assert!(
            group
                .branches
                .iter()
                .any(|b| b.deliverable_name == "verified_chart.py")
        );
        assert_eq!(group.group_kpi.sheet_count, 1);
        assert_eq!(group.group_kpi.pdf_count, 1);
        assert_eq!(group.group_kpi.markdown_count, 5);
        assert_eq!(group.group_kpi.code_count, 2);
        assert_eq!(group.group_kpi.formal_deliverable_count, 9);
        assert_eq!(group.group_kpi.workspace_change_count, 1);
        assert_eq!(group.group_kpi.delivered_count, 9);

        // A session with ONLY workspace changes falls back to Idle in `branches`
        // so Flowchart Modal stays openable without reporting false delivery.
        let scratch_only_lines = vec![ChatLine::FileChange {
            path: r"D:\Work\_build_xlsx.py".into(),
            change: flowy_agent::FileChangeKind::Add,
            added: 20,
            removed: 0,
            binary: false,
        }];
        let scratch_branches = project_session_branches(
            &summary,
            &scratch_only_lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &|k| k.to_owned(),
            |_| None,
        );
        let scratch_group = finalize_session_group(
            &summary,
            &scratch_only_lines,
            scratch_branches,
            &[],
            Vec::new(),
            false,
            None,
            None,
            None,
            "Default",
        );
        assert_eq!(scratch_group.workspace_changes.len(), 1);
        assert_eq!(scratch_group.branches.len(), 1);
        assert_eq!(scratch_group.branches[0].phase, BranchPhase::Idle);
        assert!(!scratch_group.has_delivered);
        assert!(!scratch_group.is_delivered_settled());
        assert_eq!(scratch_group.total_file_count, 0);
        assert_eq!(scratch_group.group_kpi.workspace_change_count, 1);
        assert_eq!(scratch_group.group_kpi.formal_deliverable_count, 0);
    }

    #[test]
    fn is_delivered_settled_and_has_formal_format_enforce_state_invariants() {
        let formal_pdf = WorkBranchSummary {
            id: "b-pdf".into(),
            owner_session_id: None,
            skill_label: SkillLabel::Key("deliverables.kind_document"),
            phase: BranchPhase::Delivered {
                override_used: false,
                hash_short: "abcd1234".into(),
                provenance_warnings: Vec::new(),
            },
            deliverable_name: "brief.pdf".into(),
            deliverable_path: Some(r"D:\Docs\brief.pdf".into()),
            format: DeliverableFormat::Pdf,
            is_workspace_change: false,
            progress_pct: 100.0,
            detail: String::new(),
            work_progress: None,
        };
        let scratch_py = WorkBranchSummary {
            id: "b-py".into(),
            owner_session_id: None,
            skill_label: SkillLabel::Key("deliverables.kind_code"),
            phase: BranchPhase::Delivered {
                override_used: false,
                hash_short: String::new(),
                provenance_warnings: Vec::new(),
            },
            deliverable_name: "_helper.py".into(),
            deliverable_path: Some(r"D:\Docs\_helper.py".into()),
            format: DeliverableFormat::CodeWeb,
            is_workspace_change: true,
            progress_pct: 100.0,
            detail: String::new(),
            work_progress: None,
        };
        let mut group = SessionBoardGroup {
            session_id: "s-settled".into(),
            title: "Brief".into(),
            project_id: None,
            project_name: None,
            workspace_display: String::new(),
            updated_at: 1_760_000_000_000,
            pinned: false,
            archived: false,
            preview: None,
            assistant_summary: None,
            branches: vec![formal_pdf],
            workspace_changes: vec![scratch_py],
            child_workers: Vec::new(),
            total_branch_count: 1,
            total_file_count: 1,
            session_contract: None,
            has_blocker: false,
            has_delivered: true,
            is_running: false,
            is_active: false,
            group_kpi: BoardKpiSummary::default(),
        };

        assert!(group.is_delivered_settled());
        assert!(group.has_formal_format(DeliverableFormat::Pdf));
        assert!(!group.has_formal_format(DeliverableFormat::CodeWeb));

        group.has_blocker = true;
        assert!(!group.is_delivered_settled());

        group.has_blocker = false;
        group.is_running = true;
        assert!(!group.is_delivered_settled());

        group.is_running = false;
        group.archived = true;
        assert!(!group.is_delivered_settled());
    }

    #[test]
    fn router_group_populates_child_workers_assistant_summary_and_session_kpis()
    {
        let now_ms = 1_760_000_000_000_u64;
        let tz = 8 * 3600;
        let parent = SessionSummary {
            id: "r-main".into(),
            title: "Q4 Dispatch".into(),
            updated_at: now_ms - 10_000,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let long_secret_reply = format!(
            "**Summary** api_key=sk-secret-do-not-leak {}",
            "a".repeat(520)
        );
        let parent_lines = vec![
            ChatLine::User {
                text: "Prepare Q4 pack".into(),
            },
            ChatLine::Assistant {
                text: "old reply".into(),
            },
            ChatLine::Assistant {
                text: long_secret_reply,
            },
        ];

        let w1_id = "w-slides".to_owned();
        let w2_id = "w-approval".to_owned();
        let child_works = vec![
            flowy_agent::ChildWorkSummary {
                work_id: w1_id.clone(),
                root_session_id: parent.id.clone(),
                title: "Slides Worker".into(),
                state: "delivered".into(),
                access_mode: flowy_agent::WorkAccessMode::ReadWrite,
                origin: flowy_agent::SessionOrigin::Home,
                workspace_root: None,
                turn_status: TurnStatus::Completed,
                updated_at: now_ms - 5_000,
                contract_revision: Some(1),
                delivered_artifact_ids: vec!["art-1".into()],
                missing_items: Vec::new(),
                objects: Vec::new(),
                last_stop_handoff: None,
                heartbeat: None,
                pending_cards: Vec::new(),
            },
            flowy_agent::ChildWorkSummary {
                work_id: w2_id.clone(),
                root_session_id: parent.id.clone(),
                title: "Audit Worker".into(),
                state: "awaiting_approval".into(),
                access_mode: flowy_agent::WorkAccessMode::ReadWrite,
                origin: flowy_agent::SessionOrigin::Home,
                workspace_root: None,
                turn_status: TurnStatus::WaitingForUser,
                updated_at: now_ms - 2_000,
                contract_revision: Some(1),
                delivered_artifact_ids: Vec::new(),
                missing_items: Vec::new(),
                objects: Vec::new(),
                last_stop_handoff: None,
                heartbeat: None,
                pending_cards: vec![flowy_agent::PendingCard::ToolApproval {
                    card_id: "card-w2".into(),
                    tool_name: "shell".into(),
                    command_preview: "rm -rf tmp".into(),
                    args_hash: "deadbeef".into(),
                    risk_reason: "deletes temporary build directory".into(),
                    target_summary: "workspace".into(),
                    created_at_ms: now_ms - 2_000,
                }],
            },
        ];

        let mut child_lines: HashMap<&str, Vec<ChatLine>> = HashMap::new();
        child_lines.insert(
            w1_id.as_str(),
            vec![
                ChatLine::Verification {
                    report_id: "rep-w1".into(),
                    artifact_path: r"D:\Work\Q4.pptx".into(),
                    artifact_hash: "aabbccdd11223344".into(),
                    ok: true,
                    issues: Vec::new(),
                    semantic_hint: None,
                    override_used: false,
                    provenance_warnings: Vec::new(),
                    source_session_id: Some(w1_id.clone()),
                },
                ChatLine::FileChange {
                    path: r"D:\Work\_scratch.py".into(),
                    change: flowy_agent::FileChangeKind::Add,
                    added: 12,
                    removed: 0,
                    binary: false,
                },
            ],
        );
        child_lines.insert(w2_id.as_str(), Vec::new());

        let (child_branches, child_workers) =
            crate::ports::router_projection::project_router_children(
                &parent,
                &child_works,
                &std::collections::BTreeSet::new(),
                &|k| k.to_owned(),
                |wid| child_lines.get(wid).cloned().unwrap_or_default(),
                |_| Vec::new(),
                |_| None,
            );

        let router_group = finalize_session_group(
            &parent,
            &parent_lines,
            child_branches,
            &child_works,
            child_workers,
            false,
            None,
            None,
            None,
            "Default",
        );

        let summary = router_group
            .assistant_summary
            .as_deref()
            .expect("assistant summary present");
        assert!(summary.starts_with("Summary api_key="));
        assert!(!summary.contains("sk-secret-do-not-leak"));
        assert!(summary.chars().count() <= 480);

        assert_eq!(router_group.child_workers.len(), 2);
        assert_eq!(router_group.child_workers[0].session_id, w1_id);
        assert_eq!(router_group.child_workers[0].title, "Slides Worker");
        assert!(!router_group.child_workers[0].is_running);
        assert!(!router_group.child_workers[0].has_blocker);
        assert_eq!(router_group.child_workers[0].deliverable_count, 1);
        assert_eq!(
            router_group.child_workers[0].status_hint,
            "verify.card.passed"
        );

        assert_eq!(router_group.child_workers[1].session_id, w2_id);
        assert_eq!(router_group.child_workers[1].title, "Audit Worker");
        assert!(!router_group.child_workers[1].is_running);
        assert!(router_group.child_workers[1].has_blocker);
        assert_eq!(router_group.child_workers[1].deliverable_count, 0);
        assert!(
            router_group.child_workers[1]
                .status_hint
                .contains("deletes temporary build directory")
        );

        assert_eq!(router_group.workspace_changes.len(), 1);
        assert_eq!(
            router_group.workspace_changes[0].deliverable_name,
            "_scratch.py"
        );
        assert!(router_group.has_formal_format(DeliverableFormat::Slides));

        // Add one settled delivered group today to verify session-level KPI counts.
        let settled_summary = SessionSummary {
            id: "s-done".into(),
            title: "Done Today".into(),
            updated_at: now_ms - 30_000,
            project_id: None,
            pinned: false,
            archived: false,
        };
        let settled_lines = vec![ChatLine::FileChange {
            path: r"D:\Work\memo.md".into(),
            change: flowy_agent::FileChangeKind::Add,
            added: 5,
            removed: 0,
            binary: false,
        }];
        let settled_branches = project_session_branches(
            &settled_summary,
            &settled_lines,
            false,
            false,
            None,
            &[],
            &std::collections::BTreeSet::new(),
            None,
            &|k| k.to_owned(),
            |_| None,
        );
        let settled_group = finalize_session_group(
            &settled_summary,
            &settled_lines,
            settled_branches,
            &[],
            Vec::new(),
            false,
            None,
            None,
            None,
            "Default",
        );
        let kpi = BoardKpiSummary::from_groups_in_tz(
            &[&router_group, &settled_group],
            now_ms,
            tz,
        );
        assert_eq!(kpi.total_sessions, 2);
        assert_eq!(kpi.blocker_sessions, 1);
        assert_eq!(kpi.running_sessions, 0);
        assert_eq!(kpi.delivered_sessions, 1);
        assert_eq!(kpi.today_completed_sessions, 1);
        assert_eq!(kpi.slides_count, 1);
        assert_eq!(kpi.markdown_count, 1);
        assert_eq!(kpi.formal_deliverable_count, 2);
        assert_eq!(kpi.workspace_change_count, 1);
    }
}

//! UI-side extensible adapter ports, dirty-cached board snapshots, local
//! environment checks, and redacted trace projections.
//!
//! Boundary invariant: this module only reads through `flowy_agent::AgentHandle`
//! public APIs and `crate::projects::Project`. It never modifies kernel crates
//! or parses kernel disk files (`sessions/*.json`, `delivery/*.jsonl`,
//! `acceptance/journal.jsonl`) directly.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use flowy_agent::{
    AcceptedMessage, AgentHandle, ChatLine, ReceiptState, SessionSummary,
};
use serde::Serialize;

use crate::projects::{self, Project};

/// Maximum UTF-8 byte budget retained per trace summary line.
const MAX_TRACE_SUMMARY_BYTES: usize = 2048;

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

/// High-level deliverable format bucket for KPI visualization and file pills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeliverableFormat {
    Sheet,
    Slides,
    Doc,
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
            Some("docx" | "doc" | "pdf" | "md" | "txt" | "rtf" | "odt") => {
                Self::Doc
            }
            _ => Self::Other,
        }
    }

    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Sheet => "XLSX",
            Self::Slides => "PPTX",
            Self::Doc => "DOCX",
            Self::Other => "FILE",
        }
    }
}

/// User-facing skill option pushed from `flowy-app` (`flowy-skills`) into Studio.
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
    /// Verification failed and awaits explicit two-step user confirmation, or a
    /// future kernel tool-level approval gate.
    NeedsApproval {
        report_id: String,
        artifact_path: String,
        summary: String,
        /// Reserved seam for future kernel `ApprovalPort` tool-level gates.
        tool_approval_id: Option<String>,
    },
    /// Actively running a turn or tool call.
    Running { step_label: String, percent: f32 },
    /// Deferred acceptance receipt queued or retrying in `AcceptanceQueue`.
    Queued {
        receipt_id: String,
        state: ReceiptState,
    },
    /// Delivered and verified (or explicitly overridden with audit trail).
    Delivered {
        override_used: bool,
        hash_short: String,
    },
    /// Idle conversation without pending deliverables.
    Idle,
}

/// One visual flow branch inside a session swimlane / modal flowchart.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkBranchSummary {
    pub id: String,
    pub skill_label: String,
    pub phase: BranchPhase,
    pub deliverable_name: String,
    pub deliverable_path: Option<String>,
    pub format: DeliverableFormat,
    pub progress_pct: f32,
    pub detail: String,
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
    pub branches: Vec<WorkBranchSummary>,
    pub has_blocker: bool,
    pub is_running: bool,
    pub is_active: bool,
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
    pub other_count: usize,
    pub active_sessions: usize,
    pub total_sessions: usize,
}

/// Dirty-cached snapshot owned by `StudioPanel`.
///
/// Recomputed strictly when `AgentUpdate` or local project/approval changes
/// occur — never on idle 33ms pump ticks or inside `Render::render`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkBoardSnapshot {
    pub kpi: BoardKpiSummary,
    pub sessions: Vec<SessionBoardGroup>,
    pub generation: u64,
}

/// Abstraction for board data sources so future kernel snapshots can plug in
/// without altering `overview.rs`.
pub trait BoardDataSource {
    fn build_board_snapshot(
        &self,
        projects: &[Project],
        pending_receipts: &[AcceptedMessage],
        default_space_label: &str,
        generation: u64,
    ) -> WorkBoardSnapshot;
}

impl BoardDataSource for AgentHandle {
    fn build_board_snapshot(
        &self,
        projects: &[Project],
        pending_receipts: &[AcceptedMessage],
        default_space_label: &str,
        generation: u64,
    ) -> WorkBoardSnapshot {
        let sessions = self.list_sessions();
        let busy_ids: HashSet<String> =
            self.busy_session_ids().into_iter().collect();
        let overridden_hashes = self.overridden_artifact_hashes();
        let project_map: HashMap<&str, &Project> = projects
            .iter()
            .map(|project| (project.id.as_str(), project))
            .collect();

        // Group queued receipts by optional session_hint.
        let mut receipts_by_session: HashMap<
            Option<&str>,
            Vec<&AcceptedMessage>,
        > = HashMap::new();
        for receipt in pending_receipts {
            receipts_by_session
                .entry(receipt.session_hint.as_deref())
                .or_default()
                .push(receipt);
        }

        let mut groups = Vec::with_capacity(sessions.len());
        let mut kpi = BoardKpiSummary {
            total_sessions: sessions.len(),
            ..BoardKpiSummary::default()
        };

        for summary in &sessions {
            let lines = self.session_lines(&summary.id);
            let is_busy = busy_ids.contains(&summary.id);
            let session_receipts = receipts_by_session
                .remove(&Some(summary.id.as_str()))
                .unwrap_or_default();

            let project = summary
                .project_id
                .as_deref()
                .and_then(|id| project_map.get(id).copied());
            let workspace_display = project.map_or_else(
                || default_space_label.to_owned(),
                |proj| display_path(&proj.root),
            );

            let branches = project_session_branches(
                summary,
                &lines,
                is_busy,
                &session_receipts,
                &overridden_hashes,
                |receipt_id| self.receipt_state(receipt_id),
            );

            let has_blocker = branches.iter().any(|branch| {
                matches!(branch.phase, BranchPhase::NeedsApproval { .. })
            });
            let is_running = is_busy
                || branches.iter().any(|branch| {
                    matches!(
                        branch.phase,
                        BranchPhase::Running { .. }
                            | BranchPhase::Queued { .. }
                    )
                });
            let is_active = has_blocker || is_running;
            if is_active {
                kpi.active_sessions += 1;
            }

            for branch in &branches {
                accumulate_kpi(&mut kpi, branch);
            }

            let title = if summary.title.trim().is_empty() {
                self.session_preview(&summary.id)
                    .unwrap_or_else(|| short_id(&summary.id))
            } else {
                summary.title.clone()
            };

            groups.push(SessionBoardGroup {
                session_id: summary.id.clone(),
                title,
                project_id: summary.project_id.clone(),
                project_name: project.map(|proj| proj.name.clone()),
                workspace_display,
                updated_at: summary.updated_at,
                branches,
                has_blocker,
                is_running,
                is_active,
            });
        }

        // Surface unassigned queued receipts (session_hint == None) as one group
        // so nothing in AcceptanceQueue goes invisible on the board.
        if let Some(unassigned) = receipts_by_session.remove(&None)
            && !unassigned.is_empty()
        {
            let branches: Vec<WorkBranchSummary> = unassigned
                .iter()
                .map(|receipt| {
                    let state = self
                        .receipt_state(&receipt.id)
                        .unwrap_or(ReceiptState::Queued);
                    WorkBranchSummary {
                        id: format!("receipt-{}", receipt.id),
                        skill_label: "Queue".to_owned(),
                        phase: BranchPhase::Queued {
                            receipt_id: receipt.id.clone(),
                            state,
                        },
                        deliverable_name: clip_label(&receipt.text, 28),
                        deliverable_path: None,
                        format: DeliverableFormat::Other,
                        progress_pct: 15.0,
                        detail: clip_label(&receipt.text, 96),
                    }
                })
                .collect();
            for branch in &branches {
                accumulate_kpi(&mut kpi, branch);
            }
            kpi.active_sessions += 1;
            kpi.total_sessions += 1;
            groups.insert(
                0,
                SessionBoardGroup {
                    session_id: "unassigned-queue".to_owned(),
                    title: default_space_label.to_owned(),
                    project_id: None,
                    project_name: None,
                    workspace_display: default_space_label.to_owned(),
                    updated_at: unassigned
                        .first()
                        .map_or(0, |r| r.created_at_ms / 1000),
                    branches,
                    has_blocker: false,
                    is_running: true,
                    is_active: true,
                },
            );
        }

        // Priority sort: blockers first, then running, then newest updated_at.
        groups.sort_by(|a, b| {
            b.has_blocker
                .cmp(&a.has_blocker)
                .then_with(|| b.is_running.cmp(&a.is_running))
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });

        WorkBoardSnapshot {
            kpi,
            sessions: groups,
            generation,
        }
    }
}

const fn accumulate_kpi(kpi: &mut BoardKpiSummary, branch: &WorkBranchSummary) {
    kpi.total_branches += 1;
    match &branch.phase {
        BranchPhase::NeedsApproval { .. } => kpi.approval_count += 1,
        BranchPhase::Running { .. } | BranchPhase::Queued { .. } => {
            kpi.running_count += 1;
        }
        BranchPhase::Delivered { .. } => {
            kpi.delivered_count += 1;
            match branch.format {
                DeliverableFormat::Sheet => kpi.sheet_count += 1,
                DeliverableFormat::Slides => kpi.slides_count += 1,
                DeliverableFormat::Doc => kpi.doc_count += 1,
                DeliverableFormat::Other => kpi.other_count += 1,
            }
        }
        BranchPhase::Idle => {}
    }
}

fn project_session_branches<F>(
    summary: &SessionSummary,
    lines: &[ChatLine],
    is_busy: bool,
    queued_receipts: &[&AcceptedMessage],
    overridden_hashes: &std::collections::BTreeSet<String>,
    receipt_state_fn: F,
) -> Vec<WorkBranchSummary>
where
    F: Fn(&str) -> Option<ReceiptState>,
{
    let mut branches = Vec::new();
    let mut seen_paths = HashSet::new();

    // 1. Collect explicit verification & delivery artifacts (newest first, dedup by path).
    for (idx, line) in lines.iter().enumerate().rev() {
        if let ChatLine::Verification {
            report_id,
            artifact_path,
            artifact_hash,
            ok,
            issues,
            semantic_hint,
            override_used,
        } = line
        {
            if !seen_paths.insert(artifact_path.clone()) {
                continue;
            }
            let overridden =
                *override_used || overridden_hashes.contains(artifact_hash);
            let name = basename(artifact_path);
            let format = DeliverableFormat::from_path(artifact_path);
            let hash_short: String = artifact_hash.chars().take(8).collect();
            let (phase, progress_pct, detail) = if *ok || overridden {
                (
                    BranchPhase::Delivered {
                        override_used: overridden,
                        hash_short,
                    },
                    100.0,
                    semantic_hint
                        .clone()
                        .unwrap_or_else(|| display_path_str(artifact_path)),
                )
            } else {
                let issue_count = issues.len();
                let detail_msg = format!(
                    "{} ({issue_count})",
                    display_path_str(artifact_path)
                );
                (
                    BranchPhase::NeedsApproval {
                        report_id: report_id.clone(),
                        artifact_path: artifact_path.clone(),
                        summary: detail_msg.clone(),
                        tool_approval_id: None,
                    },
                    85.0,
                    detail_msg,
                )
            };
            branches.push(WorkBranchSummary {
                id: format!("{}-v-{idx}", summary.id),
                skill_label: infer_skill_label(format, None),
                phase,
                deliverable_name: name,
                deliverable_path: Some(artifact_path.clone()),
                format,
                progress_pct,
                detail,
            });
        }
    }

    // 2. Include file changes not already covered by Verification rows.
    for (idx, line) in lines.iter().enumerate().rev() {
        match line {
            ChatLine::FileChanges { files } => {
                for (f_ix, file) in files.iter().enumerate() {
                    if !seen_paths.insert(file.path.clone()) {
                        continue;
                    }
                    let format = DeliverableFormat::from_path(&file.path);
                    branches.push(WorkBranchSummary {
                        id: format!("{}-fc-{idx}-{f_ix}", summary.id),
                        skill_label: infer_skill_label(format, None),
                        phase: BranchPhase::Delivered {
                            override_used: false,
                            hash_short: String::new(),
                        },
                        deliverable_name: basename(&file.path),
                        deliverable_path: Some(file.path.clone()),
                        format,
                        progress_pct: 100.0,
                        detail: display_path_str(&file.path),
                    });
                }
            }
            ChatLine::FileChange { path, .. }
                if seen_paths.insert(path.clone()) =>
            {
                let format = DeliverableFormat::from_path(path);
                branches.push(WorkBranchSummary {
                    id: format!("{}-f-{idx}", summary.id),
                    skill_label: infer_skill_label(format, None),
                    phase: BranchPhase::Delivered {
                        override_used: false,
                        hash_short: String::new(),
                    },
                    deliverable_name: basename(path),
                    deliverable_path: Some(path.clone()),
                    format,
                    progress_pct: 100.0,
                    detail: display_path_str(path),
                });
            }
            _ => {}
        }
    }

    // 3. Running live turn (surface latest tool call / reasoning step).
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
        let (skill_label, step_label, detail) =
            if let Some((name, target)) = active_tool {
                (
                    infer_skill_label(DeliverableFormat::Other, Some(name)),
                    name.to_owned(),
                    display_path_str(target),
                )
            } else {
                (
                    "Router".to_owned(),
                    "Running".to_owned(),
                    summary.title.clone(),
                )
            };
        branches.insert(
            0,
            WorkBranchSummary {
                id: format!("{}-live", summary.id),
                skill_label,
                phase: BranchPhase::Running {
                    step_label: step_label.clone(),
                    percent: pct,
                },
                deliverable_name: step_label,
                deliverable_path: None,
                format: DeliverableFormat::Other,
                progress_pct: pct,
                detail,
            },
        );
    }

    // 4. Deferred receipts hinted to this session.
    for receipt in queued_receipts {
        let state =
            receipt_state_fn(&receipt.id).unwrap_or(ReceiptState::Queued);
        branches.push(WorkBranchSummary {
            id: format!("{}-q-{}", summary.id, receipt.id),
            skill_label: "Queue".to_owned(),
            phase: BranchPhase::Queued {
                receipt_id: receipt.id.clone(),
                state,
            },
            deliverable_name: clip_label(&receipt.text, 24),
            deliverable_path: None,
            format: DeliverableFormat::Other,
            progress_pct: 15.0,
            detail: clip_label(&receipt.text, 96),
        });
    }

    // 5. Fallback single idle branch if the session had pure Q&A without files.
    if branches.is_empty() {
        let preview = lines
            .iter()
            .rev()
            .find_map(|line| match line {
                ChatLine::Assistant { text } | ChatLine::User { text } => {
                    let trimmed = text.trim();
                    (!trimmed.is_empty()).then(|| clip_label(trimmed, 48))
                }
                _ => None,
            })
            .unwrap_or_else(|| summary.title.clone());
        branches.push(WorkBranchSummary {
            id: format!("{}-idle", summary.id),
            skill_label: "Router".to_owned(),
            phase: BranchPhase::Idle,
            deliverable_name: clip_label(&summary.title, 24),
            deliverable_path: None,
            format: DeliverableFormat::Other,
            progress_pct: 100.0,
            detail: preview,
        });
    }

    branches.truncate(6);
    branches
}

fn infer_skill_label(format: DeliverableFormat, tool: Option<&str>) -> String {
    if let Some(name) = tool {
        return match name {
            "create_deck" | "verify_deck" => "PPTX".to_owned(),
            "read_file" | "write_file" | "edit_file" => "File".to_owned(),
            "glob" | "grep" => "Search".to_owned(),
            "bash" | "shell" => "Shell".to_owned(),
            other => other.to_owned(),
        };
    }
    match format {
        DeliverableFormat::Sheet => "Sheet".to_owned(),
        DeliverableFormat::Slides => "Slides".to_owned(),
        DeliverableFormat::Doc => "Doc".to_owned(),
        DeliverableFormat::Other => "Work".to_owned(),
    }
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn short_id(id: &str) -> String {
    let count = id.chars().count();
    id.chars().skip(count.saturating_sub(8)).collect()
}

fn clip_label(text: &str, max_chars: usize) -> String {
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

// ============================================================================
// Structured Redacted Trace View (In-memory projection + JSONL export)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RedactedTraceEntry {
    pub seq: usize,
    pub level: TraceLevel,
    pub event: &'static str,
    pub summary: String,
}

/// Projects structured, redacted trace events for a single session purely from
/// `AgentHandle` in-memory projections (never reads kernel disk files).
#[must_use]
pub fn collect_session_traces(
    agent: &AgentHandle,
    session_id: &str,
    pending_receipts: &[AcceptedMessage],
) -> Vec<RedactedTraceEntry> {
    let lines = agent.session_lines(session_id);
    let mut entries = Vec::new();
    let mut push = |level: TraceLevel, event: &'static str, raw: String| {
        let seq = entries.len() + 1;
        entries.push(RedactedTraceEntry {
            seq,
            level,
            event,
            summary: redact_trace_text(&raw),
        });
    };

    for line in &lines {
        match line {
            ChatLine::User { text } => {
                push(TraceLevel::Info, "turn_start", clip_label(text, 120));
            }
            ChatLine::ToolCall {
                name,
                target,
                running,
                ..
            } => {
                let state = if *running { "running" } else { "done" };
                push(
                    TraceLevel::Info,
                    "tool_call",
                    format!(
                        "tool={name} state={state} target={}",
                        display_path_str(target)
                    ),
                );
            }
            ChatLine::ToolResult { text, is_error } if *is_error => {
                push(TraceLevel::Error, "tool_error", clip_label(text, 180));
            }
            ChatLine::Verification {
                report_id,
                artifact_path,
                artifact_hash,
                ok,
                issues,
                override_used,
                ..
            } => {
                let level = if *ok {
                    TraceLevel::Info
                } else {
                    TraceLevel::Warn
                };
                let short_hash: String =
                    artifact_hash.chars().take(12).collect();
                push(
                    level,
                    if *override_used {
                        "verify_override"
                    } else {
                        "verify_result"
                    },
                    format!(
                        "report={report_id} ok={ok} override={override_used} issues={} sha256={short_hash} file={}",
                        issues.len(),
                        display_path_str(artifact_path)
                    ),
                );
            }
            ChatLine::FileChanges { files } => {
                push(
                    TraceLevel::Info,
                    "delivery_files",
                    format!("count={}", files.len()),
                );
            }
            ChatLine::TaskReceipt { task_id, goal } => {
                push(
                    TraceLevel::Info,
                    "task_receipt",
                    format!("id={task_id} goal={}", clip_label(goal, 80)),
                );
            }
            _ => {}
        }
    }

    for receipt in pending_receipts
        .iter()
        .filter(|r| r.session_hint.as_deref() == Some(session_id))
    {
        push(
            TraceLevel::Warn,
            "receipt_queued",
            format!(
                "id={} attempts={} text={}",
                receipt.id,
                receipt.attempts,
                clip_label(&receipt.text, 80)
            ),
        );
    }

    // Keep ring-bounded tail (most recent 256 events).
    if entries.len() > 256 {
        let drop_count = entries.len() - 256;
        entries.drain(..drop_count);
    }
    entries
}

/// Formats redacted trace rows into JSONL for clipboard copy or bug reports.
#[must_use]
pub fn format_traces_jsonl(entries: &[RedactedTraceEntry]) -> String {
    entries
        .iter()
        .filter_map(|entry| serde_json::to_string(entry).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Masks secret tokens/keys and clamps payload length before displaying or
/// exporting trace text.
#[must_use]
pub fn redact_trace_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_TRACE_SUMMARY_BYTES));
    let mut words = raw.split_whitespace();
    while let Some(word) = words.next() {
        if !out.is_empty() {
            out.push(' ');
        }
        let lower = word.to_ascii_lowercase();
        if lower == "bearer" {
            out.push_str("Bearer [REDACTED]");
            let _ = words.next();
            continue;
        }
        if let Some((lhs, _)) = word.split_once('=') {
            let key = lhs.to_ascii_lowercase();
            if key.contains("key")
                || key.contains("token")
                || key.contains("secret")
                || key.contains("password")
            {
                out.push_str(lhs);
                out.push_str("=[REDACTED]");
                continue;
            }
        }
        if (word.starts_with("sk-") || word.starts_with("ghp_"))
            && word.len() >= 12
        {
            out.push_str("[REDACTED]");
            continue;
        }
        out.push_str(word);
        if out.len() >= MAX_TRACE_SUMMARY_BYTES {
            break;
        }
    }
    if out.len() > MAX_TRACE_SUMMARY_BYTES {
        let mut end = MAX_TRACE_SUMMARY_BYTES;
        while end > 0 && !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push('\u{2026}');
    }
    out
}

// ============================================================================
// Local Environment Health Snapshot (Read-only OS checks + L3 Semantic Toggle)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvSeverity {
    Ready,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEnvCheck {
    pub label_key: &'static str,
    pub severity: EnvSeverity,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEnvSnapshot {
    pub overall: EnvSeverity,
    pub semantic_verify_enabled: bool,
    pub checks: Vec<LocalEnvCheck>,
}

/// Evaluates lightweight, non-blocking local environment checks without reading
/// any kernel state files.
#[must_use]
pub fn probe_local_env(
    projects: &[Project],
    installed_skills: &[SkillChip],
    cloud_ready: bool,
    semantic_verify_enabled: bool,
) -> LocalEnvSnapshot {
    let unavailable_projects = projects
        .iter()
        .filter(|project| !projects::root_available(&project.root))
        .count();

    let mut checks = Vec::with_capacity(4);
    checks.push(LocalEnvCheck {
        label_key: "env.check_projects",
        severity: if unavailable_projects == 0 {
            EnvSeverity::Ready
        } else {
            EnvSeverity::Warning
        },
        detail: if unavailable_projects == 0 {
            format!("{}/{}", projects.len(), projects.len())
        } else {
            format!(
                "{}/{} ({unavailable_projects}!)",
                projects.len().saturating_sub(unavailable_projects),
                projects.len()
            )
        },
    });
    checks.push(LocalEnvCheck {
        label_key: "env.check_skills",
        severity: EnvSeverity::Ready,
        detail: installed_skills.len().to_string(),
    });
    checks.push(LocalEnvCheck {
        label_key: "env.check_model_route",
        severity: if cloud_ready {
            EnvSeverity::Ready
        } else {
            EnvSeverity::Warning
        },
        detail: if cloud_ready {
            "OK".to_owned()
        } else {
            "Setup".to_owned()
        },
    });
    checks.push(LocalEnvCheck {
        label_key: "env.check_l3_verify",
        severity: EnvSeverity::Ready,
        detail: if semantic_verify_enabled {
            "ON".to_owned()
        } else {
            "OFF".to_owned()
        },
    });

    let overall = if unavailable_projects > 0 || !cloud_ready {
        EnvSeverity::Warning
    } else {
        EnvSeverity::Ready
    };

    LocalEnvSnapshot {
        overall,
        semantic_verify_enabled,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowy_agent::VerificationIssue;

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
    fn redact_trace_text_masks_secrets_and_clamps_long_payloads() {
        let raw = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9 api_key=my-secret-value sk-live-1234567890abcdef ok=true";
        let clean = redact_trace_text(raw);
        assert!(!clean.contains("eyJhbGciOiJIUzI1NiJ9"), "{clean}");
        assert!(!clean.contains("my-secret-value"), "{clean}");
        assert!(!clean.contains("sk-live-1234567890abcdef"), "{clean}");
        assert!(clean.contains("Bearer [REDACTED]"), "{clean}");
        assert!(clean.contains("api_key=[REDACTED]"), "{clean}");
        assert!(clean.contains("ok=true"), "{clean}");

        let huge = "a ".repeat(3000);
        let clamped = redact_trace_text(&huge);
        assert!(clamped.len() <= MAX_TRACE_SUMMARY_BYTES + 4);
    }

    #[test]
    fn projection_classifies_failed_verification_as_blocker_with_report_id() {
        let summary = SessionSummary {
            id: "s1".into(),
            title: "Q3 Report".into(),
            updated_at: 42,
            project_id: None,
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
            },
            ChatLine::Verification {
                report_id: "rep-002".into(),
                artifact_path: r"D:\Work\Deck.pptx".into(),
                artifact_hash: "3c92a01b99887766".into(),
                ok: true,
                issues: vec![],
                semantic_hint: None,
                override_used: false,
            },
        ];
        let branches = project_session_branches(
            &summary,
            &lines,
            false,
            &[],
            &std::collections::BTreeSet::new(),
            |_| None,
        );
        assert_eq!(branches.len(), 2);
        assert!(branches.iter().any(|b| matches!(
            &b.phase,
            BranchPhase::NeedsApproval { report_id, .. } if report_id == "rep-001"
        )));
        assert!(branches.iter().any(|b| matches!(
            &b.phase,
            BranchPhase::Delivered { override_used: false, hash_short } if hash_short == "3c92a01b"
        )));
        // Confirm Windows extended prefix stripped in detail.
        let blocker = branches
            .iter()
            .find(|b| matches!(b.phase, BranchPhase::NeedsApproval { .. }))
            .unwrap();
        assert!(!blocker.detail.contains(r"\\?\"));
    }
}

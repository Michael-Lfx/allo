//! Dirty-cached Visual Board snapshots and work-contract branch projections.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use flowy_agent::{
    AcceptedMessage, AgentHandle, ChatLine, ReceiptState, RequiredDeliverable,
    SessionSummary, SubmissionOutcome, WorkStatus,
};

use super::{clip_label, display_path, display_path_str, short_id};
use crate::projects::Project;

/// Synthetic session group id used to surface unassigned queued/failed receipts.
pub const UNASSIGNED_QUEUE_SESSION_ID: &str = "unassigned-queue";

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
    /// Verification failed and awaits explicit two-step user confirmation.
    NeedsApproval {
        report_id: String,
        artifact_path: String,
        summary: String,
    },
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
    },
    /// Idle conversation without pending deliverables.
    Idle,
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
            || status.outcome.is_some()
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

/// One visual flow branch inside a session swimlane / modal flowchart.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkBranchSummary {
    pub id: String,
    pub skill_label: SkillLabel,
    pub phase: BranchPhase,
    pub deliverable_name: String,
    pub deliverable_path: Option<String>,
    pub format: DeliverableFormat,
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
    pub branches: Vec<WorkBranchSummary>,
    pub has_blocker: bool,
    pub is_running: bool,
    pub is_active: bool,
}

impl SessionBoardGroup {
    /// Synthetic bucket for receipts that are not yet attached to a real session.
    #[must_use]
    pub fn is_synthetic(&self) -> bool {
        self.session_id == UNASSIGNED_QUEUE_SESSION_ID
    }
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
/// without altering `overview`.
pub trait BoardDataSource {
    fn build_board_snapshot(
        &self,
        projects: &[Project],
        visible_receipts: &[(AcceptedMessage, ReceiptState)],
        default_space_label: &str,
        generation: u64,
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

const fn branch_is_blocker(phase: &BranchPhase) -> bool {
    matches!(
        phase,
        BranchPhase::NeedsApproval { .. }
            | BranchPhase::Queued {
                state: ReceiptState::Dead | ReceiptState::Quarantined,
                ..
            }
    )
}

const fn branch_is_running(phase: &BranchPhase) -> bool {
    matches!(
        phase,
        BranchPhase::Running { .. }
            | BranchPhase::Queued {
                state: ReceiptState::Queued
                    | ReceiptState::Running
                    | ReceiptState::Retrying,
                ..
            }
    )
}

impl BoardDataSource for AgentHandle {
    fn build_board_snapshot(
        &self,
        projects: &[Project],
        visible_receipts: &[(AcceptedMessage, ReceiptState)],
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
        let mut kpi = BoardKpiSummary {
            total_sessions: sessions.len(),
            ..BoardKpiSummary::default()
        };

        for summary in &sessions {
            let lines = self.session_lines(&summary.id);
            let is_busy = busy_ids.contains(&summary.id);
            let session_receipts = receipts_by_session
                .remove(&Some(summary.id.clone()))
                .unwrap_or_default();
            let session_contract = self
                .session_work_status(&summary.id)
                .and_then(|status| WorkContractProgress::from_status(&status));

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
                session_contract,
                |work_id| self.work_status(work_id),
            );

            let has_blocker = branches
                .iter()
                .any(|branch| branch_is_blocker(&branch.phase));
            let is_running = is_busy
                || branches
                    .iter()
                    .any(|branch| branch_is_running(&branch.phase));
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

        // Surface unassigned queued/failed receipts (no session owner) as one
        // group so nothing in AcceptanceQueue goes invisible on the board.
        if let Some(unassigned) = receipts_by_session.remove(&None)
            && !unassigned.is_empty()
        {
            let branches: Vec<WorkBranchSummary> = unassigned
                .iter()
                .map(|(receipt, state)| {
                    let work_progress = self
                        .work_status(&receipt.id)
                        .and_then(|s| WorkContractProgress::from_status(&s));
                    WorkBranchSummary {
                        id: format!("receipt-{}", receipt.id),
                        skill_label: SkillLabel::Key("board.skill_queue"),
                        phase: BranchPhase::Queued {
                            receipt_id: receipt.id.clone(),
                            state: *state,
                        },
                        deliverable_name: clip_label(&receipt.text, 28),
                        deliverable_path: None,
                        format: DeliverableFormat::Other,
                        progress_pct: receipt_progress_pct(*state),
                        detail: clip_label(&receipt.text, 96),
                        work_progress,
                    }
                })
                .collect();
            let has_blocker = branches
                .iter()
                .any(|branch| branch_is_blocker(&branch.phase));
            let is_running = branches
                .iter()
                .any(|branch| branch_is_running(&branch.phase));
            let is_active = has_blocker || is_running;
            for branch in &branches {
                accumulate_kpi(&mut kpi, branch);
            }
            if is_active {
                kpi.active_sessions += 1;
            }
            kpi.total_sessions += 1;
            groups.insert(
                0,
                SessionBoardGroup {
                    session_id: UNASSIGNED_QUEUE_SESSION_ID.to_owned(),
                    title: default_space_label.to_owned(),
                    project_id: None,
                    project_name: None,
                    workspace_display: default_space_label.to_owned(),
                    updated_at: unassigned
                        .first()
                        .map_or(0, |(r, _)| r.created_at_ms / 1000),
                    branches,
                    has_blocker,
                    is_running,
                    is_active,
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
        BranchPhase::NeedsApproval { .. }
        | BranchPhase::Queued {
            state: ReceiptState::Dead | ReceiptState::Quarantined,
            ..
        } => kpi.approval_count += 1,
        BranchPhase::Running { .. }
        | BranchPhase::Queued {
            state:
                ReceiptState::Queued
                | ReceiptState::Running
                | ReceiptState::Retrying,
            ..
        } => {
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
        BranchPhase::Queued {
            state: ReceiptState::Completed | ReceiptState::Cancelled,
            ..
        }
        | BranchPhase::Idle => {}
    }
}

fn project_session_branches<F>(
    summary: &SessionSummary,
    lines: &[ChatLine],
    is_busy: bool,
    queued_receipts: &[(AcceptedMessage, ReceiptState)],
    overridden_hashes: &std::collections::BTreeSet<String>,
    session_contract: Option<WorkContractProgress>,
    work_status_fn: F,
) -> Vec<WorkBranchSummary>
where
    F: Fn(&str) -> Option<WorkStatus>,
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
                work_progress: None,
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
                        work_progress: None,
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
                    work_progress: None,
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
                    SkillLabel::Key("chat.router_name"),
                    String::new(),
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
                deliverable_name: if step_label.is_empty() {
                    clip_label(&summary.title, 24)
                } else {
                    step_label
                },
                deliverable_path: None,
                format: DeliverableFormat::Other,
                progress_pct: pct,
                detail,
                work_progress: None,
            },
        );
    }

    // 4. Deferred / retrying / dead receipts associated with this session.
    for (receipt, state) in queued_receipts {
        let work_progress = work_status_fn(&receipt.id)
            .and_then(|s| WorkContractProgress::from_status(&s));
        branches.push(WorkBranchSummary {
            id: format!("{}-q-{}", summary.id, receipt.id),
            skill_label: SkillLabel::Key("board.skill_queue"),
            phase: BranchPhase::Queued {
                receipt_id: receipt.id.clone(),
                state: *state,
            },
            deliverable_name: clip_label(&receipt.text, 24),
            deliverable_path: None,
            format: DeliverableFormat::Other,
            progress_pct: receipt_progress_pct(*state),
            detail: clip_label(&receipt.text, 96),
            work_progress,
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
            skill_label: SkillLabel::Key("chat.router_name"),
            phase: BranchPhase::Idle,
            deliverable_name: clip_label(&summary.title, 24),
            deliverable_path: None,
            format: DeliverableFormat::Other,
            progress_pct: 100.0,
            detail: preview,
            work_progress: None,
        });
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

    branches.truncate(6);
    branches
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
        DeliverableFormat::Doc => SkillLabel::Key("deliverables.kind_document"),
        DeliverableFormat::Other => SkillLabel::Key("board.skill_work"),
    }
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
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
            &[],
            &std::collections::BTreeSet::new(),
            Some(contract.clone()),
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
        assert!(branch_is_blocker(&dead_phase));
        assert!(!branch_is_running(&dead_phase));

        let retrying_phase = BranchPhase::Queued {
            receipt_id: "rcpt-43".into(),
            state: ReceiptState::Retrying,
        };
        assert!(!branch_is_blocker(&retrying_phase));
        assert!(branch_is_running(&retrying_phase));
    }
}

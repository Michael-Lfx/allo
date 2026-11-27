//! Pure projection helpers folding a Router conversation's child Workers
//! (`ChildWorkSummary`) into `WorkBranchSummary` swimlanes on the Progress Board
//! without leaking child `strict_target` receipts into `unassigned` or
//! double-counting KPIs across pages.

use std::collections::{BTreeSet, HashSet};

use flowy_agent::{
    AcceptedMessage, ChatLine, ChildWorkSummary, PendingCard, ReceiptState,
    SessionSummary, TurnStatus, WorkStatus,
};

use super::board::{
    BranchPhase, ChildWorkDelegation, DeliverableFormat, SkillLabel,
    WorkBranchSummary, WorkContractProgress, project_session_branches,
};
use super::{clip_label, redact_trace_text, short_id};

/// Project all owned child Workers of a `SessionRole::Router` conversation into
/// deduplicated [`WorkBranchSummary`] lanes and concise [`ChildWorkDelegation`]
/// hierarchy summaries.
pub fn project_router_children<FLines, FReceipts, FContract>(
    parent: &SessionSummary,
    child_works: &[ChildWorkSummary],
    overridden_hashes: &BTreeSet<String>,
    localize: &dyn Fn(&str) -> String,
    mut lines_for: FLines,
    mut receipts_for: FReceipts,
    contract_for: FContract,
) -> (Vec<WorkBranchSummary>, Vec<ChildWorkDelegation>)
where
    FLines: FnMut(&str) -> Vec<ChatLine>,
    FReceipts: FnMut(&str) -> Vec<(AcceptedMessage, ReceiptState)>,
    FContract: Fn(&str) -> Option<WorkStatus>,
{
    let mut out = Vec::new();
    let mut delegations = Vec::new();
    let mut seen_work_ids = HashSet::new();

    for work in child_works {
        if !seen_work_ids.insert(work.work_id.clone()) {
            continue;
        }
        let child_summary = SessionSummary {
            id: work.work_id.clone(),
            title: work.title.clone(),
            updated_at: work.updated_at,
            project_id: parent.project_id.clone(),
            pinned: false,
            archived: parent.archived,
        };
        let lines = lines_for(&work.work_id);
        let child_receipts = receipts_for(&work.work_id);
        let is_busy =
            matches!(work.state.as_str(), "running" | "verifying" | "stopping")
                || work.turn_status.is_busy();
        let delivery_failed =
            matches!(work.turn_status, TurnStatus::DeliveryFailed);
        let child_contract = contract_for(&work.work_id)
            .as_ref()
            .and_then(WorkContractProgress::from_status);

        let mut child_branches = project_session_branches(
            &child_summary,
            &lines,
            is_busy,
            delivery_failed,
            work.heartbeat.as_ref(),
            &child_receipts,
            overridden_hashes,
            child_contract.clone(),
            localize,
            &contract_for,
        );

        // If the child Worker is parked on a pending approval/disambiguation
        // card before producing any verification report, guarantee a blocker
        // branch is visible on the parent Router's board group.
        if !work.pending_cards.is_empty()
            && !child_branches.iter().any(|b| b.phase.is_blocker())
        {
            let raw_detail = match &work.pending_cards[0] {
                PendingCard::ToolApproval {
                    tool_name,
                    risk_reason,
                    ..
                } => format!("{tool_name}: {risk_reason}"),
                PendingCard::Disambiguation { prompt, .. } => prompt.clone(),
            };
            let card_detail = clip_label(&redact_trace_text(&raw_detail), 96);
            let branch_title = if work.title.trim().is_empty() {
                short_id(&work.work_id)
            } else {
                clip_label(&redact_trace_text(&work.title), 48)
            };
            child_branches.retain(|b| !matches!(b.phase, BranchPhase::Idle));
            child_branches.push(WorkBranchSummary {
                id: format!("work:{}:approval", work.work_id),
                owner_session_id: Some(work.work_id.clone()),
                skill_label: SkillLabel::Key("board.skill_work"),
                phase: BranchPhase::NeedsApproval {
                    report_id: String::new(),
                    artifact_path: String::new(),
                    summary: card_detail.clone(),
                    issues: Vec::new(),
                    semantic_hint: None,
                    provenance_warnings: Vec::new(),
                },
                deliverable_name: branch_title,
                deliverable_path: None,
                format: DeliverableFormat::Other,
                is_workspace_change: false,
                progress_pct: 60.0,
                detail: card_detail,
                work_progress: child_contract,
            });
        }

        let worker_is_running =
            is_busy || child_branches.iter().any(|b| b.phase.is_running());
        let worker_has_blocker =
            child_branches.iter().any(|b| b.phase.is_blocker())
                || !work.pending_cards.is_empty()
                || work.state == "awaiting_approval";
        let branch_deliverables = child_branches
            .iter()
            .filter(|b| {
                !b.is_workspace_change
                    && b.deliverable_path.is_some()
                    && matches!(b.phase, BranchPhase::Delivered { .. })
            })
            .count();
        let deliverable_count =
            branch_deliverables.max(work.delivered_artifact_ids.len());
        let worker_title = if work.title.trim().is_empty() {
            short_id(&work.work_id)
        } else {
            clip_label(&redact_trace_text(&work.title), 72)
        };
        let status_hint = build_child_status_hint(
            work,
            &child_branches,
            worker_is_running,
            worker_has_blocker,
            deliverable_count,
            localize,
        );
        delegations.push(ChildWorkDelegation {
            session_id: work.work_id.clone(),
            title: worker_title,
            is_running: worker_is_running,
            has_blocker: worker_has_blocker,
            deliverable_count,
            status_hint,
        });

        // Prefix branch IDs with `work:<worker_id>:` so sibling Workers never
        // collide on branch IDs inside the parent Router's board group, and
        // tag `owner_session_id` so Flowchart Modal actions route to the child.
        for mut branch in child_branches {
            if !branch.id.starts_with("work:") {
                branch.id = format!("work:{}:{}", work.work_id, branch.id);
            }
            branch.owner_session_id = Some(work.work_id.clone());
            out.push(branch);
        }
    }

    (out, delegations)
}

fn build_child_status_hint(
    work: &ChildWorkSummary,
    branches: &[WorkBranchSummary],
    is_running: bool,
    has_blocker: bool,
    deliverable_count: usize,
    localize: &dyn Fn(&str) -> String,
) -> String {
    if has_blocker {
        if let Some(detail) = branches.iter().find_map(|b| match &b.phase {
            BranchPhase::NeedsApproval { summary, .. }
                if !summary.trim().is_empty() =>
            {
                Some(clip_label(&redact_trace_text(summary), 96))
            }
            _ => None,
        }) {
            return detail;
        }
        if matches!(work.turn_status, TurnStatus::DeliveryFailed) {
            return localize("chat.status_delivery_failed");
        }
        return localize("board.state_needs_approval");
    }
    if is_running {
        if let Some(hb_text) = work.heartbeat.as_ref().and_then(|hb| {
            crate::pending_card::format_heartbeat_line_localized(hb, localize)
        }) {
            let safe = clip_label(&redact_trace_text(&hb_text), 96);
            if !safe.is_empty() {
                return safe;
            }
        }
        if let Some(detail) = branches
            .iter()
            .find(|b| b.phase.is_running())
            .map(|b| clip_label(&redact_trace_text(&b.detail), 96))
            .filter(|s| !s.is_empty())
        {
            return detail;
        }
        return localize(work.turn_status.key());
    }
    match work.state.as_str() {
        "failed" => localize("chat.status_error"),
        "stopped" => localize("chat.status_stopped"),
        "partial" if !work.missing_items.is_empty() => {
            clip_label(&redact_trace_text(&work.missing_items.join(", ")), 96)
        }
        "delivered" => localize("verify.card.passed"),
        "completed" if deliverable_count > 0 => localize("verify.card.passed"),
        "completed" => localize("chat.status_completed"),
        _ if deliverable_count > 0 => localize("verify.card.passed"),
        _ => localize(work.turn_status.key()),
    }
}

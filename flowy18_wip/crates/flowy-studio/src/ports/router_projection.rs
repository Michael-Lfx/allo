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
    BranchPhase, DeliverableFormat, SkillLabel, WorkBranchSummary,
    WorkContractProgress, project_session_branches,
};
use super::{clip_label, short_id};

/// Project all owned child Workers of a `SessionRole::Router` conversation into
/// deduplicated [`WorkBranchSummary`] lanes.
pub fn project_router_child_branches<FLines, FReceipts, FContract>(
    parent: &SessionSummary,
    child_works: &[ChildWorkSummary],
    overridden_hashes: &BTreeSet<String>,
    mut lines_for: FLines,
    mut receipts_for: FReceipts,
    contract_for: FContract,
) -> Vec<WorkBranchSummary>
where
    FLines: FnMut(&str) -> Vec<ChatLine>,
    FReceipts: FnMut(&str) -> Vec<(AcceptedMessage, ReceiptState)>,
    FContract: Fn(&str) -> Option<WorkStatus>,
{
    let mut out = Vec::new();
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
            &contract_for,
        );

        // If the child Worker is parked on a pending approval/disambiguation
        // card before producing any verification report, guarantee a blocker
        // branch is visible on the parent Router's board group.
        if !work.pending_cards.is_empty()
            && !child_branches.iter().any(|b| b.phase.is_blocker())
        {
            let card_detail = match &work.pending_cards[0] {
                PendingCard::ToolApproval {
                    tool_name,
                    risk_reason,
                    ..
                } => clip_label(&format!("{tool_name}: {risk_reason}"), 96),
                PendingCard::Disambiguation { prompt, .. } => {
                    clip_label(prompt, 96)
                }
            };
            let title = if work.title.trim().is_empty() {
                short_id(&work.work_id)
            } else {
                clip_label(&work.title, 48)
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
                deliverable_name: title,
                deliverable_path: None,
                format: DeliverableFormat::Other,
                progress_pct: 60.0,
                detail: card_detail,
                work_progress: child_contract,
            });
        }

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

    out
}

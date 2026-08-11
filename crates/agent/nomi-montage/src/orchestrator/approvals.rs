//! HITL approvals — `awaiting_human` → approve / reject / send_back.

use serde::{Deserialize, Serialize};

use crate::checkpoint::Checkpoint;
use crate::error::{MontageError, MontageResult};
use crate::pipeline::{PipelineManifest, StageStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Reject,
    SendBack,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalRequest {
    pub stage: String,
    pub decision: ApprovalDecision,
    #[serde(default)]
    pub note: Option<String>,
    /// Required when `decision == send_back`: which earlier stage to rewind to.
    #[serde(default)]
    pub send_back_to: Option<String>,
}

/// Applies an approval decision to the checkpoint in place. Does not persist —
/// callers own the `CheckpointStore::write` call so this stays a pure function.
pub fn apply_approval(
    checkpoint: &mut Checkpoint,
    manifest: &PipelineManifest,
    request: &ApprovalRequest,
) -> MontageResult<()> {
    if checkpoint.awaiting_human_stage.as_deref() != Some(request.stage.as_str()) {
        return Err(MontageError::InvalidParams(format!(
            "project is not awaiting human approval for stage '{}' (currently awaiting: {:?})",
            request.stage, checkpoint.awaiting_human_stage
        )));
    }
    manifest
        .stage(&request.stage)
        .ok_or_else(|| MontageError::StageNotFound(request.stage.clone(), manifest.name.clone()))?;

    if let Some(note) = &request.note {
        checkpoint
            .notes
            .push(format!("[approval:{:?}] {note}", request.decision));
    }

    match request.decision {
        ApprovalDecision::Approve => {
            checkpoint.set_stage_status(&request.stage, StageStatus::Completed);
            checkpoint.awaiting_human_stage = None;
            checkpoint.status = crate::checkpoint::ProjectRunStatus::Idle;
            if let Some(next) = manifest.next_stage(&request.stage) {
                checkpoint.current_stage = next.name.clone();
            }
        }
        ApprovalDecision::Reject => {
            checkpoint.set_stage_status(&request.stage, StageStatus::Failed);
            checkpoint.awaiting_human_stage = None;
            checkpoint.status = crate::checkpoint::ProjectRunStatus::Failed;
            checkpoint.last_error = Some(format!(
                "human rejected stage '{}'{}",
                request.stage,
                request
                    .note
                    .as_ref()
                    .map(|n| format!(": {n}"))
                    .unwrap_or_default()
            ));
        }
        ApprovalDecision::SendBack => {
            let target = request.send_back_to.clone().ok_or_else(|| {
                MontageError::InvalidParams("send_back requires 'send_back_to'".into())
            })?;
            manifest
                .stage(&target)
                .ok_or_else(|| MontageError::StageNotFound(target.clone(), manifest.name.clone()))?;
            let counters = checkpoint.counters_mut(&request.stage);
            counters.send_backs += 1;
            let send_backs = counters.send_backs;
            let max_send_backs = manifest.orchestration.max_send_backs;
            if send_backs > max_send_backs {
                checkpoint.status = crate::checkpoint::ProjectRunStatus::Failed;
                checkpoint.last_error = Some(format!(
                    "stage '{}' exceeded max_send_backs ({max_send_backs}) — needs a human decision \
                     outside the normal loop, not another automatic retry",
                    request.stage
                ));
                return Ok(());
            }
            checkpoint.set_stage_status(&request.stage, StageStatus::Pending);
            checkpoint.set_stage_status(&target, StageStatus::Pending);
            checkpoint.current_stage = target;
            checkpoint.awaiting_human_stage = None;
            checkpoint.status = crate::checkpoint::ProjectRunStatus::Idle;
        }
    }
    checkpoint.touch();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{OrchestrationSpec, Stability, StageSpec};

    fn manifest() -> PipelineManifest {
        PipelineManifest {
            name: "test".into(),
            version: "1.0".into(),
            description: String::new(),
            category: String::new(),
            stability: Stability::Beta,
            default_checkpoint_policy: Default::default(),
            mode: Default::default(),
            orchestration: OrchestrationSpec {
                mode: "executive-producer".into(),
                skill: "x".into(),
                max_revisions_per_stage: 3,
                max_send_backs: 1,
                max_wall_time_minutes: 10,
                budget_default: None,
            },
            stages: vec![
                StageSpec {
                    name: "script".into(),
                    skill: "x".into(),
                    human_approval_default: true,
                    ..Default::default()
                },
                StageSpec {
                    name: "compose".into(),
                    skill: "y".into(),
                    human_approval_default: true,
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn approve_advances_to_next_stage() {
        let m = manifest();
        let mut cp = Checkpoint::new("p", "test", "compose");
        cp.awaiting_human_stage = Some("script".into());
        apply_approval(
            &mut cp,
            &m,
            &ApprovalRequest {
                stage: "script".into(),
                decision: ApprovalDecision::Approve,
                note: None,
                send_back_to: None,
            },
        )
        .unwrap();
        assert_eq!(cp.current_stage, "compose");
        assert!(cp.awaiting_human_stage.is_none());
    }

    #[test]
    fn send_back_beyond_limit_fails_project() {
        let m = manifest();
        let mut cp = Checkpoint::new("p", "test", "compose");
        cp.awaiting_human_stage = Some("compose".into());
        for _ in 0..2 {
            cp.awaiting_human_stage = Some("compose".into());
            apply_approval(
                &mut cp,
                &m,
                &ApprovalRequest {
                    stage: "compose".into(),
                    decision: ApprovalDecision::SendBack,
                    note: None,
                    send_back_to: Some("script".into()),
                },
            )
            .unwrap();
        }
        assert_eq!(cp.status, crate::checkpoint::ProjectRunStatus::Failed);
    }

    #[test]
    fn reject_fails_project() {
        let m = manifest();
        let mut cp = Checkpoint::new("p", "test", "script");
        cp.awaiting_human_stage = Some("script".into());
        apply_approval(
            &mut cp,
            &m,
            &ApprovalRequest {
                stage: "script".into(),
                decision: ApprovalDecision::Reject,
                note: Some("not usable".into()),
                send_back_to: None,
            },
        )
        .unwrap();
        assert_eq!(cp.status, crate::checkpoint::ProjectRunStatus::Failed);
    }
}

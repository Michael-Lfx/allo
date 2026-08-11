//! Backlot-equivalent observable state for the Agent workspace UI.

use serde::{Deserialize, Serialize};

use crate::checkpoint::{Checkpoint, StageCounters};
use crate::events::EventRecord;
use crate::pipeline::{PipelineManifest, StageStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardStage {
    pub name: String,
    pub status: StageStatus,
    pub human_approval_default: bool,
    pub produces: Vec<String>,
    pub revisions: u32,
    pub send_backs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub project_id: String,
    pub pipeline: String,
    pub status: String,
    pub current_stage: String,
    pub awaiting_human_stage: Option<String>,
    pub last_error: Option<String>,
    pub stages: Vec<BoardStage>,
    pub notes: Vec<String>,
    pub recent_events: Vec<EventRecord>,
    /// Relative path under the project root when `renders/final.mp4` (or
    /// another finished cut) exists — e.g. `"renders/final.mp4"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_video: Option<String>,
    /// Other playable clips under `renders/` and `assets/video/` (relative paths).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_clips: Vec<String>,
}

impl BoardState {
    pub fn build(manifest: &PipelineManifest, checkpoint: &Checkpoint, recent_events: Vec<EventRecord>) -> Self {
        let stages = manifest
            .stages
            .iter()
            .map(|s| {
                let counters: StageCounters = checkpoint.counters_of(&s.name);
                BoardStage {
                    name: s.name.clone(),
                    status: checkpoint.stage_status_of(&s.name),
                    human_approval_default: s.human_approval_default,
                    produces: s.produces.clone(),
                    revisions: counters.revisions,
                    send_backs: counters.send_backs,
                }
            })
            .collect();
        Self {
            project_id: checkpoint.project_id.clone(),
            pipeline: checkpoint.pipeline.clone(),
            status: checkpoint.status.as_str().to_string(),
            current_stage: checkpoint.current_stage.clone(),
            awaiting_human_stage: checkpoint.awaiting_human_stage.clone(),
            last_error: checkpoint.last_error.clone(),
            stages,
            notes: checkpoint.notes.clone(),
            recent_events,
            final_video: None,
            media_clips: Vec::new(),
        }
    }

    /// Attach on-disk media discovery (call after [`Self::build`]).
    pub fn with_media(mut self, final_video: Option<String>, media_clips: Vec<String>) -> Self {
        self.final_video = final_video;
        self.media_clips = media_clips;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{OrchestrationSpec, Stability, StageSpec};

    #[test]
    fn board_state_reflects_checkpoint() {
        let manifest = PipelineManifest {
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
                max_send_backs: 3,
                max_wall_time_minutes: 10,
                budget_default: None,
            },
            stages: vec![StageSpec {
                name: "script".into(),
                skill: "x".into(),
                produces: vec!["script".into()],
                ..Default::default()
            }],
        };
        let mut cp = Checkpoint::new("p1", "test", "script");
        cp.set_stage_status("script", StageStatus::InProgress);
        let board = BoardState::build(&manifest, &cp, vec![]);
        assert_eq!(board.stages.len(), 1);
        assert_eq!(board.stages[0].status, StageStatus::InProgress);
    }
}

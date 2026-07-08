//! Tracks the latest in-progress long-video workflow so resume survives agent retries.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::video_segment::{LongVideoCheckpoint, long_video_work_dir, read_long_video_checkpoint};
use crate::workflows::store::{WorkflowRunRecord, WorkflowRunStatus, WorkflowRunStore};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongVideoActiveJob {
    pub run_id: String,
    pub workflow_id: String,
    pub target_duration_secs: u32,
    pub next_segment_index: usize,
    pub segment_total: usize,
    pub updated_at_unix: u64,
}

fn active_job_path() -> PathBuf {
    nomi_config::data_dir()
        .join("media")
        .join("long_video_active.json")
}

pub fn read_active_job() -> Option<LongVideoActiveJob> {
    let data = std::fs::read_to_string(active_job_path()).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn write_active_job(job: &LongVideoActiveJob) {
    let path = active_job_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(job) {
        let _ = std::fs::write(path, json);
    }
}

pub fn clear_active_job(run_id: &str) {
    if read_active_job().is_some_and(|j| j.run_id == run_id) {
        let _ = std::fs::remove_file(active_job_path());
    }
}

pub fn sync_active_job(record: &WorkflowRunRecord, checkpoint: Option<&LongVideoCheckpoint>) {
    if !record.workflow_id.starts_with("long_") {
        return;
    }
    let Some(cp) = checkpoint else {
        return;
    };
    if cp.is_complete() {
        clear_active_job(&record.run_id);
        return;
    }
    write_active_job(&LongVideoActiveJob {
        run_id: record.run_id.clone(),
        workflow_id: record.workflow_id.clone(),
        target_duration_secs: cp.target_duration_secs,
        next_segment_index: cp.next_segment_index,
        segment_total: cp.segment_total(),
        updated_at_unix: now_unix(),
    });
}

pub fn register_active_workflow(
    record: &WorkflowRunRecord,
    target_duration_secs: u32,
    segment_total: usize,
) {
    if !record.workflow_id.starts_with("long_") || segment_total <= 1 {
        return;
    }
    write_active_job(&LongVideoActiveJob {
        run_id: record.run_id.clone(),
        workflow_id: record.workflow_id.clone(),
        target_duration_secs,
        next_segment_index: 0,
        segment_total,
        updated_at_unix: now_unix(),
    });
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn record_target_duration(record: &WorkflowRunRecord) -> Option<u32> {
    record
        .inputs
        .get("duration")
        .and_then(|v| v.as_u64())
        .map(|d| d as u32)
        .filter(|d| *d > 0)
}

fn workflow_steps_incomplete(record: &WorkflowRunRecord) -> bool {
    if record.workflow_id.starts_with("long_") && !record.step_outputs.contains_key("generate") {
        return record.step_outputs.contains_key("refine_prompt")
            || record.step_outputs.contains_key("refine_motion")
            || record.step_outputs.contains_key("refine_scene");
    }
    false
}

fn is_stale_running(record: &WorkflowRunRecord) -> bool {
    let path = nomi_config::data_dir()
        .join("media")
        .join("workflows")
        .join(&record.run_id)
        .join("state.json");
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = meta.modified() else {
        return true;
    };
    modified
        .elapsed()
        .map(|e| e > Duration::from_secs(15 * 60))
        .unwrap_or(true)
}

pub fn record_is_resumable(record: &WorkflowRunRecord, target_duration_secs: Option<u32>) -> bool {
    if !record.workflow_id.starts_with("long_") {
        return false;
    }
    match record.status {
        WorkflowRunStatus::Failed => {}
        WorkflowRunStatus::Running if is_stale_running(record) => {}
        _ => return false,
    }

    let work_dir = long_video_work_dir(&record.run_id);
    if let Some(cp) = read_long_video_checkpoint(&work_dir) {
        if cp.is_complete() {
            return false;
        }
        if let Some(target) = target_duration_secs
            && cp.target_duration_secs != target
        {
            return false;
        }
        return true;
    }

    if record.status != WorkflowRunStatus::Failed || !workflow_steps_incomplete(record) {
        return false;
    }
    let Some(duration) = record_target_duration(record) else {
        return true;
    };
    target_duration_secs.is_none_or(|target| target == duration)
}

/// Prefer pinned active job, then newest resumable workflow run.
pub fn find_resumable_long_video_run(
    store: &WorkflowRunStore,
    target_duration_secs: Option<u32>,
) -> Option<WorkflowRunRecord> {
    if let Some(active) = read_active_job()
        && let Some(record) = store.get(&active.run_id)
        && record_is_resumable(
            &record,
            target_duration_secs.or(Some(active.target_duration_secs)),
        )
    {
        return Some(record);
    }

    store
        .list_records_newest_first()
        .into_iter()
        .find(|record| record_is_resumable(record, target_duration_secs))
}

pub fn user_wants_resume(params: &serde_json::Value, objective: Option<&str>) -> bool {
    if params
        .get("continue")
        .or_else(|| params.get("resume"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    let Some(text) = objective else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    [
        "继续",
        "续传",
        "接着",
        "恢复",
        "未完成",
        "continue",
        "resume",
        "retry",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use serde_json::json;

    use crate::workflows::store::WorkflowRunRecord;

    #[test]
    fn detects_continue_intent_in_chinese() {
        assert!(user_wants_resume(
            &json!({}),
            Some("积分充好了，请继续生成视频")
        ));
    }

    #[test]
    fn resumable_when_generate_missing_but_refine_done() {
        let record = WorkflowRunRecord {
            run_id: "r1".into(),
            workflow_id: "long_txt2video".into(),
            status: WorkflowRunStatus::Failed,
            inputs: json!({"duration": 20}),
            current_step: None,
            step_outputs: HashMap::from([("refine_prompt".into(), json!({}))]),
            artifacts: vec![],
            error: Some("credits".into()),
        };
        assert!(record_is_resumable(&record, Some(20)));
    }
}

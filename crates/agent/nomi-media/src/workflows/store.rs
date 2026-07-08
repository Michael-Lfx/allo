//! File-backed workflow run state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use nomi_types::ToolError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub run_id: String,
    pub workflow_id: String,
    pub status: WorkflowRunStatus,
    pub inputs: Value,
    pub current_step: Option<String>,
    #[serde(default)]
    pub step_outputs: HashMap<String, Value>,
    #[serde(default)]
    pub artifacts: Vec<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

pub struct WorkflowRunStore {
    root: PathBuf,
    memory: Arc<RwLock<HashMap<String, WorkflowRunRecord>>>,
}

pub fn is_terminal_workflow_status(status: &WorkflowRunStatus) -> bool {
    matches!(
        status,
        WorkflowRunStatus::Succeeded | WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled
    )
}

impl WorkflowRunStore {
    pub fn new() -> Self {
        Self::with_root(nomi_config::data_dir().join("media").join("workflows"))
    }

    pub fn with_root(root: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&root);
        Self {
            root,
            memory: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn create_run(&self, workflow_id: &str, inputs: Value) -> WorkflowRunRecord {
        let run_id = Uuid::new_v4().to_string();
        let record = WorkflowRunRecord {
            run_id: run_id.clone(),
            workflow_id: workflow_id.to_string(),
            status: WorkflowRunStatus::Pending,
            inputs,
            current_step: None,
            step_outputs: HashMap::new(),
            artifacts: Vec::new(),
            error: None,
        };
        self.save(&record);
        record
    }

    pub fn get(&self, run_id: &str) -> Option<WorkflowRunRecord> {
        if let Ok(guard) = self.memory.read()
            && let Some(rec) = guard.get(run_id)
        {
            return Some(rec.clone());
        }
        let path = self.run_path(run_id);
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self, record: &WorkflowRunRecord) {
        if let Ok(mut guard) = self.memory.write() {
            guard.insert(record.run_id.clone(), record.clone());
        }
        let path = self.run_path(&record.run_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(record) {
            let _ = std::fs::write(path, json);
        }
        self.write_manifest(record);
    }

    /// Block until a workflow run reaches a terminal state (server-side polling).
    pub async fn wait_until_terminal(
        &self,
        run_id: &str,
        timeout: Duration,
    ) -> Result<WorkflowRunRecord, ToolError> {
        let started = Instant::now();
        let poll_interval = Duration::from_secs(3);
        let mut last_step: Option<String> = None;
        let mut last_progress_at = Instant::now();
        let progress_interval = Duration::from_secs(15);

        loop {
            let record = self.get(run_id).ok_or_else(|| {
                ToolError::ExecutionFailed(format!("workflow run not found: {run_id}"))
            })?;

            if is_terminal_workflow_status(&record.status) {
                return Ok(record);
            }

            if started.elapsed() >= timeout {
                return Err(ToolError::Timeout(format!(
                    "workflow run {run_id} still {:?} after {}s — call media_workflow_status later with the same run_id",
                    record.status,
                    timeout.as_secs()
                )));
            }

            if record.current_step != last_step {
                if let Some(step) = &record.current_step {
                    nomi_types::report_tool_progress(format!(
                        "工作流进行中（{}/{}）…",
                        record.workflow_id, step
                    ));
                }
                last_step = record.current_step.clone();
                last_progress_at = Instant::now();
            } else if last_progress_at.elapsed() >= progress_interval {
                nomi_types::report_tool_progress(format!(
                    "工作流仍在运行（{}），已等待 {} 秒…",
                    record.workflow_id,
                    started.elapsed().as_secs()
                ));
                last_progress_at = Instant::now();
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    fn write_manifest(&self, record: &WorkflowRunRecord) {
        let manifest = super::manifest::WorkflowManifest::from_record(record);
        let path = self.root.join(&record.run_id).join("manifest.json");
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            let _ = std::fs::write(path, json);
        }
    }

    fn run_path(&self, run_id: &str) -> PathBuf {
        self.root.join(run_id).join("state.json")
    }

    /// All persisted runs, newest state file first.
    pub fn list_records_newest_first(&self) -> Vec<WorkflowRunRecord> {
        let mut records = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return records;
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let Some(run_id) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if let Some(record) = self.get(&run_id) {
                records.push(record);
            }
        }
        records.sort_by_key(|b| std::cmp::Reverse(self.record_mtime(b)));
        records
    }

    fn record_mtime(&self, record: &WorkflowRunRecord) -> std::time::SystemTime {
        self.run_path(&record.run_id)
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    }
}

impl Default for WorkflowRunStore {
    fn default() -> Self {
        Self::new()
    }
}

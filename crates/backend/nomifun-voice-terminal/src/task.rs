//! Task FSM and background task management.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.6.

use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Idle,
    Executing,
    SuspendedAwaitingConfirmation,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_id: String,
    pub goal: String,
    pub state: TaskState,
    pub started_at: Instant,
    pub summary: Option<String>,
}

pub struct TaskManager {
    tasks: Vec<TaskInfo>,
    active_task_id: Option<String>,
    next_seq: u64,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            active_task_id: None,
            next_seq: 1,
        }
    }

    pub fn start_task(&mut self, goal: &str) -> String {
        let task_id = format!("task_{}", self.next_seq);
        self.next_seq += 1;

        let task = TaskInfo {
            task_id: task_id.clone(),
            goal: goal.to_string(),
            state: TaskState::Executing,
            started_at: Instant::now(),
            summary: None,
        };

        self.tasks.push(task);
        self.active_task_id = Some(task_id.clone());
        task_id
    }

    pub fn active_task(&self) -> Option<&TaskInfo> {
        self.active_task_id
            .as_ref()
            .and_then(|id| self.tasks.iter().find(|t| &t.task_id == id))
    }

    pub fn active_task_mut(&mut self) -> Option<&mut TaskInfo> {
        if let Some(ref id) = self.active_task_id {
            self.tasks.iter_mut().find(|t| &t.task_id == id)
        } else {
            None
        }
    }

    pub fn query_status(&self) -> String {
        if let Some(task) = self.active_task() {
            let elapsed = task.started_at.elapsed().as_secs();
            format!(
                "任务[{}]正在执行中，已运行{}秒，当前状态={:?}",
                task.goal, elapsed, task.state
            )
        } else {
            "当前无正在执行的后台任务".to_string()
        }
    }

    pub fn cancel_task(&mut self) -> Result<String, String> {
        if let Some(task) = self.active_task_mut() {
            task.state = TaskState::Cancelled;
            let id = task.task_id.clone();
            Ok(id)
        } else {
            Err("No active task to cancel".to_string())
        }
    }

    pub fn complete_task(&mut self, summary: &str) -> Result<String, String> {
        if let Some(task) = self.active_task_mut() {
            task.state = TaskState::Completed;
            task.summary = Some(summary.to_string());
            let id = task.task_id.clone();
            Ok(id)
        } else {
            Err("No active task to complete".to_string())
        }
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

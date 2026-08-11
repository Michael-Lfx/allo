//! `events.jsonl` — append-only project activity log (feeds Board / future SSE).

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::MontageResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ProjectCreated,
    RunStarted,
    StageStarted,
    ToolCalled,
    ToolResult,
    ArtifactWritten,
    CheckpointWritten,
    ReviewRound,
    AwaitingHuman,
    Approved,
    Rejected,
    SendBack,
    StageCompleted,
    Error,
    Cancelled,
    Finished,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectCreated => "project_created",
            Self::RunStarted => "run_started",
            Self::StageStarted => "stage_started",
            Self::ToolCalled => "tool_called",
            Self::ToolResult => "tool_result",
            Self::ArtifactWritten => "artifact_written",
            Self::CheckpointWritten => "checkpoint_written",
            Self::ReviewRound => "review_round",
            Self::AwaitingHuman => "awaiting_human",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::SendBack => "send_back",
            Self::StageCompleted => "stage_completed",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Finished => "finished",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub at: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub kind: EventKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl EventRecord {
    pub fn new(project_id: impl Into<String>, kind: EventKind, message: impl Into<String>) -> Self {
        Self {
            at: Utc::now().to_rfc3339(),
            project_id: project_id.into(),
            stage: None,
            kind,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Append one event as a JSON line. Best-effort directory creation.
pub fn append_event(events_path: &Path, event: &EventRecord) -> MontageResult<()> {
    use std::io::Write;
    if let Some(parent) = events_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_path)?;
    let line = serde_json::to_string(event)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Read all events (small files only — this is a debug/board-state helper, not a log
/// viewer). Corrupted lines are skipped rather than failing the whole read.
pub fn read_events(events_path: &Path) -> MontageResult<Vec<EventRecord>> {
    if !events_path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(events_path)?;
    Ok(raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<EventRecord>(l).ok())
        .collect())
}

/// Read the most recent `limit` events (cheap tail for status endpoints).
pub fn read_recent_events(events_path: &Path, limit: usize) -> MontageResult<Vec<EventRecord>> {
    let mut all = read_events(events_path)?;
    if all.len() > limit {
        all.drain(0..all.len() - limit);
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_event(
            &path,
            &EventRecord::new("p1", EventKind::ProjectCreated, "created"),
        )
        .unwrap();
        append_event(
            &path,
            &EventRecord::new("p1", EventKind::StageStarted, "research")
                .with_stage("research"),
        )
        .unwrap();
        let events = read_events(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].stage.as_deref(), Some("research"));
    }
}

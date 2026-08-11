//! Stage runtime status — independent of the static `StageSpec` manifest shape.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    #[default]
    Pending,
    InProgress,
    AwaitingHuman,
    Completed,
    Failed,
}

impl StageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::AwaitingHuman => "awaiting_human",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn is_terminal_for_run(self) -> bool {
        matches!(self, Self::AwaitingHuman | Self::Failed)
    }
}

impl std::fmt::Display for StageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

//! Automation definitions and run records.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schedule::TriggerSpec;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn parse(value: &str) -> Result<Self, uuid::Error> {
                Uuid::parse_str(value).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(AutomationId);
id_type!(RunId);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Runs one background agent turn with this prompt.
    AgentTask { prompt: String },
    /// Only shows a notification.
    Reminder { text: String },
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    #[default]
    Read,
    Write,
}

impl Access {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            _ => None,
        }
    }
}

/// What the UI editor and the model tools submit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationInput {
    pub title: String,
    pub action: Action,
    #[serde(default)]
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub access: Access,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Automation {
    pub id: AutomationId,
    pub title: String,
    pub action: Action,
    pub trigger: TriggerSpec,
    pub cwd: Option<PathBuf>,
    pub access: Access,
    pub enabled: bool,
    pub next_run_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Automation {
    pub fn input(&self) -> AutomationInput {
        AutomationInput {
            title: self.title.clone(),
            action: self.action.clone(),
            trigger: self.trigger,
            cwd: self.cwd.clone(),
            access: self.access,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    /// The app stopped while the run was in progress. Not retried.
    Interrupted,
    /// The workspace was busy when the run started.
    Skipped,
    Cancelled,
}

impl RunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Running,
            Self::Succeeded,
            Self::Failed,
            Self::Interrupted,
            Self::Skipped,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
    }

    pub const fn is_final(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunTrigger {
    Manual,
    Scheduled,
}

impl RunTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scheduled => "scheduled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "scheduled" => Some(Self::Scheduled),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub automation_id: AutomationId,
    pub status: RunStatus,
    pub trigger: RunTrigger,
    /// Background session that ran the turn; open it to see the result.
    pub session_id: Option<String>,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub error: Option<String>,
}

impl Run {
    pub fn queued(
        automation_id: AutomationId,
        trigger: RunTrigger,
        now_ms: i64,
    ) -> Self {
        Self {
            id: RunId::generate(),
            automation_id,
            status: RunStatus::Queued,
            trigger,
            session_id: None,
            created_at_ms: now_ms,
            started_at_ms: None,
            finished_at_ms: None,
            error: None,
        }
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

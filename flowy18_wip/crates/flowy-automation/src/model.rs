//! Automation definitions and run records.

use std::fmt;

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
    /// Dispatches one Worker in the owning conversation with this prompt.
    AgentTask { prompt: String },
    /// Tells the user in the owning conversation.
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

/// What the model tools and the page editor submit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationInput {
    pub title: String,
    pub action: Action,
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub access: Access,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Automation {
    pub id: AutomationId,
    /// The Router conversation that created it. Runs are dispatched there and
    /// report back there.
    pub conversation_id: String,
    pub title: String,
    pub action: Action,
    pub trigger: TriggerSpec,
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
            access: self.access,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    /// The app stopped while the run was in progress. Not retried.
    Interrupted,
    Cancelled,
}

impl RunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Running,
            Self::Succeeded,
            Self::Failed,
            Self::Interrupted,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
    }

    pub const fn is_final(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub automation_id: AutomationId,
    pub status: RunStatus,
    /// Worker dispatched in the owning conversation; `None` for reminders.
    pub work_id: Option<String>,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub error: Option<String>,
}

impl Run {
    /// A run starts as soon as it is created.
    pub fn started(automation_id: AutomationId, now_ms: i64) -> Self {
        Self {
            id: RunId::generate(),
            automation_id,
            status: RunStatus::Running,
            work_id: None,
            created_at_ms: now_ms,
            started_at_ms: Some(now_ms),
            finished_at_ms: None,
            error: None,
        }
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

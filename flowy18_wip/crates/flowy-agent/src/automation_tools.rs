//! Model-facing automation management. Each tool parses its arguments, calls
//! the [`AutomationService`] shared with the automation page, and returns
//! JSON. Sessions created by automation runs never get these tools.

use std::path::PathBuf;
use std::sync::Arc;

use flowy_agent_core::tools::{ToolDefinition, ToolRegistry};
use flowy_agent_core::{ToolCallArgs, ToolExecutionResult, ToolRunContext};
use flowy_automation::schedule::TriggerSpec;
use flowy_automation::{
    Access, Action, AutomationId, AutomationInput, AutomationService, RunId,
    ServiceError,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const HISTORY_LIMIT: usize = 20;
const DOWNGRADE_NOTE: &str = "Write access needs a read-write conversation; the automation was saved as read-only.";

#[derive(Clone, Copy)]
enum Op {
    Create,
    List,
    Get,
    Update,
    Pause,
    Resume,
    Delete,
    RunNow,
    History,
    CancelRun,
}

const TOOLS: [(Op, &str, &str); 10] = [
    (
        Op::Create,
        "create",
        "Create an automation. An agent_task runs the prompt as a background conversation; a reminder only shows a notification. Without a schedule it runs only through automation_run_now. Workspace access defaults to read; write needs a read-write conversation.",
    ),
    (
        Op::List,
        "list",
        "List saved automations with their enabled state and next run time (Unix UTC milliseconds).",
    ),
    (Op::Get, "get", "Read one automation."),
    (
        Op::Update,
        "update",
        "Replace an automation's whole definition. Its schedule restarts from now.",
    ),
    (
        Op::Pause,
        "pause",
        "Stop scheduled runs. A run already in progress continues.",
    ),
    (
        Op::Resume,
        "resume",
        "Resume scheduled runs. Occurrences missed while paused are not run.",
    ),
    (
        Op::Delete,
        "delete",
        "Delete an automation and its run history, cancelling its runs in progress.",
    ),
    (
        Op::RunNow,
        "run_now",
        "Queue one run now and return it. Check its result with automation_history.",
    ),
    (
        Op::History,
        "history",
        "Recent runs, newest first. A run's session_id is the conversation that holds its result.",
    ),
    (
        Op::CancelRun,
        "cancel_run",
        "Cancel a queued or running run.",
    ),
];

/// Registers the ten tools. `writable` is whether the current conversation
/// itself may write; it caps the access of automations the model saves.
pub fn register(
    registry: &ToolRegistry,
    service: &Arc<AutomationService>,
    writable: bool,
) {
    for (op, name, description) in TOOLS {
        let service = Arc::clone(service);
        registry.register(Arc::new(ToolDefinition::new(
            format!("automation_{name}"),
            description,
            op.parameters(),
            move |args: ToolCallArgs, _context: ToolRunContext| {
                let service = Arc::clone(&service);
                Box::pin(
                    async move { op.call(&service, args.arguments, writable) },
                )
            },
        )));
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    title: String,
    action: Action,
    #[serde(default)]
    schedule: Option<Schedule>,
    #[serde(default)]
    workspace: Option<Workspace>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Schedule {
    trigger: TriggerSpec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workspace {
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    access: Access,
}

impl Definition {
    /// Returns the input and whether write access was lowered to read.
    fn into_input(self, writable: bool) -> (AutomationInput, bool) {
        let (cwd, requested) =
            self.workspace.map_or((None, Access::Read), |workspace| {
                (workspace.path, workspace.access)
            });
        let downgraded = requested == Access::Write && !writable;
        let input = AutomationInput {
            title: self.title,
            action: self.action,
            trigger: self
                .schedule
                .map_or_else(Default::default, |schedule| schedule.trigger),
            cwd,
            access: if downgraded { Access::Read } else { requested },
        };
        (input, downgraded)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    definition: Definition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    automation_id: AutomationId,
    definition: Definition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ById {
    automation_id: AutomationId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByRun {
    run_id: RunId,
}

enum ToolError {
    Args(String),
    Service(ServiceError),
}

impl From<ServiceError> for ToolError {
    fn from(error: ServiceError) -> Self {
        Self::Service(error)
    }
}

impl Op {
    fn call(
        self,
        service: &AutomationService,
        args: Value,
        writable: bool,
    ) -> ToolExecutionResult {
        match self.run(service, args, writable) {
            Ok(value) => ToolExecutionResult::success_value(value),
            Err(ToolError::Args(message)) => {
                ToolExecutionResult::error("invalid_request", message)
            }
            Err(ToolError::Service(error)) => {
                let code = match &error {
                    ServiceError::NotFound => "not_found",
                    ServiceError::Invalid(_) => "invalid_request",
                    ServiceError::Schedule(_) => "invalid_schedule",
                    ServiceError::Store(_) => "storage_error",
                };
                ToolExecutionResult::error(code, error.to_string())
            }
        }
    }

    fn run(
        self,
        service: &AutomationService,
        args: Value,
        writable: bool,
    ) -> Result<Value, ToolError> {
        Ok(match self {
            Self::Create => {
                let Create { definition } = parse(args)?;
                let (input, downgraded) = definition.into_input(writable);
                saved(&service.create(input)?, downgraded)
            }
            Self::Update => {
                let Update {
                    automation_id,
                    definition,
                } = parse(args)?;
                let (input, downgraded) = definition.into_input(writable);
                saved(&service.update(automation_id, input)?, downgraded)
            }
            Self::List => json!({ "automations": service.list()? }),
            Self::Get => {
                to_json(&service.get(parse::<ById>(args)?.automation_id)?)
            }
            Self::Pause => {
                to_json(&service.pause(parse::<ById>(args)?.automation_id)?)
            }
            Self::Resume => {
                to_json(&service.resume(parse::<ById>(args)?.automation_id)?)
            }
            Self::Delete => {
                let ById { automation_id } = parse(args)?;
                service.delete(automation_id)?;
                json!({ "automation_id": automation_id, "deleted": true })
            }
            Self::RunNow => {
                to_json(&service.run_now(parse::<ById>(args)?.automation_id)?)
            }
            Self::History => {
                let ById { automation_id } = parse(args)?;
                json!({ "runs": service.history(automation_id, HISTORY_LIMIT)? })
            }
            Self::CancelRun => {
                let ByRun { run_id } = parse(args)?;
                json!({ "run_id": run_id, "cancelled": service.cancel(run_id)? })
            }
        })
    }

    fn parameters(self) -> Value {
        let id = json!({ "type": "string", "description": "UUID" });
        let (properties, required): (Value, &[&str]) = match self {
            Self::List => (json!({}), &[]),
            Self::Create => (
                json!({ "definition": definition_schema() }),
                &["definition"],
            ),
            Self::Update => (
                json!({ "automation_id": id, "definition": definition_schema() }),
                &["automation_id", "definition"],
            ),
            Self::CancelRun => (json!({ "run_id": id }), &["run_id"]),
            _ => (json!({ "automation_id": id }), &["automation_id"]),
        };
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": properties,
            "required": required,
        })
    }
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, ToolError> {
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args)
        .map_err(|error| ToolError::Args(error.to_string()))
}

fn to_json(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn saved(automation: &flowy_automation::Automation, downgraded: bool) -> Value {
    let mut value = to_json(automation);
    if downgraded && let Some(object) = value.as_object_mut() {
        object.insert("access_note".to_owned(), json!(DOWNGRADE_NOTE));
    }
    value
}

fn definition_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "action"],
        "properties": {
            "title": { "type": "string" },
            "action": { "oneOf": [
                {
                    "type": "object", "additionalProperties": false,
                    "required": ["kind", "prompt"],
                    "properties": {
                        "kind": { "const": "agent_task" },
                        "prompt": { "type": "string" }
                    }
                },
                {
                    "type": "object", "additionalProperties": false,
                    "required": ["kind", "text"],
                    "properties": {
                        "kind": { "const": "reminder" },
                        "text": { "type": "string" }
                    }
                }
            ]},
            "schedule": {
                "type": "object", "additionalProperties": false,
                "required": ["trigger"],
                "description": "Omit to run only on demand.",
                "properties": { "trigger": { "oneOf": [
                    {
                        "type": "object", "required": ["kind"],
                        "properties": { "kind": { "const": "manual" } }
                    },
                    {
                        "type": "object", "required": ["kind", "scheduled_at_ms"],
                        "properties": {
                            "kind": { "const": "once" },
                            "scheduled_at_ms": { "type": "integer", "description": "Unix UTC milliseconds" }
                        }
                    },
                    {
                        "type": "object",
                        "required": ["kind", "time_zone", "local_time", "frequency"],
                        "properties": {
                            "kind": { "const": "calendar" },
                            "time_zone": { "type": "string", "description": "IANA time zone, e.g. Asia/Singapore" },
                            "local_time": { "type": "string", "description": "HH:MM:SS" },
                            "frequency": { "oneOf": [
                                {
                                    "type": "object", "required": ["kind"],
                                    "properties": { "kind": { "const": "daily" } }
                                },
                                {
                                    "type": "object", "required": ["kind", "weekday"],
                                    "properties": {
                                        "kind": { "const": "weekly" },
                                        "weekday": { "enum": ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] }
                                    }
                                }
                            ]}
                        }
                    }
                ]}}
            },
            "workspace": {
                "type": "object", "additionalProperties": false,
                "description": "Omit to run in a fresh conversation folder.",
                "properties": {
                    "path": { "type": "string", "description": "Absolute folder the run works in." },
                    "access": { "enum": ["read", "write"] }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests;

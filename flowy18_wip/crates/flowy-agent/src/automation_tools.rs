//! Automation management for read-write Workers in a conversation. Each tool
//! parses its arguments, calls the [`AutomationService`] shared with the
//! automation page, and returns JSON. Every tool is scoped to the Worker's
//! conversation: automations are created there, run there and report there.

use std::sync::Arc;

use flowy_agent_core::tools::{ToolDefinition, ToolRegistry};
use flowy_agent_core::{ToolCallArgs, ToolExecutionResult, ToolRunContext};
use flowy_automation::schedule::TriggerSpec;
use flowy_automation::{
    Access, Action, Automation, AutomationId, AutomationInput,
    AutomationService, RunId, ServiceError,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const HISTORY_LIMIT: usize = 20;

#[derive(Clone, Copy)]
enum Op {
    Create,
    List,
    Get,
    Update,
    Pause,
    Resume,
    Delete,
    History,
    CancelRun,
}

const TOOLS: [(Op, &str, &str); 9] = [
    (
        Op::Create,
        "create",
        "Create a scheduled automation in this conversation. When it is due, an agent_task runs as new work in this conversation and its result is announced here; a reminder is posted here. Automations only run on their schedule.",
    ),
    (
        Op::List,
        "list",
        "List this conversation's automations with their enabled state and next run time (Unix UTC milliseconds).",
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
        "Delete an automation and its run history, cancelling its run in progress.",
    ),
    (
        Op::History,
        "history",
        "Recent runs, newest first. A run's work_id is the work in this conversation that carried it out.",
    ),
    (Op::CancelRun, "cancel_run", "Cancel a running run."),
];

/// Registers the tools for a Worker of `conversation`.
pub fn register(
    registry: &ToolRegistry,
    service: &Arc<AutomationService>,
    conversation: &str,
) {
    for (op, name, description) in TOOLS {
        let service = Arc::clone(service);
        let conversation = conversation.to_owned();
        registry.register(Arc::new(ToolDefinition::new(
            format!("automation_{name}"),
            description,
            op.parameters(),
            move |args: ToolCallArgs, _context: ToolRunContext| {
                let service = Arc::clone(&service);
                let conversation = conversation.clone();
                Box::pin(async move {
                    op.call(&service, &conversation, args.arguments)
                })
            },
        )));
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    title: String,
    action: Action,
    schedule: Schedule,
    #[serde(default)]
    access: Access,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Schedule {
    trigger: TriggerSpec,
}

impl From<Definition> for AutomationInput {
    fn from(definition: Definition) -> Self {
        Self {
            title: definition.title,
            action: definition.action,
            trigger: definition.schedule.trigger,
            access: definition.access,
        }
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
        conversation: &str,
        args: Value,
    ) -> ToolExecutionResult {
        match self.run(service, conversation, args) {
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
        conversation: &str,
        args: Value,
    ) -> Result<Value, ToolError> {
        // Automations of other conversations look missing.
        let owned = |id: AutomationId| -> Result<Automation, ToolError> {
            let automation = service.get(id)?;
            if automation.conversation_id == conversation {
                Ok(automation)
            } else {
                Err(ServiceError::NotFound.into())
            }
        };
        Ok(match self {
            Self::Create => {
                let Create { definition } = parse(args)?;
                to_json(&service.create(conversation, definition.into())?)
            }
            Self::Update => {
                let Update {
                    automation_id,
                    definition,
                } = parse(args)?;
                owned(automation_id)?;
                to_json(&service.update(automation_id, definition.into())?)
            }
            Self::List => {
                json!({ "automations": service.list_for(conversation)? })
            }
            Self::Get => to_json(&owned(parse::<ById>(args)?.automation_id)?),
            Self::Pause => {
                let ById { automation_id } = parse(args)?;
                owned(automation_id)?;
                to_json(&service.pause(automation_id)?)
            }
            Self::Resume => {
                let ById { automation_id } = parse(args)?;
                owned(automation_id)?;
                to_json(&service.resume(automation_id)?)
            }
            Self::Delete => {
                let ById { automation_id } = parse(args)?;
                owned(automation_id)?;
                service.delete(automation_id)?;
                json!({ "automation_id": automation_id, "deleted": true })
            }
            Self::History => {
                let ById { automation_id } = parse(args)?;
                owned(automation_id)?;
                json!({ "runs": service.history(automation_id, HISTORY_LIMIT)? })
            }
            Self::CancelRun => {
                let ByRun { run_id } = parse(args)?;
                let run = service.run(run_id)?.ok_or(ServiceError::NotFound)?;
                owned(run.automation_id)?;
                json!({ "run_id": run_id, "cancelled": service.cancel(run_id) })
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

fn definition_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "action", "schedule"],
        "properties": {
            "title": { "type": "string" },
            "action": { "oneOf": [
                {
                    "type": "object", "additionalProperties": false,
                    "required": ["kind", "prompt"],
                    "properties": {
                        "kind": { "const": "agent_task" },
                        "prompt": { "type": "string", "description": "Self-contained task brief for the work dispatched at each run." }
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
                "properties": { "trigger": { "oneOf": [
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
            "access": {
                "enum": ["read", "write"],
                "description": "Workspace access of the work each run dispatches. Defaults to read."
            }
        }
    })
}

#[cfg(test)]
mod tests;

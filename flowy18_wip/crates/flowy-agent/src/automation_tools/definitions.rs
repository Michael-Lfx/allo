use super::{AutomationToolBridge, validate_model_arguments};
use flowy_agent_core::{ToolCallArgs, ToolRunContext, tools::ToolRegistry};
use flowy_agent_core::{ToolExecutionResult, tools::ToolDefinition};
use flowy_automation::schedule::{MisfirePolicy, SchedulePolicy, TriggerSpec};
use flowy_automation::{
    ActionSpec, AutomationId, CapabilityRequest, DeliveryTarget,
    ManualAutomationInput, ModelBinding, PrivateText, Revision, RunBudget,
    RunId, WorkspaceSpec,
};
use flowy_automation::{AutomationCommand, AutomationQuery, PortError};
use serde::Deserialize;
use serde_json::{Value, json};
use std::num::NonZeroU64;

const REQUEST_TTL_MS: i64 = 86_400_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    title: PrivateText,
    action: ActionSpec,
    workspace: Option<WorkspaceSpec>,
    model: Option<ModelBinding>,
    budget: Option<RunBudget>,
    schedule: Option<ScheduleInput>,
    #[serde(default)]
    tools: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleInput {
    trigger: TriggerSpec,
    misfire: Option<MisfirePolicy>,
    max_lateness_ms: Option<NonZeroU64>,
}

impl ScheduleInput {
    fn with_defaults(self, action: &ActionSpec) -> SchedulePolicy {
        let (misfire, max_lateness_ms) = match action {
            ActionSpec::Reminder { .. } => (MisfirePolicy::Skip, 3_600_000),
            ActionSpec::AgentTask { .. } => {
                (MisfirePolicy::RunOnce, 21_600_000)
            }
        };
        SchedulePolicy {
            trigger: self.trigger,
            misfire: self.misfire.unwrap_or(misfire),
            max_lateness_ms: self.max_lateness_ms.unwrap_or_else(|| {
                NonZeroU64::new(max_lateness_ms)
                    .expect("positive lateness default")
            }),
        }
    }
}

impl From<Definition> for ManualAutomationInput {
    fn from(value: Definition) -> Self {
        let mut resources = Vec::new();
        if let Some(workspace) = &value.workspace {
            resources.extend(workspace.roots.iter().map(|path| {
                format!("workspace:{:?}:{}", workspace.access, path.display())
            }));
        }
        if let Some(model) = &value.model {
            resources.extend(
                std::iter::once(&model.selected)
                    .chain(&model.allowed_fallbacks)
                    .map(|route| {
                        format!("model:{}/{}", route.provider, route.model)
                    }),
            );
        }
        let budget = value.budget.or_else(|| {
            if matches!(value.action, ActionSpec::AgentTask { .. }) {
                RunBudget::new(600_000, 20, 50)
            } else {
                None
            }
        });
        let schedule = value
            .schedule
            .map(|schedule| schedule.with_defaults(&value.action));
        Self {
            title: value.title,
            action: value.action,
            workspace: value.workspace,
            model: value.model,
            budget,
            schedule,
            capabilities: CapabilityRequest {
                tools: value.tools,
                resources,
                grant_ref: None,
            },
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
    expected_revision: Revision,
    definition: Definition,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Existing {
    automation_id: AutomationId,
    expected_revision: Revision,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByAutomation {
    automation_id: AutomationId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByRun {
    run_id: RunId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryDelivery {
    run_id: RunId,
    target: Option<DeliveryTarget>,
}

#[derive(Clone, Copy)]
enum Operation {
    Create,
    List,
    Get,
    Update,
    Pause,
    Resume,
    Delete,
    RunNow,
    History,
    Cancel,
    Retry,
}
enum Request {
    Command(AutomationCommand),
    Query(AutomationQuery),
}

impl Operation {
    fn request(self, args: Value) -> Result<Request, PortError> {
        fn read<T: serde::de::DeserializeOwned>(
            value: Value,
        ) -> Result<T, PortError> {
            serde_json::from_value(value).map_err(|_| PortError::InvalidRequest)
        }
        use AutomationCommand as C;
        use AutomationQuery as Q;
        Ok(Request::Command(match self {
            Self::Create => C::CreateManual {
                definition: Box::new(read::<Create>(args)?.definition.into()),
            },
            Self::Update => {
                let Update {
                    automation_id,
                    expected_revision,
                    definition,
                } = read(args)?;
                C::UpdateManual {
                    automation_id,
                    expected_revision,
                    definition: Box::new(definition.into()),
                }
            }
            Self::List => {
                if !args.as_object().is_some_and(serde_json::Map::is_empty) {
                    return Err(PortError::InvalidRequest);
                }
                return Ok(Request::Query(Q::List));
            }
            Self::Get | Self::History => {
                let ByAutomation { automation_id } = read(args)?;
                return Ok(Request::Query(if matches!(self, Self::Get) {
                    Q::Automation { automation_id }
                } else {
                    Q::History { automation_id }
                }));
            }
            Self::Cancel => C::CancelRun {
                run_id: read::<ByRun>(args)?.run_id,
            },
            Self::Retry => {
                let RetryDelivery { run_id, target } = read(args)?;
                match target {
                    Some(target) => C::RetryDeliveryTarget { run_id, target },
                    None => C::RetryDelivery { run_id },
                }
            }
            operation => {
                let Existing {
                    automation_id,
                    expected_revision,
                } = read(args)?;
                match operation {
                    Self::Pause => C::Pause {
                        automation_id,
                        expected_revision,
                    },
                    Self::Resume => C::Resume {
                        automation_id,
                        expected_revision,
                    },
                    Self::Delete => C::Archive {
                        automation_id,
                        expected_revision,
                    },
                    Self::RunNow => C::RunNow {
                        automation_id,
                        expected_revision,
                    },
                    _ => unreachable!(),
                }
            }
        }))
    }
}

impl AutomationToolBridge {
    pub fn register(&self, registry: &ToolRegistry) {
        self.install_context_guard(registry);
        use Operation::{
            Cancel, Create, Delete, Get, History, List, Pause, Resume, Retry,
            RunNow, Update,
        };
        let tools = [
            (
                Create,
                "create",
                "Create an automation or request approval. Report pending approval as pending.",
            ),
            (
                List,
                "list",
                "List saved automations with state and next scheduled UTC time.",
            ),
            (
                Get,
                "get",
                "Read the saved definition and its current revision.",
            ),
            (
                Update,
                "update",
                "Save the complete replacement definition against the expected revision or request approval.",
            ),
            (
                Pause,
                "pause",
                "Pause future scheduled triggers. Existing runs keep their state.",
            ),
            (
                Resume,
                "resume",
                "Resume scheduled triggers for the current revision.",
            ),
            (
                Delete,
                "delete",
                "Archive the automation and cancel runs that have not started.",
            ),
            (
                RunNow,
                "run_now",
                "Queue one run and return its durable run ID immediately.",
            ),
            (History, "history", "Read durable runs for an automation."),
            (
                Cancel,
                "cancel_run",
                "Request cancellation of the specified run and report the disposition.",
            ),
            (
                Retry,
                "retry_delivery",
                "Retry pending deliveries by run ID without rerunning the task.",
            ),
        ];
        let id = json!({"type":"string","format":"uuid"});
        let revision = json!({"type":"integer","minimum":1});
        for (operation, name, description) in tools {
            let (properties, required) = match operation {
                List => (json!({}), vec![]),
                Get | History => {
                    (json!({"automation_id":id}), vec!["automation_id"])
                }
                Cancel => (json!({"run_id":id}), vec!["run_id"]),
                Retry => (
                    json!({"run_id":id,"target":{"enum":["inbox","system_notification"]}}),
                    vec!["run_id"],
                ),
                Create => (
                    json!({"definition":definition_schema()}),
                    vec!["definition"],
                ),
                Update => (
                    json!({"automation_id":id,"expected_revision":revision,"definition":definition_schema()}),
                    vec!["automation_id", "expected_revision", "definition"],
                ),
                _ => (
                    json!({"automation_id":id,"expected_revision":revision}),
                    vec!["automation_id", "expected_revision"],
                ),
            };
            let bridge = self.clone();
            registry.register(ToolDefinition::new(format!("automation_{name}"), description,
                json!({"type":"object","additionalProperties":false,"required":required,"properties":properties}),
                move |args, context| {
                    let bridge = bridge.clone();
                    Box::pin(async move {
                        match bridge.invoke(operation, &context, &args).await {
                            Ok(value) => ToolExecutionResult::success_value(value),
                            Err(error) => ToolExecutionResult::error(match error {
                                    PortError::Unavailable => "unavailable",
                                    PortError::Storage => "storage_error",
                                    PortError::Forbidden => "forbidden",
                                    PortError::NotFound => "not_found",
                                    PortError::InvalidRequest => "invalid_request",
                                    PortError::RequestExpired => "request_expired",
                                    PortError::RevisionConflict => "revision_conflict",
                                }, error.to_string())
                        }
                    })
                }
            ).into());
        }
    }

    async fn invoke(
        &self,
        operation: Operation,
        context: &ToolRunContext,
        args: &ToolCallArgs,
    ) -> Result<Value, PortError> {
        validate_model_arguments(&args.arguments)?;
        let result = match operation.request(args.arguments.clone())? {
            Request::Query(query) => {
                serde_json::to_value(self.query(context, args, query).await?)
            }
            Request::Command(command) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| i64::try_from(d.as_millis()).ok())
                    .ok_or(PortError::Unavailable)?;
                serde_json::to_value(
                    self.command(
                        context,
                        args,
                        command,
                        now,
                        now.saturating_add(REQUEST_TTL_MS),
                    )
                    .await?,
                )
            }
        };
        result.map_err(|_| PortError::Storage)
    }
}

fn definition_schema() -> Value {
    json!({
        "type":"object", "additionalProperties":false, "required":["title","action"],
        "properties":{
            "title":{"type":"string"},
            "action":{"oneOf":[
                {"type":"object","required":["kind","text"],"additionalProperties":false,"properties":{"kind":{"const":"reminder"},"text":{"type":"string"}}},
                {"type":"object","required":["kind","prompt"],"additionalProperties":false,"properties":{"kind":{"const":"agent_task"},"prompt":{"type":"string"}}}
            ]},
            "workspace":{"type":["object","null"],"required":["roots","access"],"properties":{"roots":{"type":"array","items":{"type":"string"}},"access":{"enum":["read","write"]}}},
            "tools":{"type":"array","items":{"type":"string"},"description":"Only explicitly requested tools. Background shell and automation management are unavailable."},
            "model":{"type":["object","null"],"required":["selected","allowed_fallbacks"],"properties":{
                "selected":{"type":"object","required":["provider","model"],"properties":{"provider":{"type":"string"},"model":{"type":"string"}}},
                "allowed_fallbacks":{"type":"array","items":{"type":"object","required":["provider","model"],"properties":{"provider":{"type":"string"},"model":{"type":"string"}}}}
            }},
            "budget":{"type":["object","null"],"required":["max_execution_ms","max_model_calls","max_tool_calls"],"properties":{
                "max_execution_ms":{"type":"integer","minimum":1},"max_model_calls":{"type":"integer","minimum":1},"max_tool_calls":{"type":"integer","minimum":1}
            }},
            "schedule":{"type":["object","null"],"required":["trigger"],"properties":{
                "trigger":{"oneOf":[
                    {"type":"object","required":["kind","scheduled_at_ms"],"properties":{"kind":{"const":"once"},"scheduled_at_ms":{"type":"integer","description":"Unix UTC milliseconds"}}},
                    {"type":"object","required":["kind","time_zone","local_time","frequency"],"properties":{"kind":{"const":"calendar"},"time_zone":{"type":"string","description":"IANA time zone, e.g. Asia/Singapore"},"local_time":{"type":"string","description":"HH:MM:SS"},"frequency":{"oneOf":[{"type":"object","required":["kind"],"properties":{"kind":{"const":"daily"}}},{"type":"object","required":["kind","weekday"],"properties":{"kind":{"const":"weekly"},"weekday":{"enum":["Mon","Tue","Wed","Thu","Fri","Sat","Sun"]}}}]}}}
                ]},
                "misfire":{"enum":["skip","run_once"],"description":"Defaults to skip for reminders and run_once for agent tasks."},"max_lateness_ms":{"type":"integer","minimum":1,"description":"Defaults to one hour for reminders and six hours for agent tasks."}
            }}
        }
    })
}

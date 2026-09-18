//! Host-backed `nomi_delegate` deployment (`16` §7 决策 3).
//!
//! The Store Leader triggers a Team run by calling `nomi_delegate(strategy=planned)`;
//! the host then materializes a durable Agent Execution whose DAG is planned by the
//! server-side Planner. This module owns the **model-facing contract only**: it
//! parses the request and forwards it to a [`HostDelegateSink`] the embedding host
//! provides. The host owns the execution lifecycle, persistence and participant
//! routing.
//!
//! Two deliberate properties:
//!
//! - **The contract is planned-only.** A Store leader expresses "decompose this goal
//!   and run the DAG"; it does not get to pick parallelism, member routing, approval
//!   policy or re-planning, all of which come from the bound Team template and server
//!   policy (`16` §7 决策 3: 不接受模型输入). Hoisted work inside one execution is
//!   expressed by the DAG's independent ready steps, not by a second strategy.
//! - **Unknown fields are refused**, so a model that tries to supply those host-owned
//!   knobs receives an explicit error instead of silently having them ignored.
//!
//! The embedded, synchronous deployment (parallel-only, no durable lifecycle) lives
//! in [`crate::local_delegate_tool`] and is a different host composition: a session
//! registers one or the other, never both.

use std::sync::Arc;

use async_trait::async_trait;
use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::agent::{AgentExecutionReceipt, AgentExecutionStatus};
use nomi_types::tool::{JsonSchema, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};

/// The only strategy a host-backed deployment accepts.
const PLANNED_STRATEGY: &str = "planned";

const DESCRIPTION: &str = concat!(
    "Delegate one goal to the host's Agent Execution: the server-side Planner ",
    "decomposes it into a dependency graph and runs the steps, persisting the ",
    "execution so it survives this turn. Returns an execution_id/status/message ",
    "receipt. End this turn once you have the receipt; the host continues the work ",
    "and reports the consolidated result back into this conversation, so do not poll. ",
    "Parallelism, member routing and approval policy come from the bound team ",
    "template and server policy, not from this request."
);

/// What the host must provide for a session to expose a host-backed delegate.
///
/// One sink is bound to one leader conversation (the same per-conversation shape the
/// cron and meeting sinks use), so the tool itself carries no conversation identity
/// and the model cannot address another conversation.
#[async_trait]
pub trait HostDelegateSink: Send + Sync {
    /// Materialize the host's planned execution for this sink's conversation.
    ///
    /// `Err` is a tool-protocol error (the request was refused), never a fabricated
    /// lifecycle status: a run that starts and later fails is reported by the host
    /// as a receipt, not as an error here.
    async fn plan(&self, goal: &str) -> Result<AgentExecutionReceipt, String>;
}

/// The planned-only request contract.
///
/// `deny_unknown_fields` is load-bearing: `max_parallel`, `plan_gate`,
/// `adaptation_policy`, `work_dir` and `members` are host-owned, and accepting them
/// here would hand the model authority the decision record withholds.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostDelegationRequest {
    /// Must be `"planned"`.
    pub strategy: String,
    /// Complete objective for the host's Planner to decompose.
    pub goal: String,
}

impl HostDelegationRequest {
    /// Validate the request, naming the offending field so the model can correct it.
    pub fn validate(&self) -> Result<(), String> {
        if self.strategy.trim() != PLANNED_STRATEGY {
            return Err(format!(
                "strategy must be \"{PLANNED_STRATEGY}\" for this host (got \"{}\"): the dependency \
                 graph and its parallelism are produced by the host's Planner",
                self.strategy.trim()
            ));
        }
        if self.goal.trim().is_empty() {
            return Err("goal must not be empty".to_owned());
        }
        if self.goal.trim() != self.goal {
            return Err("goal must not have leading or trailing whitespace".to_owned());
        }
        Ok(())
    }
}

fn delegate_json_schema() -> JsonSchema {
    JsonSchema::Object(
        serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "strategy": {
                    "type": "string",
                    "enum": [PLANNED_STRATEGY],
                    "description": "Always \"planned\": the host's Planner builds the dependency graph."
                },
                "goal": {
                    "type": "string",
                    "description": "Complete objective to decompose and execute."
                }
            },
            "required": ["strategy", "goal"],
            "additionalProperties": false
        }))
        .expect("host delegation schema is a JSON object"),
    )
}

/// `nomi_delegate` backed by the embedding host's durable execution engine.
pub struct HostDelegateTool {
    sink: Arc<dyn HostDelegateSink>,
}

impl HostDelegateTool {
    pub fn new(sink: Arc<dyn HostDelegateSink>) -> Self {
        Self { sink }
    }
}

#[async_trait]
impl Tool for HostDelegateTool {
    fn name(&self) -> &str {
        "nomi_delegate"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn input_schema(&self) -> JsonSchema {
        delegate_json_schema()
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        // Starting a collaboration aggregate is not a read-only side effect.
        false
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let request = match parse_request(&input) {
            Ok(request) => request,
            Err(error) => return rejected(error),
        };
        match self.sink.plan(&request.goal).await {
            Ok(receipt) => accepted(receipt),
            Err(error) => rejected(error),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }

    fn describe(&self, input: &Value) -> String {
        let goal = input
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or("delegated goal");
        format!("Delegate a planned execution: {}", nomi_tools::truncate_utf8(goal, 80))
    }
}

fn parse_request(input: &Value) -> Result<HostDelegationRequest, String> {
    let request = serde_json::from_value::<HostDelegationRequest>(input.clone())
        .map_err(|error| format!("invalid nomi_delegate request: {error}"))?;
    request.validate()?;
    Ok(request)
}

/// A started execution is a successful tool call even though the work is ongoing:
/// the receipt carries the lifecycle state, and only a refused request is an error.
fn accepted(receipt: AgentExecutionReceipt) -> ToolResult {
    ToolResult {
        content: serde_json::to_string(&json!({ "result": receipt }))
            .expect("execution receipt is serializable"),
        is_error: false,
        images: Vec::new(),
    }
}

fn rejected(error: String) -> ToolResult {
    ToolResult {
        content: serde_json::to_string(&json!({ "error": error }))
            .expect("delegation rejection is serializable"),
        is_error: true,
        images: Vec::new(),
    }
}

/// Whether a receipt describes an execution that will not progress further on its
/// own. Exposed for hosts that want to report a terminal outcome distinctly.
pub fn receipt_is_terminal(receipt: &AgentExecutionReceipt) -> bool {
    matches!(
        receipt.status,
        AgentExecutionStatus::Completed
            | AgentExecutionStatus::CompletedWithFailures
            | AgentExecutionStatus::Failed
            | AgentExecutionStatus::Cancelled
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;

    struct RecordingSink {
        goals: Mutex<Vec<String>>,
        outcome: Result<AgentExecutionReceipt, String>,
    }

    impl RecordingSink {
        fn accepting() -> Self {
            Self {
                goals: Mutex::new(Vec::new()),
                outcome: Ok(AgentExecutionReceipt::new(
                    "0190f5fe-7c00-7a00-8000-0000000000aa".to_owned(),
                    AgentExecutionStatus::Planning,
                    "Delegated work was accepted.",
                )),
            }
        }

        fn refusing(message: &str) -> Self {
            Self {
                goals: Mutex::new(Vec::new()),
                outcome: Err(message.to_owned()),
            }
        }
    }

    #[async_trait]
    impl HostDelegateSink for RecordingSink {
        async fn plan(&self, goal: &str) -> Result<AgentExecutionReceipt, String> {
            self.goals.lock().await.push(goal.to_owned());
            self.outcome.clone()
        }
    }

    fn tool(sink: Arc<RecordingSink>) -> HostDelegateTool {
        HostDelegateTool::new(sink)
    }

    #[test]
    fn accepted_request_is_planned_only_and_needs_a_goal() {
        assert!(
            parse_request(&json!({"strategy": "planned", "goal": "ship it"})).is_ok()
        );
        for bad in [
            json!({"strategy": "parallel", "goal": "ship it"}),
            json!({"strategy": "planned", "goal": "   "}),
            json!({"strategy": "planned", "goal": " padded "}),
            json!({"strategy": "planned"}),
        ] {
            assert!(parse_request(&bad).is_err(), "{bad} must be refused");
        }
    }

    /// The knobs the decision record reserves for the host must be refused loudly
    /// rather than silently dropped.
    #[test]
    fn host_owned_knobs_are_refused_instead_of_ignored() {
        for field in ["max_parallel", "plan_gate", "adaptation_policy", "work_dir", "members"] {
            let payload = json!({"strategy": "planned", "goal": "g", field: 1});
            let error = parse_request(&payload).expect_err("must refuse");
            assert!(error.contains(field), "error must name `{field}`: {error}");
        }
    }

    #[tokio::test]
    async fn execute_forwards_the_goal_and_returns_the_receipt() {
        let sink = Arc::new(RecordingSink::accepting());
        let result = tool(sink.clone())
            .execute(json!({"strategy": "planned", "goal": "ship it"}))
            .await;

        assert!(!result.is_error, "a started execution is not a tool error");
        let payload: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(payload["result"]["status"], "planning");
        assert_eq!(
            payload["result"]["execution_id"],
            "0190f5fe-7c00-7a00-8000-0000000000aa"
        );
        assert_eq!(sink.goals.lock().await.as_slice(), &["ship it".to_owned()]);
    }

    #[tokio::test]
    async fn a_refused_request_is_a_tool_error_with_no_fake_lifecycle() {
        let sink = Arc::new(RecordingSink::refusing("delegation is disabled"));
        let result = tool(sink).execute(json!({"strategy": "planned", "goal": "g"})).await;

        assert!(result.is_error);
        let payload: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(payload["error"], "delegation is disabled");
        assert!(payload.get("result").is_none());
    }

    #[tokio::test]
    async fn a_malformed_request_never_reaches_the_sink() {
        let sink = Arc::new(RecordingSink::accepting());
        let result = tool(sink.clone())
            .execute(json!({"strategy": "parallel", "goal": "g"}))
            .await;

        assert!(result.is_error);
        assert!(
            sink.goals.lock().await.is_empty(),
            "the host must not be asked to start anything for a refused request"
        );
    }

    #[test]
    fn schema_advertises_exactly_one_strategy_and_no_host_knobs() {
        let JsonSchema::Object(schema) = delegate_json_schema() else {
            panic!("host delegation schema must be an object");
        };
        assert_eq!(schema["properties"]["strategy"]["enum"], json!([PLANNED_STRATEGY]));
        assert_eq!(schema["additionalProperties"], json!(false));
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(
            properties.keys().collect::<Vec<_>>(),
            vec!["goal", "strategy"],
            "only the two model-facing fields may be advertised"
        );
    }

    #[test]
    fn terminal_detection_covers_every_terminal_status() {
        for status in [
            AgentExecutionStatus::Completed,
            AgentExecutionStatus::CompletedWithFailures,
            AgentExecutionStatus::Failed,
            AgentExecutionStatus::Cancelled,
        ] {
            let receipt =
                AgentExecutionReceipt::new("id".to_owned(), status, "message");
            assert!(receipt_is_terminal(&receipt), "{status:?} is terminal");
        }
        for status in [
            AgentExecutionStatus::Planning,
            AgentExecutionStatus::Running,
            AgentExecutionStatus::Paused,
        ] {
            let receipt =
                AgentExecutionReceipt::new("id".to_owned(), status, "message");
            assert!(!receipt_is_terminal(&receipt), "{status:?} is not terminal");
        }
    }
}

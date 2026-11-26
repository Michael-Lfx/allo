//! Scoped tool registry with a policy hook and monotonic guards.
//!
//! Replaces the previous kernel waterfall pipeline: the host supplies one
//! explicit policy callback plus any number of guards. A denial can never be
//! turned back into permission by listener ordering.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::types::{
    ToolCallArgs, ToolExecutionResult, ToolRunContext, ToolSchema,
};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// One registered tool: schema plus the execution body.
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object for the arguments.
    pub parameters: Value,
    pub execute: Arc<
        dyn Fn(ToolCallArgs, ToolRunContext) -> BoxFuture<ToolExecutionResult>
            + Send
            + Sync,
    >,
    pub is_concurrency_safe: bool,
    artifact_delivery: bool,
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("concurrency_safe", &self.is_concurrency_safe)
            .field("artifact_delivery", &self.artifact_delivery)
            .finish_non_exhaustive()
    }
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
        execute: impl Fn(
            ToolCallArgs,
            ToolRunContext,
        ) -> BoxFuture<ToolExecutionResult>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
            execute: Arc::new(execute),
            is_concurrency_safe: false,
            artifact_delivery: false,
        }
    }

    /// Mark a host-registered tool as the dedicated artifact publisher.
    /// `RegistryTool` still requires the canonical `publish_file` name and
    /// validates every receipt against its injected turn context.
    #[must_use]
    pub const fn with_artifact_delivery(mut self) -> Self {
        self.artifact_delivery = true;
        self
    }

    #[must_use]
    pub const fn supports_artifact_delivery(&self) -> bool {
        self.artifact_delivery
    }

    #[must_use]
    pub fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
        }
    }
}

/// A monotonic guard: `Some(reason)` denies the call.
pub type ToolGuard = Arc<dyn Fn(&ToolCallArgs) -> Option<String> + Send + Sync>;

/// Host context guard supplements the existing model-argument policy. A
/// denial is monotonic and is evaluated immediately before body dispatch.
pub type ToolContextGuard =
    Arc<dyn Fn(&ToolRunContext, &ToolCallArgs) -> Option<String> + Send + Sync>;

/// The host's allow/deny decision for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPolicyDecision {
    Allow,
    Deny(String),
    /// The host wants an interactive approval that this layer cannot provide.
    ApprovalRequired,
}

/// Host policy hook. `None` means "no policy configured: allow".
pub type ToolPolicy =
    Arc<dyn Fn(&ToolCallArgs) -> ToolPolicyDecision + Send + Sync>;

struct Inner {
    tools: Mutex<HashMap<String, Arc<ToolDefinition>>>,
    guards: Mutex<Vec<ToolGuard>>,
    context_guards: Mutex<Vec<ToolContextGuard>>,
    policy: Mutex<Option<ToolPolicy>>,
}

/// Scoped tool registry. Cheap to clone.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    inner: Arc<Inner>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            tools: Mutex::new(HashMap::new()),
            guards: Mutex::new(Vec::new()),
            context_guards: Mutex::new(Vec::new()),
            policy: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.list())
            .finish_non_exhaustive()
    }
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one tool; a duplicate name is replaced (last wins).
    pub fn register(&self, tool: Arc<ToolDefinition>) {
        self.inner
            .tools
            .lock()
            .unwrap()
            .insert(tool.name.clone(), tool);
    }

    pub fn unregister(&self, name: &str) {
        self.inner.tools.lock().unwrap().remove(name);
    }

    /// Install the single host policy hook.
    pub fn set_policy(&self, policy: ToolPolicy) {
        *self.inner.policy.lock().unwrap() = Some(policy);
    }

    /// Add a monotonic guard evaluated before every dispatch.
    pub fn add_guard(&self, guard: ToolGuard) {
        self.inner.guards.lock().unwrap().push(guard);
    }

    pub fn add_context_guard(&self, guard: ToolContextGuard) {
        self.inner.context_guards.lock().unwrap().push(guard);
    }

    /// Model-facing schemas of every registered tool.
    #[must_use]
    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.inner
            .tools
            .lock()
            .unwrap()
            .values()
            .map(|tool| tool.schema())
            .collect()
    }

    #[must_use]
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.inner.tools.lock().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<ToolDefinition>> {
        self.inner.tools.lock().unwrap().get(name).cloned()
    }

    /// Run one accepted call through policy, guards, and the tool body.
    pub async fn execute(
        &self,
        call_id: String,
        name: String,
        arguments: Value,
        run_ctx: ToolRunContext,
    ) -> ToolExecutionResult {
        self.execute_under_gate(call_id, name, arguments, run_ctx, None)
            .await
    }

    pub(crate) async fn execute_under_gate(
        &self,
        call_id: String,
        name: String,
        arguments: Value,
        run_ctx: ToolRunContext,
        gate: Option<&dyn crate::ExecutionGate>,
    ) -> ToolExecutionResult {
        self.execute_with_limits(call_id, name, arguments, run_ctx, gate, None)
            .await
    }

    pub(crate) async fn execute_with_limits(
        &self,
        call_id: String,
        name: String,
        arguments: Value,
        run_ctx: ToolRunContext,
        gate: Option<&dyn crate::ExecutionGate>,
        budget: Option<&crate::run_budget::RunBudgetState>,
    ) -> ToolExecutionResult {
        let args = ToolCallArgs {
            call_id,
            name: name.clone(),
            arguments,
        };
        let Some(tool) = self.get(&name) else {
            return ToolExecutionResult::error(
                "UNKNOWN_TOOL",
                format!("no tool registered with name \"{name}\""),
            );
        };

        let decision = {
            let policy = self.inner.policy.lock().unwrap().clone();
            policy.map(|policy| policy(&args))
        };
        match decision {
            None | Some(ToolPolicyDecision::Allow) => {}
            Some(ToolPolicyDecision::Deny(reason)) => {
                return ToolExecutionResult::error("DENIED", reason);
            }
            Some(ToolPolicyDecision::ApprovalRequired) => {
                return ToolExecutionResult::error(
                    "APPROVAL",
                    "tool call requires approval",
                );
            }
        }

        let guards = self.inner.guards.lock().unwrap().clone();
        for guard in &guards {
            if let Some(reason) = guard(&args) {
                return ToolExecutionResult::error("DENIED", reason);
            }
        }

        if run_ctx.signal.is_cancelled() {
            return ToolExecutionResult::error(
                "CANCELLED",
                "tool call cancelled before start",
            );
        }

        if let Some(budget) = budget
            && let Err(error) = budget.reserve_tool()
        {
            return ToolExecutionResult::error("RUN_BUDGET", error.to_string());
        }
        // A budget checkpoint can involve slow storage. Recheck cancellation
        // and resolve current authority after it, immediately before dispatch.
        if run_ctx.signal.is_cancelled() {
            return ToolExecutionResult::error(
                "CANCELLED",
                "tool call cancelled before dispatch",
            );
        }
        if let Some(gate) = gate
            && let Err(error) = gate.authorize_tool(&run_ctx, &args)
        {
            return ToolExecutionResult::error("DENIED", error.to_string());
        }
        // Authorization may wait on host-owned locks or durable ledger work.
        // Recheck both stop conditions after it so a slow gate cannot dispatch
        // a tool after cancellation or the monotonic deadline.
        if let Some(budget) = budget
            && let Some(stop) = budget.stop_requested()
        {
            return ToolExecutionResult::error("RUN_BUDGET", stop.to_string());
        }
        if run_ctx.signal.is_cancelled() {
            return ToolExecutionResult::error(
                "CANCELLED",
                "tool call cancelled before dispatch",
            );
        }

        let context_guards = self.inner.context_guards.lock().unwrap().clone();
        for guard in &context_guards {
            if let Some(reason) = guard(&run_ctx, &args) {
                return ToolExecutionResult::error("DENIED", reason);
            }
        }
        if run_ctx.signal.is_cancelled() {
            return ToolExecutionResult::error(
                "CANCELLED",
                "tool call cancelled after context authorization",
            );
        }
        if let Some(budget) = budget
            && let Some(stop) = budget.stop_requested()
        {
            return ToolExecutionResult::error("RUN_BUDGET", stop.to_string());
        }
        (tool.execute)(args, run_ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolDefinition, ToolPolicyDecision, ToolRegistry};
    use crate::types::{
        ContentBlock, ToolCallArgs, ToolExecutionResult, ToolRunContext,
    };
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct DispatchGate<F>(F);

    impl<F> crate::ExecutionGate for DispatchGate<F>
    where
        F: Fn(&ToolRunContext) + Send + Sync,
    {
        fn model_credential(
            &self,
            _: &crate::ExecutionMetadata,
            _: &crate::ModelRoute,
        ) -> Result<String, crate::ExecutionDenied> {
            Ok("fixture".into())
        }

        fn authorize_tool(
            &self,
            context: &ToolRunContext,
            _: &ToolCallArgs,
        ) -> Result<(), crate::ExecutionDenied> {
            (self.0)(context);
            Ok(())
        }
    }

    fn echo_tool(executed: Arc<AtomicBool>) -> Arc<ToolDefinition> {
        Arc::new(ToolDefinition::new(
            "echo",
            "echo text",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            move |args: ToolCallArgs, _ctx: ToolRunContext| {
                let executed = executed.clone();
                Box::pin(async move {
                    executed.store(true, Ordering::SeqCst);
                    let text = args.arguments["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    ToolExecutionResult::success_text(text, args.arguments)
                })
            },
        ))
    }

    fn ctx() -> ToolRunContext {
        ToolRunContext {
            execution: crate::session::ExecutionMetadata::default(),
            signal: crate::types::CancelToken::new(),
            agent_id: Some("agent".into()),
            cwd: None,
            todos: None,
            submission_id: None,
            session_origin: crate::session::SessionOrigin::Unknown,
            output_root: None,
            artifact_delivery_available: false,
        }
    }

    #[tokio::test]
    async fn policy_denial_never_reaches_the_tool_body() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(executed.clone()));
        registry.set_policy(Arc::new(|args| {
            if args.name == "echo" {
                ToolPolicyDecision::Deny("blocked by host".into())
            } else {
                ToolPolicyDecision::Allow
            }
        }));
        let result = registry
            .execute("call-1".into(), "echo".into(), json!({"text":"x"}), ctx())
            .await;
        assert!(result.is_error());
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn guard_denial_never_reaches_the_tool_body() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(executed.clone()));
        registry.add_guard(Arc::new(|args| {
            (args.arguments["text"] == "secret").then(|| "blocked".to_owned())
        }));
        let result = registry
            .execute(
                "call-2".into(),
                "echo".into(),
                json!({"text":"secret"}),
                ctx(),
            )
            .await;
        assert!(result.is_error());
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancelled_context_skips_execution_and_unknown_tool_is_explicit() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(executed.clone()));
        let cancelled = ctx();
        cancelled.signal.cancel();
        let result = registry
            .execute(
                "call-3".into(),
                "echo".into(),
                json!({"text":"x"}),
                cancelled,
            )
            .await;
        assert!(result.is_error());
        assert!(!executed.load(Ordering::SeqCst));

        let missing = registry
            .execute("call-4".into(), "missing".into(), json!({}), ctx())
            .await;
        match missing {
            ToolExecutionResult::Error { code, .. } => {
                assert_eq!(code, "UNKNOWN_TOOL");
            }
            ToolExecutionResult::Success { .. } => {
                panic!("unknown tool must not succeed")
            }
        }
    }

    #[tokio::test]
    async fn revoked_authority_is_rechecked_before_the_next_tool_dispatch() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(Arc::clone(&executed)));
        let gate =
            crate::execution_gate::tests::RevocableGate(AtomicBool::new(true));
        assert!(
            !registry
                .execute_under_gate(
                    "call-6".into(),
                    "echo".into(),
                    json!({"text":"first"}),
                    ctx(),
                    Some(&gate),
                )
                .await
                .is_error()
        );
        assert!(executed.swap(false, Ordering::SeqCst));

        gate.0.store(false, Ordering::SeqCst);
        let result = registry
            .execute_under_gate(
                "call-7".into(),
                "echo".into(),
                json!({"text":"after revoke"}),
                ctx(),
                Some(&gate),
            )
            .await;
        assert!(matches!(
            result,
            ToolExecutionResult::Error { code, .. } if code == "DENIED"
        ));
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancellation_during_authorization_skips_tool_dispatch() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(Arc::clone(&executed)));
        let gate = DispatchGate(|context: &ToolRunContext| {
            context.signal.cancel();
        });
        let result = registry
            .execute_under_gate(
                "cancel-during-gate".into(),
                "echo".into(),
                json!({"text":"must not execute"}),
                ctx(),
                Some(&gate),
            )
            .await;
        assert!(matches!(
            result,
            ToolExecutionResult::Error { code, .. } if code == "CANCELLED"
        ));
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn deadline_during_authorization_skips_tool_dispatch() {
        use std::sync::Mutex;
        use std::time::Duration;
        use tokio::time::Instant;

        let executed = Arc::new(AtomicBool::new(false));
        let authorized = AtomicBool::new(false);
        let registry = ToolRegistry::new();
        registry.register(echo_tool(Arc::clone(&executed)));
        let limits = crate::RunBudget::new(1_000, 1, 1).unwrap();
        let start =
            crate::run_budget::RunBudgetStart::new(limits, 1_000).unwrap();
        let past_deadline = Instant::now() + Duration::from_secs(1);
        let checkpoints = Arc::new(Mutex::new(Vec::new()));
        let saved = Arc::clone(&checkpoints);
        let budget = start.track(move |checkpoint| {
            saved.lock().unwrap().push(checkpoint);
            Ok(())
        });
        let gate = DispatchGate(|_: &ToolRunContext| {
            authorized.store(true, Ordering::SeqCst);
            // Wait through the known monotonic boundary while the synchronous
            // gate prevents the outer supervisor from being polled.
            while let Some(remaining) =
                past_deadline.checked_duration_since(Instant::now())
            {
                std::thread::sleep(remaining);
            }
        });
        let result = registry
            .execute_with_limits(
                "deadline-during-gate".into(),
                "echo".into(),
                json!({"text":"must not execute"}),
                ctx(),
                Some(&gate),
                Some(&budget),
            )
            .await;
        assert!(authorized.load(Ordering::SeqCst));
        assert!(matches!(
            result,
            ToolExecutionResult::Error { code, .. } if code == "RUN_BUDGET"
        ));
        assert!(!executed.load(Ordering::SeqCst));
        let checkpoints = checkpoints.lock().unwrap();
        assert_eq!(checkpoints.len(), 2);
        assert_eq!(checkpoints[0].tool_calls, 1);
        assert_eq!(checkpoints[1].tool_calls, 1);
        assert_eq!(
            checkpoints[1].stop_reason,
            Some(crate::BudgetStopReason::Deadline)
        );
    }

    #[tokio::test]
    async fn context_guards_receive_host_identity_and_cannot_bypass_policy() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(executed.clone()));
        registry.add_context_guard(Arc::new(|context, args| {
            let Some(call) = context.trusted_call(args) else {
                return Some("missing host identity".into());
            };
            (call.session_id != "agent"
                || call.submission_id != "host-submission")
                .then(|| "wrong host identity".into())
        }));
        let mut context = ctx();
        context.submission_id = Some("host-submission".into());
        registry.set_policy(Arc::new(|_| {
            ToolPolicyDecision::Deny("original policy".into())
        }));
        assert!(
            registry
                .execute(
                    "call-1".into(),
                    "echo".into(),
                    json!({}),
                    context.clone()
                )
                .await
                .is_error()
        );
        assert!(!executed.load(Ordering::SeqCst));
        registry.set_policy(Arc::new(|_| ToolPolicyDecision::Allow));
        context.agent_id = Some("forged".into());
        assert!(
            registry
                .execute(
                    "call-1".into(),
                    "echo".into(),
                    json!({"session_id":"agent"}),
                    context
                )
                .await
                .is_error()
        );
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn context_guard_cancellation_is_rechecked_before_body() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(executed.clone()));
        registry.add_context_guard(Arc::new(|context, _| {
            context.signal.cancel();
            None
        }));
        assert!(
            registry
                .execute("call-1".into(), "echo".into(), json!({}), ctx())
                .await
                .is_error()
        );
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn allowed_call_returns_model_facing_content() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(executed.clone()));
        let result = registry
            .execute(
                "call-5".into(),
                "echo".into(),
                json!({"text":"hello"}),
                ctx(),
            )
            .await;
        assert_eq!(
            result.content(),
            vec![ContentBlock::text("hello")],
            "result content must reach the model unchanged"
        );
        assert!(executed.load(Ordering::SeqCst));
    }
}

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

use crate::types::{ToolCallArgs, ToolExecutionResult, ToolRunContext, ToolSchema};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// One registered tool: schema plus the execution body.
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object for the arguments.
    pub parameters: Value,
    pub execute:
        Arc<dyn Fn(ToolCallArgs, ToolRunContext) -> BoxFuture<ToolExecutionResult> + Send + Sync>,
    pub is_concurrency_safe: bool,
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("concurrency_safe", &self.is_concurrency_safe)
            .finish_non_exhaustive()
    }
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
        execute: impl Fn(ToolCallArgs, ToolRunContext) -> BoxFuture<ToolExecutionResult>
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
        }
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

/// The host's allow/deny decision for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPolicyDecision {
    Allow,
    Deny(String),
    /// The host wants an interactive approval that this layer cannot provide.
    ApprovalRequired,
}

/// Host policy hook. `None` means "no policy configured: allow".
pub type ToolPolicy = Arc<dyn Fn(&ToolCallArgs) -> ToolPolicyDecision + Send + Sync>;

struct Inner {
    tools: Mutex<HashMap<String, Arc<ToolDefinition>>>,
    guards: Mutex<Vec<ToolGuard>>,
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
        let mut names: Vec<String> = self.inner.tools.lock().unwrap().keys().cloned().collect();
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
                return ToolExecutionResult::error("APPROVAL", "tool call requires approval");
            }
        }

        let guards = self.inner.guards.lock().unwrap().clone();
        for guard in &guards {
            if let Some(reason) = guard(&args) {
                return ToolExecutionResult::error("DENIED", reason);
            }
        }

        if run_ctx.signal.is_cancelled() {
            return ToolExecutionResult::error("CANCELLED", "tool call cancelled before start");
        }

        (tool.execute)(args, run_ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ToolDefinition, ToolPolicyDecision, ToolRegistry,
    };
    use crate::types::{ContentBlock, ToolCallArgs, ToolExecutionResult, ToolRunContext};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn echo_tool(executed: Arc<AtomicBool>) -> Arc<ToolDefinition> {
        Arc::new(ToolDefinition::new(
            "echo",
            "echo text",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            move |args: ToolCallArgs, _ctx: ToolRunContext| {
                let executed = executed.clone();
                Box::pin(async move {
                    executed.store(true, Ordering::SeqCst);
                    let text = args.arguments["text"].as_str().unwrap_or_default().to_owned();
                    ToolExecutionResult::success_text(text, args.arguments)
                })
            },
        ))
    }

    fn ctx() -> ToolRunContext {
        ToolRunContext {
            signal: crate::types::CancelToken::new(),
            agent_id: Some("agent".into()),
            cwd: None,
            todos: None,
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
            .execute("call-2".into(), "echo".into(), json!({"text":"secret"}), ctx())
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
            .execute("call-3".into(), "echo".into(), json!({"text":"x"}), cancelled)
            .await;
        assert!(result.is_error());
        assert!(!executed.load(Ordering::SeqCst));

        let missing = registry
            .execute("call-4".into(), "missing".into(), json!({}), ctx())
            .await;
        match missing {
            ToolExecutionResult::Error { code, .. } => assert_eq!(code, "UNKNOWN_TOOL"),
            ToolExecutionResult::Success { .. } => panic!("unknown tool must not succeed"),
        }
    }

    #[tokio::test]
    async fn allowed_call_returns_model_facing_content() {
        let executed = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(executed.clone()));
        let result = registry
            .execute("call-5".into(), "echo".into(), json!({"text":"hello"}), ctx())
            .await;
        assert_eq!(
            result.content(),
            vec![ContentBlock::text("hello")],
            "result content must reach the model unchanged"
        );
        assert!(executed.load(Ordering::SeqCst));
    }
}

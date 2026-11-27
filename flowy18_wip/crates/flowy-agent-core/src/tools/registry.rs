//! Scoped tool registry with a policy hook and monotonic guards.
//!
//! Replaces the previous kernel waterfall pipeline: the host supplies one
//! explicit policy callback plus any number of guards. A denial can never be
//! turned back into permission by listener ordering.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::types::{
    ToolCallArgs, ToolExecutionResult, ToolRunContext, ToolSchema,
};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Origin classification for a registered [`ToolDefinition`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ToolSource {
    #[default]
    Builtin,
    Host,
    Provider(String),
}

impl ToolSource {
    /// Human-readable origin label for tool search catalogs and telemetry.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Builtin => "builtin",
            Self::Host => "host",
            Self::Provider(id) => id.as_str(),
        }
    }
}

/// Controls whether a tool schema is injected into the model's initial tool
/// list (`Eager`) or discovered on demand via `tool_search` (`Deferred`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolExposure {
    #[default]
    Eager,
    Deferred,
}

/// One registered tool: schema plus the execution body.
#[derive(Clone)]
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
    read_only: bool,
    exposure: ToolExposure,
    source: ToolSource,
    tags: Vec<String>,
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("concurrency_safe", &self.is_concurrency_safe)
            .field("artifact_delivery", &self.artifact_delivery)
            .field("read_only", &self.read_only)
            .field("exposure", &self.exposure)
            .field("source", &self.source)
            .field("tags", &self.tags)
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
            read_only: false,
            exposure: ToolExposure::Eager,
            source: ToolSource::Builtin,
            tags: Vec::new(),
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

    /// Strip `artifact_delivery` capability (used when mounting untrusted or
    /// extension provider tools).
    #[must_use]
    pub const fn without_artifact_delivery(mut self) -> Self {
        self.artifact_delivery = false;
        self
    }

    #[must_use]
    pub const fn supports_artifact_delivery(&self) -> bool {
        self.artifact_delivery
    }

    /// Declare whether this tool performs only side-effect-free inspection.
    #[must_use]
    pub const fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Control whether this tool is eagerly sent in initial schemas or
    /// discovered on demand via `tool_search`.
    #[must_use]
    pub const fn with_exposure(mut self, exposure: ToolExposure) -> Self {
        self.exposure = exposure;
        self
    }

    #[must_use]
    pub const fn exposure(&self) -> ToolExposure {
        self.exposure
    }

    /// Record the origin (`Builtin`, `Host`, or `Provider(id)`) of this tool.
    #[must_use]
    pub fn with_source(mut self, source: ToolSource) -> Self {
        self.source = source;
        self
    }

    #[must_use]
    pub fn source(&self) -> &ToolSource {
        &self.source
    }

    /// Attach searchable keyword tags for `tool_search` discovery.
    #[must_use]
    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tags.clear();
        for tag in tags {
            let trimmed = tag.into().trim().to_owned();
            if !trimmed.is_empty()
                && !self
                    .tags
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&trimmed))
            {
                self.tags.push(trimmed);
            }
        }
        self
    }

    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
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
pub type ToolPolicy =
    Arc<dyn Fn(&ToolCallArgs) -> ToolPolicyDecision + Send + Sync>;

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

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one tool; a duplicate name is replaced (last wins).
    pub fn register(&self, tool: Arc<ToolDefinition>) {
        lock(&self.inner.tools).insert(tool.name.clone(), tool);
    }

    pub fn unregister(&self, name: &str) {
        lock(&self.inner.tools).remove(name);
    }

    /// Install the single host policy hook.
    pub fn set_policy(&self, policy: ToolPolicy) {
        *lock(&self.inner.policy) = Some(policy);
    }

    /// Add a monotonic guard evaluated before every dispatch.
    pub fn add_guard(&self, guard: ToolGuard) {
        lock(&self.inner.guards).push(guard);
    }

    /// Registered tool definitions in deterministic ascending name order.
    #[must_use]
    pub fn definitions(&self) -> Vec<Arc<ToolDefinition>> {
        let mut defs: Vec<Arc<ToolDefinition>> =
            lock(&self.inner.tools).values().cloned().collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    /// Model-facing schemas of every registered tool in deterministic order.
    #[must_use]
    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.definitions()
            .into_iter()
            .map(|tool| tool.schema())
            .collect()
    }

    #[must_use]
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> =
            lock(&self.inner.tools).keys().cloned().collect();
        names.sort();
        names
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<ToolDefinition>> {
        lock(&self.inner.tools).get(name).cloned()
    }

    /// True if a registered tool declares side-effect-free read-only semantics.
    #[must_use]
    pub fn is_read_only(&self, name: &str) -> bool {
        self.get(name).is_some_and(|tool| tool.is_read_only())
    }

    /// True if at least one registered tool uses [`ToolExposure::Deferred`].
    #[must_use]
    pub fn has_deferred(&self) -> bool {
        lock(&self.inner.tools)
            .values()
            .any(|tool| tool.exposure() == ToolExposure::Deferred)
    }

    /// Registered [`ToolExposure::Deferred`] tools in ascending name order.
    #[must_use]
    pub fn deferred_definitions(&self) -> Vec<Arc<ToolDefinition>> {
        let mut defs: Vec<Arc<ToolDefinition>> = lock(&self.inner.tools)
            .values()
            .filter(|tool| tool.exposure() == ToolExposure::Deferred)
            .cloned()
            .collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    /// Set of registered tool names marked `read_only`.
    #[must_use]
    pub fn read_only_tool_names(&self) -> HashSet<String> {
        lock(&self.inner.tools)
            .iter()
            .filter(|(_, tool)| tool.is_read_only())
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Create an independent shallow copy (isolated mutexes for tools, guards,
    /// and policy) so per-turn additions like `tool_search` never mutate a
    /// shared base registry.
    #[must_use]
    pub fn fork(&self) -> Self {
        let tools = lock(&self.inner.tools).clone();
        let guards = lock(&self.inner.guards).clone();
        let policy = lock(&self.inner.policy).clone();
        Self {
            inner: Arc::new(Inner {
                tools: Mutex::new(tools),
                guards: Mutex::new(guards),
                policy: Mutex::new(policy),
            }),
        }
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
            let policy = lock(&self.inner.policy).clone();
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

        let guards = lock(&self.inner.guards).clone();
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

    #[test]
    fn schemas_are_sorted_deterministically_and_fork_isolates_mutations() {
        let registry = ToolRegistry::new();
        for name in ["zeta", "alpha", "middle"] {
            registry.register(Arc::new(ToolDefinition::new(
                name,
                format!("tool {name}"),
                json!({"type": "object"}),
                |_args, _ctx| {
                    Box::pin(async {
                        ToolExecutionResult::success_text("ok", json!({}))
                    })
                },
            )));
        }
        let schema_names: Vec<String> =
            registry.schemas().into_iter().map(|s| s.name).collect();
        assert_eq!(schema_names, vec!["alpha", "middle", "zeta"]);

        let forked = registry.fork();
        forked.unregister("alpha");
        forked.register(Arc::new(ToolDefinition::new(
            "extra",
            "extra",
            json!({"type": "object"}),
            |_args, _ctx| {
                Box::pin(async {
                    ToolExecutionResult::success_text("ok", json!({}))
                })
            },
        )));

        assert_eq!(registry.list(), vec!["alpha", "middle", "zeta"]);
        assert_eq!(forked.list(), vec!["extra", "middle", "zeta"]);
    }
}

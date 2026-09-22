//! Plan mode tools: `EnterPlanMode` and `ExitPlanMode`.
//!
//! Both tools read the plan service's shared flags to validate a transition,
//! and both report the transition to the engine as a `ContextModifier` — the
//! engine never inspects the tool name.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_tools::registry::ToolRegistry;
use nomi_types::skill_types::{ContextModifier, PlanModeTransition};
use nomi_types::tool::{JsonSchema, ToolResult};

use super::PlanService;

// ---------------------------------------------------------------------------
// EnterPlanModeTool
// ---------------------------------------------------------------------------

/// Transitions the agent into Plan Mode.
///
/// While in plan mode the engine restricts the available tool set to
/// read-only (`Info`-category) tools so the LLM can focus on understanding
/// the codebase and composing an implementation plan.
pub struct EnterPlanModeTool {
    /// Shared flag indicating whether plan mode is currently active.
    /// Read by `execute()` to prevent double-entry.
    plan_active: Arc<AtomicBool>,
}

impl EnterPlanModeTool {
    pub fn new(plan_active: Arc<AtomicBool>) -> Self {
        Self { plan_active }
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn description(&self) -> &str {
        "Enter plan mode to focus on reading code and creating an implementation plan. \
         While in plan mode, only read-only tools are available. \
         Use ExitPlanMode when your plan is ready."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(&self, _input: Value) -> ToolResult {
        if self.plan_active.load(Ordering::Acquire) {
            return ToolResult {
                content: "Already in plan mode. Use ExitPlanMode to exit first.".to_string(),
                is_error: true,
                images: Vec::new(),
            };
        }

        ToolResult {
            content: "Entered plan mode. You can now only use read-only tools to explore \
                      the codebase and create your implementation plan. When your plan is \
                      ready, use ExitPlanMode to exit plan mode and begin implementation."
                .to_string(),
            is_error: false,
            images: Vec::new(),
        }
    }

    fn context_modifier_for(&self, _input: &Value) -> Option<ContextModifier> {
        Some(ContextModifier {
            plan_mode_transition: Some(PlanModeTransition::Enter),
            ..Default::default()
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, _input: &Value) -> String {
        "Enter plan mode".to_string()
    }
}

// ---------------------------------------------------------------------------
// ExitPlanModeTool
// ---------------------------------------------------------------------------

/// Minimum plan body length so a one-liner cannot skip the verification gate.
const MIN_PLAN_CHARS: usize = 40;

fn plan_has_verification(plan: &str) -> bool {
    let lower = plan.to_ascii_lowercase();
    nomi_coding::looks_like_verification_command(plan)
        || lower.contains("verif")
        || lower.contains("how to test")
}

fn plan_is_ready(plan: &str) -> bool {
    plan.trim().chars().count() >= MIN_PLAN_CHARS && plan_has_verification(plan)
}

/// Submits a verifiable implementation plan. Write tools stay locked until
/// the next user message (the Build-click analogue).
pub struct ExitPlanModeTool {
    /// Shared flag indicating whether plan mode is currently active.
    plan_active: Arc<AtomicBool>,
    /// Set by the engine once a valid plan is latched for approval.
    exit_latched: Arc<AtomicBool>,
}

impl ExitPlanModeTool {
    pub fn new(plan_active: Arc<AtomicBool>) -> Self {
        Self::with_latch(plan_active, Arc::new(AtomicBool::new(false)))
    }

    pub fn with_latch(plan_active: Arc<AtomicBool>, exit_latched: Arc<AtomicBool>) -> Self {
        Self {
            plan_active,
            exit_latched,
        }
    }
}

fn exit_plan_text(input: &Value) -> Option<&str> {
    input
        .get("plan")
        .or_else(|| input.get("plan_content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Submit a complete implementation plan for user approval. \
         Include Context, Files to modify, and a concrete Verification command \
         (for example `cargo test` or `bun run check`). Write tools stay locked \
         until the user sends the next message."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "Required implementation plan: goal, scope, and a Verification command the user can run"
                },
                "plan_content": {
                    "type": "string",
                    "description": "Alias for plan"
                }
            },
            "required": []
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        if !self.plan_active.load(Ordering::Acquire) {
            return ToolResult {
                content: "Not in plan mode. Use EnterPlanMode to enter plan mode first."
                    .to_string(),
                is_error: true,
                images: Vec::new(),
            };
        }
        if self.exit_latched.load(Ordering::Acquire) {
            return ToolResult {
                content: "A plan is already submitted and waiting for the user to continue. \
                          Do not call ExitPlanMode again."
                    .to_string(),
                is_error: true,
                images: Vec::new(),
            };
        }

        let Some(plan) = exit_plan_text(&input) else {
            return ToolResult {
                content: "ExitPlanMode requires a non-empty `plan` (goal, scope, and a \
                          Verification command such as `cargo test` or `bun run check`)."
                    .to_string(),
                is_error: true,
                images: Vec::new(),
            };
        };
        if !plan_is_ready(plan) {
            return ToolResult {
                content: "ExitPlanMode rejected: the plan is too short or has no Verification \
                          command. Add a concrete check (for example `cargo test`, `bun run check`, \
                          or a 'How to test' section) and call ExitPlanMode again."
                    .to_string(),
                is_error: true,
                images: Vec::new(),
            };
        }

        let mut content = String::from(
            "Plan submitted for approval. Stay in read-only mode until the user \
             sends the next message. Do not start implementation in this turn.",
        );
        content.push_str("\n\n");
        content.push_str(plan);

        ToolResult {
            content,
            is_error: false,
            images: Vec::new(),
        }
    }

    fn context_modifier_for(&self, input: &Value) -> Option<ContextModifier> {
        if !self.plan_active.load(Ordering::Acquire) {
            return None;
        }
        if self.exit_latched.load(Ordering::Acquire) {
            return None;
        }
        let plan = exit_plan_text(input)?;
        if !plan_is_ready(plan) {
            return None;
        }
        Some(ContextModifier {
            plan_mode_transition: Some(PlanModeTransition::Exit {
                plan_content: Some(plan.to_string()),
            }),
            ..Default::default()
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, _input: &Value) -> String {
        "Exit plan mode".to_string()
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub const PLAN_TOOL_ENTER: &str = "EnterPlanMode";
pub const PLAN_TOOL_EXIT: &str = "ExitPlanMode";

/// Register both plan tools against `service`'s shared flags.
///
/// Returns whether tools were registered. A registry that already holds either
/// name is left untouched: two registrations would mean two flag pairs, and the
/// tools would then disagree about whether plan mode is active.
pub fn register_plan_tools(registry: &mut ToolRegistry, service: &PlanService) -> bool {
    if registry.get(PLAN_TOOL_ENTER).is_some() || registry.get(PLAN_TOOL_EXIT).is_some() {
        return false;
    }
    registry.register(Box::new(EnterPlanModeTool::new(service.active_flag().as_arc())));
    registry.register(Box::new(ExitPlanModeTool::with_latch(
        service.active_flag().as_arc(),
        service.exit_latch().as_arc(),
    )));
    true
}

/// Convenience for callers that only need the two boxes.
pub fn plan_tools(service: &PlanService) -> (Box<EnterPlanModeTool>, Box<ExitPlanModeTool>) {
    (
        Box::new(EnterPlanModeTool::new(service.active_flag().as_arc())),
        Box::new(ExitPlanModeTool::with_latch(
            service.active_flag().as_arc(),
            service.exit_latch().as_arc(),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shared_flag(active: bool) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(active))
    }

    // --- EnterPlanModeTool unit tests ---

    #[test]
    fn enter_tool_name() {
        let tool = EnterPlanModeTool::new(make_shared_flag(false));
        assert_eq!(tool.name(), "EnterPlanMode");
    }

    #[test]
    fn enter_tool_category_is_info() {
        let tool = EnterPlanModeTool::new(make_shared_flag(false));
        assert!(matches!(tool.category(), ToolCategory::Info));
    }

    #[test]
    fn enter_tool_concurrency_safe() {
        let tool = EnterPlanModeTool::new(make_shared_flag(false));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn enter_tool_schema_has_no_required_fields() {
        let tool = EnterPlanModeTool::new(make_shared_flag(false));
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.is_empty());
    }

    #[test]
    fn enter_tool_context_modifier_returns_enter() {
        let tool = EnterPlanModeTool::new(make_shared_flag(false));
        let modifier = tool.context_modifier_for(&json!({}));
        assert!(modifier.is_some());
        let cm = modifier.unwrap();
        assert_eq!(cm.plan_mode_transition, Some(PlanModeTransition::Enter));
        // Other fields are default
        assert!(cm.model.is_none());
        assert!(cm.effort.is_none());
        assert!(cm.allowed_tools.is_empty());
    }

    #[tokio::test]
    async fn enter_succeeds_when_not_active() {
        let tool = EnterPlanModeTool::new(make_shared_flag(false));
        let result = tool.execute(json!({})).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Entered plan mode"));
    }

    #[tokio::test]
    async fn enter_rejects_when_already_active() {
        let tool = EnterPlanModeTool::new(make_shared_flag(true));
        let result = tool.execute(json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Already in plan mode"));
    }

    #[test]
    fn enter_tool_describe() {
        let tool = EnterPlanModeTool::new(make_shared_flag(false));
        assert_eq!(tool.describe(&json!({})), "Enter plan mode");
    }

    fn valid_plan() -> serde_json::Value {
        json!({
            "plan": "# Goal\nFix the parser.\nFiles: src/parser.rs\nVerification: cargo test -p parser\nHow to test: cargo test -p parser"
        })
    }

    // --- ExitPlanModeTool unit tests ---

    #[test]
    fn exit_tool_name() {
        let tool = ExitPlanModeTool::new(make_shared_flag(false));
        assert_eq!(tool.name(), "ExitPlanMode");
    }

    #[test]
    fn exit_tool_category_is_info() {
        let tool = ExitPlanModeTool::new(make_shared_flag(false));
        assert!(matches!(tool.category(), ToolCategory::Info));
    }

    #[test]
    fn exit_tool_concurrency_safe() {
        let tool = ExitPlanModeTool::new(make_shared_flag(false));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn exit_tool_schema_has_no_required_fields() {
        let tool = ExitPlanModeTool::new(make_shared_flag(false));
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.is_empty());
    }

    #[test]
    fn exit_tool_context_modifier_returns_exit() {
        let tool = ExitPlanModeTool::new(make_shared_flag(true));
        let modifier = tool.context_modifier_for(&valid_plan());
        assert!(modifier.is_some());
        let cm = modifier.unwrap();
        assert!(matches!(
            cm.plan_mode_transition,
            Some(PlanModeTransition::Exit {
                plan_content: Some(ref text)
            }) if text.contains("cargo test")
        ));
        assert!(cm.model.is_none());
        assert!(cm.effort.is_none());
        assert!(cm.allowed_tools.is_empty());
    }

    #[test]
    fn exit_tool_context_modifier_none_when_plan_invalid() {
        let tool = ExitPlanModeTool::new(make_shared_flag(true));
        assert!(tool.context_modifier_for(&json!({})).is_none());
        assert!(tool
            .context_modifier_for(&json!({ "plan": "too short" }))
            .is_none());
    }

    #[tokio::test]
    async fn exit_succeeds_when_active_with_verifiable_plan() {
        let tool = ExitPlanModeTool::new(make_shared_flag(true));
        let result = tool.execute(valid_plan()).await;
        assert!(!result.is_error);
        assert!(result.content.contains("submitted for approval"));
        assert!(result.content.contains("Fix the parser"));
    }

    #[tokio::test]
    async fn exit_rejects_empty_plan() {
        let tool = ExitPlanModeTool::new(make_shared_flag(true));
        let result = tool.execute(json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("requires a non-empty"));
    }

    #[tokio::test]
    async fn exit_echoes_plan_content() {
        let tool = ExitPlanModeTool::new(make_shared_flag(true));
        let result = tool.execute(valid_plan()).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Fix the parser"));
        let modifier = tool.context_modifier_for(&valid_plan());
        assert!(matches!(
            modifier.and_then(|m| m.plan_mode_transition),
            Some(PlanModeTransition::Exit {
                plan_content: Some(text)
            }) if text.contains("Fix the parser")
        ));
    }

    #[tokio::test]
    async fn exit_rejects_when_latched() {
        let latch = Arc::new(AtomicBool::new(true));
        let tool = ExitPlanModeTool::with_latch(make_shared_flag(true), latch);
        let result = tool.execute(valid_plan()).await;
        assert!(result.is_error);
        assert!(result.content.contains("already submitted"));
        assert!(tool.context_modifier_for(&valid_plan()).is_none());
    }

    #[tokio::test]
    async fn exit_rejects_when_not_active() {
        let tool = ExitPlanModeTool::new(make_shared_flag(false));
        let result = tool.execute(json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Not in plan mode"));
    }

    #[test]
    fn exit_tool_describe() {
        let tool = ExitPlanModeTool::new(make_shared_flag(false));
        assert_eq!(tool.describe(&json!({})), "Exit plan mode");
    }

    // --- Shared flag tests ---

    #[tokio::test]
    async fn shared_flag_reflects_state_changes() {
        let flag = make_shared_flag(false);
        let enter_tool = EnterPlanModeTool::new(flag.clone());
        let exit_tool = ExitPlanModeTool::new(flag.clone());

        // Initially not active — enter succeeds, exit fails
        let r = enter_tool.execute(json!({})).await;
        assert!(!r.is_error);
        let r = exit_tool.execute(json!({})).await;
        assert!(r.is_error);

        // Simulate engine setting the flag after processing Enter transition
        flag.store(true, Ordering::Release);

        // Now active — enter fails, exit with a verifiable plan succeeds
        let r = enter_tool.execute(json!({})).await;
        assert!(r.is_error);
        let r = exit_tool.execute(valid_plan()).await;
        assert!(!r.is_error);
    }

    // --- Registration ---

    #[test]
    fn registration_hands_the_tools_the_service_flags() {
        let service = PlanService::new();
        let mut registry = ToolRegistry::new();
        assert!(register_plan_tools(&mut registry, &service));
        assert!(registry.get(PLAN_TOOL_ENTER).is_some());
        assert!(registry.get(PLAN_TOOL_EXIT).is_some());
        assert_eq!(service.active_flag().get(), false);
        assert_eq!(service.exit_latch().get(), false);
    }

    #[test]
    fn registration_is_refused_when_a_plan_tool_already_exists() {
        let service = PlanService::new();
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EnterPlanModeTool::new(Arc::new(AtomicBool::new(false)))));
        assert!(
            !register_plan_tools(&mut registry, &service),
            "a second registration would create a second flag pair"
        );
    }
}

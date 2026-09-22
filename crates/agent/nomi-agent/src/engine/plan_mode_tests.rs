// ---------------------------------------------------------------------------
// Plan mode integration in the engine — seam-level tests.
//
// These used to construct `AgentEngine` with a literal and assert on the
// engine's private `plan_state`/`plan_active_flag` fields. Plan mode is a
// Feature now (docs/architecture/plan-goal-feature-seam.zh.md §6), so the
// assertions moved to the plan service's public read interface and to the
// engine's generic hook folds. The *semantics* are unchanged: enter snapshots
// the allow-list, exit latches without restoring writes, and the next user
// message approves the plan and restores the snapshot.
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use nomi_providers::{LlmProvider, ProviderError};
use nomi_tools::registry::ToolRegistry;
use nomi_types::llm::{LlmEvent, LlmRequest};
use nomi_types::skill_types::{ContextModifier, PlanModeTransition};

use crate::compact::state::CompactState;
use crate::confirm::ToolConfirmer;
use crate::features::plan::{PlanFeature, PlanPhase, PlanService};
use crate::output::null_sink::NullSink;

struct NullProvider;
#[async_trait::async_trait]
impl LlmProvider for NullProvider {
    async fn stream(
        &self,
        _: &LlmRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<LlmEvent>, ProviderError> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(rx)
    }
}

/// An engine whose only registered feature is plan mode, wired exactly as
/// bootstrap wires it: the flags are created first and adopted by the service,
/// so the service, the plan tools and the engine façade share them.
fn make_plan_engine(allow_list: Vec<String>) -> (super::AgentEngine, Arc<PlanService>) {
    let feature = Arc::new(PlanFeature::new());
    let service = Arc::clone(feature.service());

    let active = Arc::new(AtomicBool::new(false));
    let latch = Arc::new(AtomicBool::new(false));
    service.adopt_active_flag(active.clone().into());
    service.adopt_exit_latch(latch.clone().into());

    let mut features = crate::features::FeatureRegistry::new();
    features.register(feature);

    let mut engine = super::AgentEngine {
        provider: Arc::new(NullProvider),
        tools: ToolRegistry::new(),
        messages: vec![],
        system_prompt: String::new(),
        model: "test-model".to_string(),
        output_max_tokens: Some(4096),
        max_turns: Some(10),
        total_usage: Default::default(),
        thinking: None,
        compat: nomi_config::compat::ProviderCompat::anthropic_defaults(),
        confirmer: Arc::new(Mutex::new(ToolConfirmer::new(true, allow_list.clone()))),
        hooks: None,
        session_manager: None,
        current_session: None,
        output: Arc::new(NullSink),
        current_msg_id: String::new(),
        approval_manager: None,
        protocol_writer: None,
        allow_list,
        current_reasoning_effort: None,
        compact_config: nomi_config::compact::CompactConfig::default(),
        compact_state: CompactState::new(),
        cache_detector: super::CacheBreakDetector::new(),
        compaction_level: nomi_compact::CompactionLevel::default(),
        toon_enabled: false,
        max_recent_images: 3,
        commands: crate::commands::default_registry(),
        goal: None,
        goal_wait_probe: None,
        system_prompt_sections: std::collections::HashMap::new(),
        last_context_breakdown: None,
        moa: None,
        stagnation_guard: crate::loop_guard::StagnationGuard::new(crate::engine::STAGNATION_THRESHOLD),
        coding_harness: None,
        harness_runtime: Default::default(),
        compact_config_base: nomi_config::compact::CompactConfig::default(),
        file_cache: None,
        context_contributors: Vec::new(),
        steering_inbox: None,
        system_resource_inbox: None,
        frozen_provider_tools: None,
        sent_prefix_len: 0,
        process_supervisor: None,
        editable_turn: None,
        observation: None,
        horizon: Default::default(),
        features: Default::default(),
        reminders: Default::default(),
        provider_passes_in_turn: 0,
    };
    engine.set_features(features);
    engine.set_plan_active_flag(active);
    engine.set_plan_exit_latch(latch);
    (engine, service)
}

fn enter() -> ContextModifier {
    ContextModifier {
        plan_mode_transition: Some(PlanModeTransition::Enter),
        ..Default::default()
    }
}

fn exit(plan: &str) -> ContextModifier {
    ContextModifier {
        plan_mode_transition: Some(PlanModeTransition::Exit {
            plan_content: Some(plan.to_string()),
        }),
        ..Default::default()
    }
}

fn verifiable_plan() -> &'static str {
    "# Goal\nFix the parser\nVerification: cargo test -p parser"
}

// --- the plan service owns the transition ---

#[test]
fn enter_transition_activates_plan_mode() {
    let (mut engine, service) = make_plan_engine(vec!["Read".into(), "Bash".into()]);

    engine.apply_context_modifiers(&[Some(enter())]);

    assert!(service.is_active(), "plan mode should be active");
    assert_eq!(
        service.snapshot().pre_plan_allow_list,
        vec!["Read".to_string(), "Bash".to_string()],
        "the allow-list is snapshotted for restoration"
    );
    assert_eq!(service.snapshot().phase, PlanPhase::Exploring);
}

#[test]
fn enter_transition_updates_shared_flag() {
    let (mut engine, service) = make_plan_engine(vec![]);
    let flag = service.active_flag();
    assert!(!flag.get());

    engine.apply_context_modifiers(&[Some(enter())]);

    assert!(flag.get(), "the tool-visible flag mirrors the entry");
}

#[test]
fn exit_transition_latches_awaiting_approval() {
    let (mut engine, service) = make_plan_engine(vec!["Read".into(), "Bash".into()]);

    engine.apply_context_modifiers(&[Some(enter())]);
    assert!(service.is_active());

    engine.allow_list.push("NewTool".into());
    engine.apply_context_modifiers(&[Some(exit(verifiable_plan()))]);

    assert!(
        service.is_active(),
        "writes stay locked until the user continues"
    );
    assert_eq!(service.snapshot().phase, PlanPhase::AwaitingApproval);
    assert!(
        engine.allow_list.contains(&"NewTool".to_string()),
        "allow_list is not restored until the user continues"
    );
}

#[test]
fn next_user_turn_approves_plan_and_restores_writes() {
    let (mut engine, service) = make_plan_engine(vec!["Read".into(), "Bash".into()]);
    engine.apply_context_modifiers(&[Some(enter())]);
    engine.apply_context_modifiers(&[Some(exit(verifiable_plan()))]);

    // #1's fold is what an incoming user message runs.
    let hook_ctx = engine.features().run_user_request();
    if let Some(restored) = hook_ctx.take_allow_list() {
        engine.allow_list = restored;
    }

    assert!(!service.is_active());
    assert_eq!(service.snapshot().phase, PlanPhase::Idle);
    assert_eq!(
        engine.allow_list,
        vec!["Read".to_string(), "Bash".to_string()]
    );
}

#[test]
fn exit_transition_updates_shared_flag() {
    let (mut engine, service) = make_plan_engine(vec![]);
    let flag = service.active_flag();

    engine.apply_context_modifiers(&[Some(enter())]);
    assert!(flag.get());

    engine.apply_context_modifiers(&[Some(exit(verifiable_plan()))]);
    assert!(
        flag.get(),
        "the flag stays true until the user approves the plan"
    );
}

#[test]
fn no_transition_does_not_affect_plan_state() {
    let (mut engine, service) = make_plan_engine(vec![]);

    engine.apply_context_modifiers(&[Some(ContextModifier {
        model: Some("new-model".into()),
        plan_mode_transition: None,
        ..Default::default()
    })]);

    assert_eq!(engine.model, "new-model");
    assert!(!service.is_active(), "plan state should remain inactive");
}

#[test]
fn enter_with_model_override_both_applied() {
    let (mut engine, service) = make_plan_engine(vec![]);

    engine.apply_context_modifiers(&[Some(ContextModifier {
        model: Some("planning-model".into()),
        plan_mode_transition: Some(PlanModeTransition::Enter),
        ..Default::default()
    })]);

    assert!(service.is_active());
    assert_eq!(engine.model, "planning-model");
}

#[test]
fn a_session_without_the_plan_feature_ignores_a_transition() {
    // No plan feature registered: every fold is empty, so a modifier that only
    // carries a plan transition changes nothing and does not panic.
    let mut engine = make_plan_engine(vec![]).0;
    engine.set_features(crate::features::FeatureRegistry::new());

    engine.apply_context_modifiers(&[Some(enter())]);

    assert!(engine.features().is_empty());
}

// --- the engine's generic folds carry the plan facts ---

fn thinking_budget(config: Option<nomi_types::llm::ThinkingConfig>) -> Option<u32> {
    match config {
        Some(nomi_types::llm::ThinkingConfig::Enabled { budget_tokens }) => Some(budget_tokens),
        _ => None,
    }
}

#[test]
fn the_request_gate_suspends_the_tool_loop_downgrade() {
    let (engine, service) = make_plan_engine(vec![]);
    // The engine's own candidate for a follow-up pass: a quarter of the
    // session's budget, one tier lower effort.
    let candidate = || crate::features::RequestParams {
        thinking: Some(nomi_types::llm::ThinkingConfig::Enabled {
            budget_tokens: 2048,
        }),
        reasoning_effort: Some("low".into()),
        base_thinking: Some(nomi_types::llm::ThinkingConfig::Enabled {
            budget_tokens: 8192,
        }),
        base_reasoning_effort: Some("high".into()),
        continuation_after_tools: true,
    };
    let facade = |service: &PlanService| {
        let hooks = crate::features::HookCtx::new();
        hooks.publish_plan_status(service.status());
        hooks
    };

    let outside = engine
        .features()
        .apply_request_gates(candidate(), &facade(&service));
    assert_eq!(
        thinking_budget(outside.thinking),
        Some(2048),
        "outside plan mode the downgrade stands"
    );
    assert_eq!(outside.reasoning_effort.as_deref(), Some("low"));

    service.enter(vec![]);
    let inside = engine
        .features()
        .apply_request_gates(candidate(), &facade(&service));
    assert_eq!(
        thinking_budget(inside.thinking),
        Some(8192),
        "plan mode keeps the session's full thinking budget"
    );
    assert_eq!(
        inside.reasoning_effort.as_deref(),
        Some("high"),
        "and its session reasoning effort"
    );
}

#[test]
fn the_dispatch_gate_refuses_writers_only_while_plan_is_active() {
    use nomi_protocol::events::ToolCategory;

    let (engine, service) = make_plan_engine(vec![]);
    let write = crate::features::DispatchCtx::new("Write", ToolCategory::Edit);
    let read = crate::features::DispatchCtx::new("Read", ToolCategory::Info);
    let gate = || {
        crate::features::DispatchGate::from_registry(engine.features(), service.status())
    };

    assert!(
        gate().denial(&write).is_none(),
        "no plan mode, no refusal"
    );

    service.enter(vec![]);
    let denial = gate()
        .denial(&write)
        .expect("a writer is refused in plan mode");
    assert!(denial.message.contains("Plan mode is read-only"));
    assert!(
        gate().denial(&read).is_none(),
        "read-only tools still dispatch"
    );
}

#[test]
fn the_engine_facade_flags_reach_the_service_the_tools_read() {
    // `set_plan_active_flag` must not create a second flag pair: the plan tools
    // read whatever the service publishes onto.
    let mut engine = make_plan_engine(vec![]).0;
    let host_flag = Arc::new(AtomicBool::new(false));

    engine.set_plan_active_flag(Arc::clone(&host_flag));
    engine.apply_context_modifiers(&[Some(enter())]);

    assert!(
        host_flag.load(Ordering::Acquire),
        "the host-created flag observes the entry"
    );
}

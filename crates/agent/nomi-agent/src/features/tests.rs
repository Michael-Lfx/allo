//! Seam-level tests for the feature registry's fold semantics.
//!
//! These pin the contract every engine call site relies on: registration order,
//! idempotent registration, chained folds, and first-denial-wins. A feature
//! implemented here is deliberately trivial — the point is the registry, not
//! the feature.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nomi_protocol::events::ToolCategory;
use nomi_types::skill_types::{ContextModifier, PlanModeTransition};

use super::*;

/// A feature that records the order in which its hooks ran, and can be made to
/// deny one tool name.
struct ProbeFeature {
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
    deny: Option<String>,
    /// Extra allow-list entry the modifier hook appends.
    allow: Option<String>,
}

impl ProbeFeature {
    fn new(name: &'static str, log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self {
            name,
            log,
            deny: None,
            allow: None,
        }
    }

    fn denying(mut self, tool: &str) -> Self {
        self.deny = Some(tool.to_string());
        self
    }

    fn pushing(mut self, tool: &str) -> Self {
        self.allow = Some(tool.to_string());
        self
    }

    fn record(&self, what: &str) {
        self.log.lock().unwrap().push(format!("{}:{what}", self.name));
    }
}

impl Feature for ProbeFeature {
    fn name(&self) -> &'static str {
        self.name
    }

    fn register_tools(&self, _registry: &mut ToolRegistry) {
        self.record("tools");
    }

    fn hooks(&self) -> FeatureHooks {
        let name = self.name;
        let log = Arc::clone(&self.log);
        let deny = self.deny.clone();
        let allow = self.allow.clone();

        let on_user_request: UserRequestFn = Arc::new(move |_| {
            log.lock().unwrap().push(format!("{name}:user_request"));
        });

        let log_gate = Arc::clone(&self.log);
        let dispatch_gate: GateFn = Arc::new(move |dispatch: &DispatchCtx, _| {
            log_gate
                .lock()
                .unwrap()
                .push(format!("{name}:gate:{}", dispatch.tool_name));
            match &deny {
                Some(denied) if *denied == dispatch.tool_name => {
                    Some(ToolDenial::new(format!("{name} refused {}", dispatch.tool_name)))
                }
                _ => None,
            }
        });

        let log_modifier = Arc::clone(&self.log);
        let apply_modifier: ModifierFn = Arc::new(move |_modifier, mut ctx| {
            log_modifier
                .lock()
                .unwrap()
                .push(format!("{name}:modifier"));
            if let Some(tool) = &allow {
                ctx.allow_list.push(tool.clone());
            }
            ctx
        });

        let log_request = Arc::clone(&self.log);
        let request_gates: RequestGateFn = Arc::new(move |mut params, _| {
            log_request
                .lock()
                .unwrap()
                .push(format!("{name}:request_gate"));
            // Each gate halves the budget; chaining order is observable.
            if let Some(nomi_types::llm::ThinkingConfig::Enabled { budget_tokens }) = params.thinking
            {
                params.thinking = Some(nomi_types::llm::ThinkingConfig::Enabled {
                    budget_tokens: budget_tokens / 2,
                });
            }
            params
        });

        let log_tool_turn = Arc::clone(&self.log);
        let on_tool_turn: ToolTurnFn = Arc::new(move |turn, _| {
            let name = name;
            let log = Arc::clone(&log_tool_turn);
            Box::pin(async move {
                log.lock()
                    .unwrap()
                    .push(format!("{name}:tool_turn:{}", turn.calls.len()));
            })
        });

        let log_natural = Arc::clone(&self.log);
        let on_natural_end: NaturalEndFn = Arc::new(move |_ctx, _| {
            let name = name;
            let log = Arc::clone(&log_natural);
            Box::pin(async move {
                log.lock().unwrap().push(format!("{name}:natural_end"));
                if name == "first" {
                    NaturalEndDecision::continue_with("keep going", true)
                } else {
                    NaturalEndDecision::stop()
                }
            })
        });

        FeatureHooks {
            on_user_request: vec![on_user_request],
            dispatch_gate: vec![dispatch_gate],
            apply_modifier: vec![apply_modifier],
            request_gates: vec![request_gates],
            on_tool_turn: vec![on_tool_turn],
            on_natural_end: vec![on_natural_end],
        }
    }
}

fn log() -> Arc<std::sync::Mutex<Vec<String>>> {
    Arc::new(std::sync::Mutex::new(Vec::new()))
}

fn names(registry: &FeatureRegistry) -> Vec<&'static str> {
    registry.entries().iter().map(|e| e.name).collect()
}

#[test]
fn an_empty_registry_folds_to_its_input() {
    let registry = FeatureRegistry::new();
    assert!(registry.is_empty());

    let params = RequestParams {
        thinking: Some(nomi_types::llm::ThinkingConfig::Enabled {
            budget_tokens: 4096,
        }),
        reasoning_effort: Some("high".into()),
        continuation_after_tools: true,
    };
    let out = registry.apply_request_gates(params, FacadeCtx::default());
    // `ThinkingConfig` is not `PartialEq`; compare the budget directly.
    match out.thinking {
        Some(nomi_types::llm::ThinkingConfig::Enabled { budget_tokens }) => {
            assert_eq!(budget_tokens, 4096)
        }
        other => panic!("an empty registry must not rewrite thinking: {other:?}"),
    }
    assert_eq!(out.reasoning_effort.as_deref(), Some("high"));
    assert!(out.continuation_after_tools);

    let ctx = registry.apply_modifiers(&[], ModifierCtx::default());
    assert!(ctx.allow_list.is_empty());

    let dispatch = DispatchCtx::new("Bash", ToolCategory::Exec);
    assert!(registry.first_denial(&dispatch, FacadeCtx::default()).is_none());
}

#[test]
fn registration_order_is_the_fold_order() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("first", Arc::clone(&log))));
    registry.register(Arc::new(ProbeFeature::new("second", Arc::clone(&log))));

    assert_eq!(names(&registry), vec!["first", "second"]);

    registry.run_user_request(FacadeCtx::default());
    assert_eq!(
        *log.lock().unwrap(),
        vec!["first:user_request", "second:user_request"]
    );
}

#[test]
fn re_registering_a_name_replaces_instead_of_duplicating() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("plan", Arc::clone(&log))));
    registry.register(Arc::new(ProbeFeature::new("plan", Arc::clone(&log))));

    assert_eq!(registry.len(), 1, "a name is registered at most once");
    assert_eq!(names(&registry), vec!["plan"]);

    registry.run_user_request(FacadeCtx::default());
    assert_eq!(
        log.lock().unwrap().len(),
        1,
        "a double bootstrap cannot double-invoke a hook"
    );
}

#[test]
fn the_first_denial_wins_and_later_gates_are_not_consulted() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(
        ProbeFeature::new("first", Arc::clone(&log)).denying("Write"),
    ));
    registry.register(Arc::new(
        ProbeFeature::new("second", Arc::clone(&log)).denying("Write"),
    ));

    let write = DispatchCtx::new("Write", ToolCategory::Edit);
    let denial = registry
        .first_denial(&write, FacadeCtx::default())
        .expect("the first gate refuses");
    assert_eq!(denial.message, "first refused Write");
    assert_eq!(
        *log.lock().unwrap(),
        vec!["first:gate:Write"],
        "the second gate short-circuits behind the first denial"
    );
}

#[test]
fn an_allowing_gate_lets_the_next_one_decide() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("first", Arc::clone(&log))));
    registry.register(Arc::new(
        ProbeFeature::new("second", Arc::clone(&log)).denying("Write"),
    ));

    let write = DispatchCtx::new("Write", ToolCategory::Edit);
    let denial = registry
        .first_denial(&write, FacadeCtx::default())
        .expect("the second gate refuses");
    assert_eq!(denial.message, "second refused Write");
    assert_eq!(
        *log.lock().unwrap(),
        vec!["first:gate:Write", "second:gate:Write"]
    );
}

#[test]
fn modifiers_chain_so_a_later_feature_sees_an_earlier_write() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(
        ProbeFeature::new("first", Arc::clone(&log)).pushing("Alpha"),
    ));
    registry.register(Arc::new(
        ProbeFeature::new("second", Arc::clone(&log)).pushing("Beta"),
    ));

    let modifiers = vec![Some(ContextModifier {
        plan_mode_transition: Some(PlanModeTransition::Enter),
        ..Default::default()
    })];
    let ctx = registry.apply_modifiers(&modifiers, ModifierCtx::default());

    assert_eq!(ctx.allow_list, vec!["Alpha".to_string(), "Beta".to_string()]);
}

#[test]
fn request_gates_chain_in_registration_order() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("first", Arc::clone(&log))));
    registry.register(Arc::new(ProbeFeature::new("second", Arc::clone(&log))));

    let params = RequestParams {
        thinking: Some(nomi_types::llm::ThinkingConfig::Enabled {
            budget_tokens: 8192,
        }),
        ..Default::default()
    };
    let out = registry.apply_request_gates(params, FacadeCtx::default());

    // Two halvings: 8192 -> 4096 -> 2048.
    match out.thinking {
        Some(nomi_types::llm::ThinkingConfig::Enabled { budget_tokens }) => {
            assert_eq!(budget_tokens, 2048)
        }
        other => panic!("expected a halved thinking budget, got {other:?}"),
    }
    assert_eq!(
        *log.lock().unwrap(),
        vec!["first:request_gate", "second:request_gate"]
    );
}

#[tokio::test]
async fn tool_turn_observation_visits_every_feature() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("first", Arc::clone(&log))));
    registry.register(Arc::new(ProbeFeature::new("second", Arc::clone(&log))));

    let turn = ToolTurn {
        calls: vec![
            ToolCallObservation {
                name: "Write".into(),
                command: None,
                success: true,
            },
            ToolCallObservation {
                name: "Bash".into(),
                command: Some("cargo test".into()),
                success: false,
            },
        ],
    };
    registry.observe_tool_turn(turn, FacadeCtx::default()).await;

    assert_eq!(
        *log.lock().unwrap(),
        vec!["first:tool_turn:2", "second:tool_turn:2"]
    );
}

#[tokio::test]
async fn every_natural_end_hook_runs_but_the_first_continuation_wins() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("first", Arc::clone(&log))));
    registry.register(Arc::new(ProbeFeature::new("second", Arc::clone(&log))));

    let ctx = NaturalEndCtx {
        assistant_text: "done".into(),
        input_tokens: 10,
        output_tokens: 2,
    };
    let decision = registry.resolve_natural_end(&ctx, FacadeCtx::default()).await;

    assert_eq!(decision.continuation.as_deref(), Some("keep going"));
    assert!(
        decision.record_continuation,
        "the winning continuation's budget charge carries through"
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec!["first:natural_end", "second:natural_end"],
        "a later feature is still consulted: it may owe bookkeeping"
    );
}

#[tokio::test]
async fn natural_end_without_a_continuation_stops() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("second", Arc::clone(&log))));

    let decision = registry
        .resolve_natural_end(&NaturalEndCtx::default(), FacadeCtx::default())
        .await;
    assert!(decision.continuation.is_none());
    assert!(!decision.record_continuation);
}

#[test]
fn register_tools_visits_every_feature_in_order() {
    let log = log();
    let mut registry = FeatureRegistry::new();
    registry.register(Arc::new(ProbeFeature::new("first", Arc::clone(&log))));
    registry.register(Arc::new(ProbeFeature::new("second", Arc::clone(&log))));

    let mut tools = ToolRegistry::new();
    registry.register_tools(&mut tools);

    assert_eq!(*log.lock().unwrap(), vec!["first:tools", "second:tools"]);
}

#[test]
fn shared_flag_is_visible_across_clones() {
    let flag = SharedFlag::new(false);
    let clone = flag.clone();
    flag.set(true);
    assert!(clone.get());

    let counter = Arc::new(AtomicUsize::new(0));
    let shared = Shared::new(0usize);
    shared.with_mut(|value| *value += 1);
    shared.with(|value| counter.store(*value, Ordering::SeqCst));
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    assert_eq!(shared.snapshot(), 1);
}

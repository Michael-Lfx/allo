//! Goal-driven continuation as a self-registering feature.
//!
//! The feature owns the horizon controller (progress ledger + auto-continue
//! budget), the goal runtime, the host liveness probe, and the provider the
//! judge calls. It contributes the three-state status reminder and the
//! natural-end hook that runs the judge and decides whether the turn continues.
//!
//! Every hardening gate stays exactly as it was and now lives behind this
//! feature: the horizon's continuation budget, the no-progress idle-streak
//! veto, judge fail-closed on parse/transport failure, and the "judge cannot
//! declare done without mechanical evidence" completion gate. Extraction is
//! move + delegate — no rule was rewritten.
//!
//! Coding sessions still disable goal auto-continue; the engine expresses that
//! as `NaturalEndCtx::auto_continue_allowed == false`, so this feature never has
//! to know a coding harness exists.
//!
//! `update_goal` registration stays with `AgentEngine::set_goal` /
//! `set_goal_state`: the tool must share the exact `Arc<Mutex<GoalState>>` its
//! runtime was built with, and tools are registered into the engine's registry,
//! which a feature hook cannot reach. Those two façade methods therefore call
//! the service first, then register the tool against the service's own slot.

use std::sync::Arc;

use nomi_providers::LlmProvider;
use nomi_tools::registry::ToolRegistry;

use super::{
    Feature, FeatureHooks, NaturalEndCtx, NaturalEndDecision, NaturalEndFn, PlanStatus,
    ReminderCtx, ReminderSpec, Shared, ToolCallObservation, ToolTurn, ToolTurnFn,
};
use crate::goal::judge::ProviderJudgeClient;
use crate::goal::runtime::{GoalContinueGate, GoalRuntime, GoalWaitProbe};
use crate::goal::state::{GoalState, GoalStatus, render_subgoals_block};
use crate::horizon::{HorizonController, HorizonDecision, ToolObservation};

/// Reminder variant name for the goal status block.
pub const GOAL_INJECTION_VARIANT: &str = "goal";

/// Re-state the goal block after this many provider passes in one turn.
///
/// The reference implementation injects the goal only on a new turn. nomi keeps
/// that (the render returns `None` without a new user message or turn start) and
/// adds a refresh so a very long tool loop does not lose the objective entirely.
pub const GOAL_REFRESH_AFTER_PASSES: usize = crate::horizon::OFFICE_PLAN_HARD;

/// Goal state, horizon controller, and the judge plumbing.
///
/// Held behind an `Arc` by [`GoalFeature`]; the engine reaches it only through
/// its façade (`set_goal` / `goal_state` / `goal_runtime_handle` …).
pub struct GoalService {
    /// Progress ledger + auto-continue budget. Also drives the office plan
    /// overlay, which is why `observe_office_plan_turn` lives here.
    horizon: Shared<HorizonController>,
    goal: Shared<Option<GoalRuntime>>,
    /// Host liveness probe, applied to the current runtime and every later one.
    wait_probe: Shared<Option<Arc<dyn GoalWaitProbe>>>,
    /// Provider + model for the judge. Injected once by bootstrap; without it a
    /// goal falls back to the synchronous, judge-free continuation path.
    judge_target: Shared<Option<(Arc<dyn LlmProvider>, String)>>,
    /// Session observation handle, for `EVENT_HORIZON_DECISION` telemetry.
    observation: Shared<Option<Arc<crate::observation::ObservationSession>>>,
}

impl Default for GoalService {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalService {
    pub fn new() -> Self {
        Self {
            horizon: Shared::new(HorizonController::default()),
            goal: Shared::new(None),
            wait_probe: Shared::new(None),
            judge_target: Shared::new(None),
            observation: Shared::new(None),
        }
    }

    /// Install the provider/model the judge calls, plus the session observation.
    pub fn configure(
        &self,
        provider: Arc<dyn LlmProvider>,
        model: String,
        observation: Option<Arc<crate::observation::ObservationSession>>,
    ) {
        self.judge_target.with_mut(|slot| *slot = Some((provider, model)));
        self.observation.with_mut(|slot| *slot = observation);
    }

    // -- façade delegates ---------------------------------------------------

    /// Enable goal-driven continuation for this session.
    pub fn set_goal(&self, objective: String, max_auto_continuations: usize) {
        let runtime = GoalRuntime::new(objective, max_auto_continuations);
        if let Some(probe) = self.wait_probe.snapshot() {
            runtime.set_wait_probe(probe);
        }
        self.horizon.with_mut(|horizon| {
            horizon.reset();
            horizon.configure_goal(max_auto_continuations, None);
        });
        self.goal.with_mut(|slot| *slot = Some(runtime));
    }

    /// Restore-semantics counterpart of [`Self::set_goal`].
    pub fn set_goal_state(&self, state: GoalState) {
        self.horizon
            .with_mut(|horizon| horizon.configure_goal(state.max_auto_continuations, state.contract.as_ref()));
        let probe = self.wait_probe.snapshot();
        self.goal.with_mut(|slot| match slot.as_ref() {
            Some(runtime) => runtime.restore(state),
            None => {
                let runtime = GoalRuntime::from_state(state);
                if let Some(probe) = probe {
                    runtime.set_wait_probe(probe);
                }
                *slot = Some(runtime);
            }
        });
    }

    /// Install the host's liveness probe for pid/session wait barriers. Applied
    /// to the current runtime and remembered for every later one.
    pub fn set_wait_probe(&self, probe: Arc<dyn GoalWaitProbe>) {
        if let Some(runtime) = self.goal.with(|slot| slot.as_ref().map(|r| r.clone())) {
            runtime.set_wait_probe(Arc::clone(&probe));
        }
        self.wait_probe.with_mut(|slot| *slot = Some(probe));
    }

    pub fn snapshot(&self) -> Option<GoalState> {
        self.goal.with(|slot| slot.as_ref().map(|runtime| runtime.snapshot()))
    }

    pub fn runtime_handle(&self) -> Option<GoalRuntime> {
        self.goal.with(|slot| slot.clone())
    }

    /// The shared goal-state slot `UpdateGoalTool` must be built against.
    ///
    /// `None` before any goal exists; the engine's façade registers the tool
    /// only once it has one.
    pub fn tool_state(&self) -> Option<Arc<std::sync::Mutex<GoalState>>> {
        self.goal
            .with(|slot| slot.as_ref().map(|runtime| runtime.shared_state()))
    }

    // -- turn-loop delegates ------------------------------------------------

    /// One root user request is starting.
    pub fn on_user_request(&self) {
        self.horizon.with_mut(|horizon| horizon.on_user_request());
    }

    /// Charge one consumed continuation to the horizon budget.
    ///
    /// The engine pushes the continuation message and owns the turn counter; the
    /// budget that caps how many such messages are allowed lives here.
    pub fn record_continuation(&self) {
        self.horizon.with_mut(|horizon| horizon.record_continuation());
    }

    /// Office plan overlay for one provider pass; `None` when inactive.
    pub fn observe_office_plan_turn(&self, plan_active: bool) -> Option<&'static str> {
        self.horizon
            .with_mut(|horizon| horizon.observe_office_plan_turn(plan_active))
            .text()
    }

    /// Observe one finished tool turn: fold the observations into the ledger and
    /// mirror the mechanical progress snapshot onto the goal state.
    pub fn observe_tool_turn(&self, calls: &[ToolCallObservation]) {
        let observations: Vec<ToolObservation> = calls
            .iter()
            .map(|call| ToolObservation {
                name: call.name.clone(),
                command: call.command.clone(),
                success: call.success,
            })
            .collect();
        self.horizon.with_mut(|horizon| horizon.observe_tools(&observations));
        self.sync_goal_progress();
    }

    /// Mirror Horizon's mechanical snapshot onto the shared goal state.
    fn sync_goal_progress(&self) {
        let snapshot = self.horizon.with(|horizon| horizon.progress());
        let goal = self.goal.with(|slot| slot.clone());
        if let Some(runtime) = goal {
            runtime.sync_progress(
                snapshot.mutated,
                snapshot.verify_ok,
                snapshot.workspace_changed,
                snapshot.no_progress_streak,
            );
        }
    }

    /// Evaluate the natural-termination point and return the continuation text
    /// the engine should push as the next user message, if any.
    async fn evaluate_natural_end(
        &self,
        ctx: &NaturalEndCtx,
        plan_status: PlanStatus,
        auto_continue_allowed: bool,
    ) -> (Option<String>, bool) {
        // Usage is charged even when the goal cannot continue: the budget is the
        // anti-runaway ceiling for the whole request, not just for goal turns.
        self.horizon.with_mut(|horizon| {
            horizon.record_usage(ctx.input_tokens, ctx.output_tokens);
            horizon.observe_end_turn(&ctx.assistant_text, ctx.cwd.as_deref(), 0);
        });
        self.sync_goal_progress();
        self.horizon.with_mut(|horizon| horizon.consume_turn_scoped());

        if !auto_continue_allowed {
            return (None, false);
        }
        let Some(snapshot) = self.snapshot() else {
            return (None, false);
        };

        self.horizon.with_mut(|horizon| {
            horizon.align_budget(
                snapshot.max_auto_continuations,
                snapshot.contract.as_ref(),
                snapshot.auto_continuations,
            );
        });
        let decision = self
            .horizon
            .with(|horizon| horizon.decide(plan_status.active, plan_status.awaiting_approval));
        self.emit_horizon_decision(&decision);
        let delta = self.horizon.with(|horizon| {
            horizon.continuation_delta(
                &snapshot.objective,
                snapshot.contract.as_ref(),
                &render_subgoals_block(&snapshot.subgoals),
            )
        });
        let gate = GoalContinueGate {
            allow_continue: decision.allow_continue,
            pause_on_veto: decision.pause_goal,
            veto_reason: Some(decision.reason.clone()),
            continuation_delta: Some(delta),
            observed: true,
        };

        let judge_target = self.judge_target.with(|slot| slot.clone());
        let Some((provider, model)) = judge_target else {
            // No judge wired: fall back to the synchronous continuation path so
            // a goal still makes progress instead of silently going inert.
            let continuation = self
                .goal
                .with(|slot| slot.as_ref().and_then(|runtime| runtime.maybe_continuation()));
            return (continuation.map(message_text), false);
        };

        let mut judge = ProviderJudgeClient::new(provider, model);
        if let Some(session) = self.observation.with(|slot| slot.clone()) {
            judge = judge.with_observation(session);
        }
        let runtime = self.goal.with(|slot| slot.clone());
        let continuation = match runtime {
            Some(runtime) => {
                runtime
                    .evaluate_and_continue_with(&ctx.assistant_text, &judge, gate)
                    .await
            }
            None => None,
        };
        let record = continuation.is_some();
        (continuation.map(message_text), record)
    }

    fn emit_horizon_decision(&self, decision: &HorizonDecision) {
        let Some(session) = self.observation.with(|slot| slot.clone()) else {
            return;
        };
        let payload = self
            .horizon
            .with(|horizon| horizon.observation_payload(decision));
        let _ = session.emit(nomi_agent_trace::EVENT_HORIZON_DECISION, payload);
    }

    /// Three-state status block for the reminder channel, or `None` when the
    /// goal is terminal.
    fn status_context(&self) -> Option<String> {
        self.goal
            .with(|slot| slot.as_ref().and_then(|runtime| runtime.status_context()))
    }

    /// Whether the goal can still run.
    fn is_live(&self) -> bool {
        self.snapshot().is_some_and(|snapshot| {
            matches!(snapshot.status, GoalStatus::Active | GoalStatus::Waiting)
        })
    }
}

/// The text of a continuation message, for handing back to the engine.
fn message_text(message: nomi_types::message::Message) -> String {
    message
        .content
        .iter()
        .find_map(|block| match block {
            nomi_types::message::ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Goal-driven continuation as an engine feature.
///
/// Registered unconditionally: whether a goal is *active* is a host decision
/// (`AgentBootstrap::goal` / `AgentEngine::set_goal`), so a goal-less session
/// contributes no reminder and never asks the engine to continue.
pub struct GoalFeature {
    service: Arc<GoalService>,
}

impl Default for GoalFeature {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalFeature {
    pub fn new() -> Self {
        Self {
            service: Arc::new(GoalService::new()),
        }
    }

    pub fn service(&self) -> &Arc<GoalService> {
        &self.service
    }
}

impl Feature for GoalFeature {
    fn name(&self) -> &'static str {
        "goal"
    }

    fn register_tools(&self, _registry: &mut ToolRegistry) {
        // `update_goal` is registered by `AgentEngine::set_goal` /
        // `set_goal_state`, which own the state slot the tool must share.
    }

    fn reminders(&self) -> Vec<ReminderSpec> {
        let service = Arc::clone(&self.service);
        vec![
            ReminderSpec::new(GOAL_INJECTION_VARIANT, move |ctx: &ReminderCtx| {
                // The reference injects the goal only on a new turn. An active
                // goal is worth restating then; a paused/blocked block is a
                // notification of a state change, so it is also only useful at a
                // turn boundary — not on every pass of the turn that follows.
                if !ctx.turn_start && !ctx.new_user_message {
                    return None;
                }
                if !service.is_live() && !ctx.new_user_message {
                    return None;
                }
                service.status_context()
            })
            .refreshing_every(GOAL_REFRESH_AFTER_PASSES),
        ]
    }

    fn hooks(&self) -> FeatureHooks {
        // #6 Natural-end continuation. The engine passes the plan facts and the
        // coding-harness verdict as data; this hook inspects neither.
        let natural_service = Arc::clone(&self.service);
        let on_natural_end: NaturalEndFn = Arc::new(move |ctx: NaturalEndCtx, plan: PlanStatus| {
            let service = Arc::clone(&natural_service);
            Box::pin(async move {
                let allowed = ctx.auto_continue_allowed;
                let (continuation, record) = service.evaluate_natural_end(&ctx, plan, allowed).await;
                NaturalEndDecision {
                    continuation,
                    record_continuation: record,
                }
            })
        });

        // Tool-turn observation: the ledger needs every tool result in every
        // profile, because the no-progress veto must see them all.
        let tool_service = Arc::clone(&self.service);
        let on_tool_turn: ToolTurnFn = Arc::new(move |turn: ToolTurn, _plan: PlanStatus| {
            let service = Arc::clone(&tool_service);
            Box::pin(async move {
                service.observe_tool_turn(&turn.calls);
            })
        });

        FeatureHooks {
            on_tool_turn: vec![on_tool_turn],
            on_natural_end: vec![on_natural_end],
            ..FeatureHooks::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_name_is_stable() {
        assert_eq!(GoalFeature::new().name(), "goal");
    }

    #[test]
    fn a_goal_less_session_contributes_no_plan_hooks() {
        let hooks = GoalFeature::new().hooks();
        assert!(hooks.on_user_request.is_empty());
        assert!(hooks.dispatch_gate.is_empty());
        assert!(hooks.apply_modifier.is_empty());
        assert!(hooks.request_gates.is_empty());
        assert_eq!(hooks.on_tool_turn.len(), 1);
        assert_eq!(hooks.on_natural_end.len(), 1);
    }

    #[test]
    fn a_goal_less_session_renders_no_reminder() {
        let feature = GoalFeature::new();
        let specs = feature.reminders();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].variant, GOAL_INJECTION_VARIANT);
        for ctx in [
            ReminderCtx {
                turn_start: true,
                new_user_message: true,
                ..Default::default()
            },
            ReminderCtx {
                new_user_message: true,
                ..Default::default()
            },
        ] {
            assert!((specs[0].render)(&ctx).is_none());
        }
    }

    #[test]
    fn office_plan_overlay_is_silent_without_plan_mode() {
        let service = GoalService::new();
        assert!(service.observe_office_plan_turn(false).is_none());
    }

    #[test]
    fn an_empty_goal_slot_reports_nothing() {
        let service = GoalService::new();
        assert!(service.snapshot().is_none());
        assert!(service.runtime_handle().is_none());
        assert!(service.tool_state().is_none());
        assert!(!service.is_live());
    }

    #[test]
    fn setting_a_goal_publishes_a_snapshot_and_tool_slot() {
        let service = GoalService::new();
        service.set_goal("ship the feature".into(), 3);

        let snapshot = service.snapshot().expect("goal is set");
        assert_eq!(snapshot.objective, "ship the feature");
        assert_eq!(snapshot.status, GoalStatus::Active);
        assert_eq!(snapshot.max_auto_continuations, 3);
        assert!(service.runtime_handle().is_some());
        assert!(service.tool_state().is_some());
        assert!(service.is_live());
    }

    #[test]
    fn a_restored_paused_goal_renders_a_status_block() {
        let service = GoalService::new();
        let mut state = GoalState::new("ship it".into(), 8);
        state.status = GoalStatus::Paused;
        state.paused_reason = Some("budget exhausted".into());
        service.set_goal_state(state);

        let block = service.status_context().expect("a paused goal says so");
        assert!(block.contains("目标已暂停"));
        assert!(block.contains("ship it"));
        assert!(block.contains("budget exhausted"));
    }

    #[test]
    fn a_blocked_goal_renders_the_judge_reason() {
        let service = GoalService::new();
        let mut state = GoalState::new("ship it".into(), 8);
        state.status = GoalStatus::Blocked;
        state.last_reason = Some("needs a production API key".into());
        service.set_goal_state(state);

        let block = service.status_context().expect("a blocked goal says so");
        assert!(block.contains("阻塞"));
        assert!(block.contains("needs a production API key"));
    }

    #[test]
    fn terminal_goals_render_nothing() {
        for status in [GoalStatus::Complete, GoalStatus::Cleared] {
            let service = GoalService::new();
            let mut state = GoalState::new("ship it".into(), 8);
            state.status = status;
            service.set_goal_state(state);
            assert!(
                service.status_context().is_none(),
                "{status:?} must stay silent so a finished goal costs no tokens"
            );
        }
    }

    #[tokio::test]
    async fn a_goal_less_session_never_continues() {
        let service = GoalService::new();
        let feature = Arc::new(GoalFeature::new());
        let mut registry = crate::features::FeatureRegistry::new();
        registry.register(feature);
        let ctx = NaturalEndCtx {
            assistant_text: "done".into(),
            ..Default::default()
        };
        let decision = registry
            .resolve_natural_end(&ctx, PlanStatus::default())
            .await;
        assert!(decision.continuation.is_none());
        assert!(!decision.record_continuation);
        assert!(service.snapshot().is_none());
    }
}

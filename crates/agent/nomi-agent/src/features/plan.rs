//! Plan mode as a self-registering feature.
//!
//! The feature owns plan-mode state (phase, pre-plan allow-list, pending plan,
//! exit latch), contributes the `EnterPlanMode` / `ExitPlanMode` tools, and
//! delivers its workflow instructions through the reminder channel.
//!
//! Extraction is "move + delegate": the workflow text and the tools are the
//! same code that previously lived in `crate::plan`, and every hardening gate
//! (verifiable-plan check, exit latch, read-only dispatch gate) is unchanged.

use std::sync::Arc;

use nomi_protocol::events::ToolCategory;
use nomi_tools::registry::ToolRegistry;

use super::{
    DispatchCtx, FacadeCtx, Feature, FeatureHooks, GateFn, ModifierFn, PlanStatus, RequestGateFn,
    Shared, SharedFlag, ToolDenial, UserRequestFn,
};

pub mod file;
pub mod prompt;
pub mod state;
pub mod tools;

pub use state::{PlanPhase, PlanState};
pub use tools::{EnterPlanModeTool, ExitPlanModeTool};

/// Why the read-only dispatch gate refuses a call.
///
/// Every non-`Info` tool is refused while plan mode is active. The tool table
/// is still *advertised* unchanged — refusing at dispatch instead of rewriting
/// the tool list is what keeps the provider's prefix cache warm.
fn read_only_denial(tool_name: &str) -> String {
    format!(
        "Plan mode is read-only. Tool '{tool_name}' was not executed. Use ExitPlanMode when the plan is ready."
    )
}

/// The two tool-visible flags, in their own slot so the service can adopt a
/// host-created handle after construction.
#[derive(Debug, Default)]
struct PlanFlags {
    active: SharedFlag,
    exit_latch: SharedFlag,
}

/// Plan-mode state, shared between the feature and its tools.
///
/// The two flags are the tool-visible half of the state: a tool reads them to
/// validate a transition (`Enter` twice, `Exit` after `Exit`) without taking
/// this lock. Mirroring state onto them is the service's job, never a caller's.
#[derive(Debug)]
pub struct PlanService {
    state: Shared<PlanState>,
    flags: Shared<PlanFlags>,
}

impl Default for PlanService {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanService {
    pub fn new() -> Self {
        Self {
            state: Shared::new(PlanState::default()),
            flags: Shared::new(PlanFlags::default()),
        }
    }

    pub fn snapshot(&self) -> PlanState {
        self.state.snapshot()
    }

    pub fn is_active(&self) -> bool {
        self.state.with(|state| state.is_active)
    }

    pub fn awaiting_approval(&self) -> bool {
        self.state.with(|state| state.awaiting_approval())
    }

    /// Read-only status snapshot for other features and for the engine's
    /// context-usage accounting.
    pub fn status(&self) -> PlanStatus {
        self.state.with(|state| PlanStatus {
            active: state.is_active,
            awaiting_approval: state.awaiting_approval(),
        })
    }

    /// The flag the plan tools must share. Handing out this exact handle is
    /// what makes the tools observe state changes made through the engine.
    pub fn active_flag(&self) -> SharedFlag {
        self.flags.with(|flags| flags.active.clone())
    }

    pub fn exit_latch(&self) -> SharedFlag {
        self.flags.with(|flags| flags.exit_latch.clone())
    }

    /// Adopt an externally created active flag (façade compatibility): the
    /// engine's `set_plan_active_flag` must keep accepting a host-created flag,
    /// and every reader has to end up on the same `Arc<AtomicBool>`.
    pub fn adopt_active_flag(&self, flag: SharedFlag) {
        flag.set(self.is_active());
        self.flags.with_mut(|flags| flags.active = flag);
    }

    pub fn adopt_exit_latch(&self, latch: SharedFlag) {
        latch.set(self.awaiting_approval());
        self.flags.with_mut(|flags| flags.exit_latch = latch);
    }

    /// Enter plan mode: snapshot the allow-list, become active, publish the
    /// tool-visible flags.
    pub fn enter(&self, pre_plan_allow_list: Vec<String>) {
        self.state.with_mut(|state| {
            state.pre_plan_allow_list = pre_plan_allow_list;
            state.is_active = true;
            state.phase = PlanPhase::Exploring;
            state.pending_plan = None;
        });
        self.publish();
    }

    /// Latch a submitted plan. Writes stay locked; the next user message
    /// approves it.
    pub fn latch_exit(&self, plan_content: Option<String>) {
        self.state.with_mut(|state| {
            state.phase = PlanPhase::AwaitingApproval;
            state.pending_plan = plan_content;
            state.is_active = true;
        });
        self.publish();
    }

    /// Cursor-style Build: the next user message after a latched plan restores
    /// write tools. Returns the allow-list to restore, or `None` when no plan
    /// was waiting for approval.
    pub fn approve_pending(&self) -> Option<Vec<String>> {
        let mut restored = None;
        self.state.with_mut(|state| {
            if !state.awaiting_approval() {
                return;
            }
            state.phase = PlanPhase::Idle;
            state.pending_plan = None;
            state.is_active = false;
            restored = Some(state.pre_plan_allow_list.clone());
        });
        if restored.is_some() {
            self.publish();
        }
        restored
    }

    /// Mirror the current state onto the tool-visible flags.
    fn publish(&self) {
        let (active, latch) = self.flags.with(|flags| (flags.active.clone(), flags.exit_latch.clone()));
        active.set(self.is_active());
        latch.set(self.awaiting_approval());
    }
}

/// Plan mode as an engine feature.
pub struct PlanFeature {
    service: Arc<PlanService>,
}

impl Default for PlanFeature {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanFeature {
    pub fn new() -> Self {
        Self {
            service: Arc::new(PlanService::new()),
        }
    }

    pub fn service(&self) -> &Arc<PlanService> {
        &self.service
    }
}

impl Feature for PlanFeature {
    fn name(&self) -> &'static str {
        "plan"
    }

    fn register_tools(&self, registry: &mut ToolRegistry) {
        // Register the shared handles this service publishes onto — and only
        // when no plan tool is present yet, so bootstrap and the feature cannot
        // end up with two independent flag pairs.
        tools::register_plan_tools(registry, &self.service);
    }

    fn hooks(&self) -> FeatureHooks {
        let service = Arc::clone(&self.service);

        // The next root user message approves a latched plan.
        let on_user_request: UserRequestFn = Arc::new(move |_facade| {
            let _ = &service;
        });

        let dispatch_gate: GateFn = Arc::new(|dispatch: &DispatchCtx, facade: FacadeCtx| {
            if !facade.plan_status.active {
                return None;
            }
            if dispatch.category == ToolCategory::Info {
                return None;
            }
            Some(ToolDenial::new(read_only_denial(&dispatch.tool_name)))
        });

        // Placeholder until the extraction phase moves the allow-list snapshot
        // and restore into this feature.
        let apply_modifier: ModifierFn = Arc::new(|_modifier, ctx| ctx);

        // Placeholder until the extraction phase moves the "plan mode blocks a
        // thinking/effort downgrade" rule here.
        let request_gates: RequestGateFn = Arc::new(|params, _facade| params);

        FeatureHooks {
            on_user_request: vec![on_user_request],
            dispatch_gate: vec![dispatch_gate],
            apply_modifier: vec![apply_modifier],
            request_gates: vec![request_gates],
            ..FeatureHooks::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_snapshots_the_allow_list_and_publishes_the_flag() {
        let service = PlanService::new();
        let flag = service.active_flag();
        assert!(!flag.get());
        assert!(!service.is_active());

        service.enter(vec!["Read".into(), "Bash".into()]);

        assert!(service.is_active());
        assert!(flag.get(), "the shared flag mirrors the entered state");
        assert_eq!(service.snapshot().phase, PlanPhase::Exploring);
        assert_eq!(
            service.snapshot().pre_plan_allow_list,
            vec!["Read".to_string(), "Bash".to_string()]
        );
        assert!(service.snapshot().pending_plan.is_none());
    }

    #[test]
    fn exit_latches_without_restoring_writes() {
        let service = PlanService::new();
        let latch = service.exit_latch();
        service.enter(vec!["Read".into()]);
        assert!(!latch.get());

        service.latch_exit(Some("plan text".into()));

        assert!(service.awaiting_approval());
        assert!(service.is_active(), "writes stay locked until approval");
        assert!(latch.get());
        assert_eq!(service.snapshot().pending_plan.as_deref(), Some("plan text"));
    }

    #[test]
    fn approval_restores_the_snapshot_and_clears_both_flags() {
        let service = PlanService::new();
        let active = service.active_flag();
        let latch = service.exit_latch();
        service.enter(vec!["Read".into(), "Bash".into()]);
        service.latch_exit(Some("plan text".into()));

        let restored = service.approve_pending();

        assert_eq!(
            restored,
            Some(vec!["Read".to_string(), "Bash".to_string()])
        );
        assert!(!service.is_active());
        assert_eq!(service.snapshot().phase, PlanPhase::Idle);
        assert!(service.snapshot().pending_plan.is_none());
        assert!(!active.get());
        assert!(!latch.get());
    }

    #[test]
    fn approving_without_a_latched_plan_is_a_noop() {
        let service = PlanService::new();
        assert_eq!(service.approve_pending(), None);
        assert!(!service.is_active());
    }

    #[test]
    fn status_reports_both_plan_facts() {
        let service = PlanService::new();
        assert_eq!(service.status(), PlanStatus::default());

        service.enter(vec![]);
        assert_eq!(
            service.status(),
            PlanStatus {
                active: true,
                awaiting_approval: false
            }
        );

        service.latch_exit(None);
        assert_eq!(
            service.status(),
            PlanStatus {
                active: true,
                awaiting_approval: true
            }
        );
    }

    #[test]
    fn adopted_flags_are_the_ones_read_by_tools() {
        // Bootstrap creates the flags it hands to the tools first, then adopts
        // them; the service must publish onto those exact handles.
        let service = PlanService::new();
        let tool_flag = SharedFlag::new(false);
        let tool_latch = SharedFlag::new(false);
        service.adopt_active_flag(tool_flag.clone());
        service.adopt_exit_latch(tool_latch.clone());

        service.enter(vec![]);
        assert!(tool_flag.get(), "the tool's handle observes the entry");

        service.latch_exit(None);
        assert!(tool_latch.get());
    }

    fn feature_hooks() -> FeatureHooks {
        PlanFeature::new().hooks()
    }

    #[test]
    fn dispatch_gate_refuses_non_info_only_while_active() {
        let hooks = feature_hooks();
        let gate = &hooks.dispatch_gate[0];

        let write = DispatchCtx::new("Write", ToolCategory::Edit);
        let inactive = FacadeCtx::default();
        assert!(
            gate(&write, inactive).is_none(),
            "outside plan mode nothing is refused"
        );

        let active = FacadeCtx {
            plan_status: PlanStatus {
                active: true,
                awaiting_approval: false,
            },
        };
        let denial = gate(&write, active).expect("a writer is refused in plan mode");
        assert!(denial.message.contains("Plan mode is read-only"));
        assert!(denial.message.contains("ExitPlanMode"));

        let read = DispatchCtx::new("Read", ToolCategory::Info);
        assert!(
            gate(&read, active).is_none(),
            "read-only tools still dispatch in plan mode"
        );
    }

    #[test]
    fn reported_name_is_stable() {
        assert_eq!(PlanFeature::new().name(), "plan");
    }
}

//! Feature seam: a self-registering contribution channel for cross-cutting
//! agent modes (plan, goal, …) so the engine main loop carries hooks instead of
//! mode-specific fields and branches.
//!
//! Ported (to Rust, without a DI container) from kimi-code
//! `packages/agent-core-v2/src/features/`. A [`Feature`] contributes three
//! things: tools ([`Feature::register_tools`]), reminders
//! ([`Feature::reminders`]) and lifecycle hooks ([`Feature::hooks`]).
//!
//! Conventions (frozen — see `docs/architecture/plan-goal-feature-seam.zh.md` §3.2):
//!
//! - [`FeatureRegistry`] lives on the engine;
//!   [`crate::bootstrap::AgentBootstrap`] registers the features a session is
//!   entitled to.
//! - Hooks run in **registration order**. Sync hooks run inline; the two that
//!   genuinely await (tool-turn observation and natural-end evaluation) are
//!   awaited in the existing async turn loop. No async-trait machinery is used:
//!   each hook is an `Arc<dyn Fn …>` closure owned by the feature that captures
//!   its own state.
//! - Every engine call site is a single generic fold over the registry. No call
//!   site may branch on a feature's name.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use nomi_protocol::events::ToolCategory;
use nomi_tools::registry::ToolRegistry;
use nomi_types::skill_types::ContextModifier;

mod goal;
// Plan mode keeps a second, compatibility path at `crate::plan` so existing
// hosts and integration tests resolve unchanged; the implementation lives here.
pub mod plan;

pub use goal::GoalFeature;
pub use plan::PlanFeature;

// ---------------------------------------------------------------------------
// Context and result types
//
// These are the *narrow* interfaces a feature is allowed to see. Engine
// internals (allow-list, messages, ledger, …) stay in the engine and cross the
// boundary as owned values in and owned results out, so no hook can reach into
// the engine's private state.
// ---------------------------------------------------------------------------

/// Read-only snapshot of plan-mode status, handed to every other feature so a
/// feature can react to plan mode without the engine (or that feature) naming
/// `PlanFeature`.
///
/// The default is the inactive shape, which is also what a session without a
/// plan feature reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanStatus {
    pub active: bool,
    pub awaiting_approval: bool,
}

/// Read-only facts a feature may consult while a turn is running.
#[derive(Debug, Clone, Copy, Default)]
pub struct FacadeCtx {
    pub plan_status: PlanStatus,
}

/// One provider pass's worth of request parameters a gate may adjust.
#[derive(Debug, Clone, Default)]
pub struct RequestParams {
    /// `LlmRequest.thinking`, or `None` when the session has no thinking config.
    pub thinking: Option<nomi_types::llm::ThinkingConfig>,
    /// `LlmRequest.reasoning_effort`, or `None` when unset.
    pub reasoning_effort: Option<String>,
    /// Whether a previous pass in this turn already produced tool results.
    /// Engine housekeeping a downgrade gate may consult.
    pub continuation_after_tools: bool,
}

/// One tool call about to be dispatched.
#[derive(Debug, Clone)]
pub struct DispatchCtx {
    pub tool_name: String,
    /// Category resolved against the call's input (a tool may be category
    /// dependent), defaulting to [`ToolCategory::Info`] for an unknown tool.
    pub category: ToolCategory,
}

impl DispatchCtx {
    pub fn new(tool_name: impl Into<String>, category: ToolCategory) -> Self {
        Self {
            tool_name: tool_name.into(),
            category,
        }
    }
}

/// A feature's refusal to dispatch one tool call.
///
/// The refusal is an observation the engine turns into a tool-result error
/// block; it never reaches the tool, the confirmer, the approval UI or the
/// execution hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDenial {
    pub message: String,
}

impl ToolDenial {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// The engine slots a context modifier may read and rewrite, threaded through
/// [`ModifierFn`] so a feature can migrate its own state from a tool-produced
/// [`ContextModifier`] without the engine knowing what that modifier means.
#[derive(Debug, Clone, Default)]
pub struct ModifierCtx {
    /// Session-wide tool allow-list; features may push names into it.
    pub allow_list: Vec<String>,
}

/// One tool turn's observations, offered to every feature.
///
/// `name`/`command`/`success` are raw tool-result facts; interpreting them is
/// the feature's job.
#[derive(Debug, Clone, Default)]
pub struct ToolTurn {
    pub calls: Vec<ToolCallObservation>,
}

#[derive(Debug, Clone)]
pub struct ToolCallObservation {
    pub name: String,
    pub command: Option<String>,
    pub success: bool,
}

/// Per-request facts a feature needs when deciding whether to continue a turn
/// after the model stopped naturally.
#[derive(Debug, Clone, Default)]
pub struct NaturalEndCtx {
    pub assistant_text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// What a feature asks the engine to do at a natural termination point.
///
/// The engine owns message construction, persistence and the turn counter; a
/// feature only supplies the message and whether the horizon's continuation
/// budget should be charged for it.
#[derive(Debug, Clone, Default)]
pub struct NaturalEndDecision {
    pub continuation: Option<String>,
    pub record_continuation: bool,
}

impl NaturalEndDecision {
    /// Stop; no continuation.
    pub fn stop() -> Self {
        Self::default()
    }

    /// Continue this turn with `text` as the next user message.
    pub fn continue_with(text: impl Into<String>, record_continuation: bool) -> Self {
        Self {
            continuation: Some(text.into()),
            record_continuation,
        }
    }
}

// ---------------------------------------------------------------------------
// Hook signatures
//
// `Send + Sync + 'static` closures held in `Arc`, so [`FeatureRegistry`] stays
// `Clone` and every engine call site is a plain fold. The two hooks that await
// return `BoxFuture`; the rest run inline.
// ---------------------------------------------------------------------------

/// A new root user request is about to start.
pub type UserRequestFn = Arc<dyn Fn(FacadeCtx) + Send + Sync + 'static>;

/// May refuse one tool call before dispatch.
pub type GateFn =
    Arc<dyn Fn(&DispatchCtx, FacadeCtx) -> Option<ToolDenial> + Send + Sync + 'static>;

/// Migrate one tool-produced [`ContextModifier`] into feature state.
pub type ModifierFn =
    Arc<dyn Fn(&ContextModifier, ModifierCtx) -> ModifierCtx + Send + Sync + 'static>;

/// Adjust this provider pass's request parameters.
pub type RequestGateFn =
    Arc<dyn Fn(RequestParams, FacadeCtx) -> RequestParams + Send + Sync + 'static>;

/// Observe one finished tool turn. Awaited: a feature may need the provider.
pub type ToolTurnFn = Arc<
    dyn Fn(ToolTurn, FacadeCtx) -> futures::future::BoxFuture<'static, ()> + Send + Sync + 'static,
>;

/// Evaluate whether this turn should continue after a natural stop. Awaited:
/// the goal feature calls an external judge here.
pub type NaturalEndFn = Arc<
    dyn Fn(NaturalEndCtx, FacadeCtx) -> futures::future::BoxFuture<'static, NaturalEndDecision>
        + Send
        + Sync
        + 'static,
>;

/// The hook set a feature contributes. Every field is optional; a feature that
/// contributes no hooks returns [`FeatureHooks::default`].
#[derive(Clone, Default)]
pub struct FeatureHooks {
    pub on_user_request: Vec<UserRequestFn>,
    /// First denial in registration order wins.
    pub dispatch_gate: Vec<GateFn>,
    /// Chained in registration order; each receives the previous result.
    pub apply_modifier: Vec<ModifierFn>,
    /// Chained in registration order.
    pub request_gates: Vec<RequestGateFn>,
    pub on_tool_turn: Vec<ToolTurnFn>,
    pub on_natural_end: Vec<NaturalEndFn>,
}

/// When a reminder is being rendered.
///
/// `pass_in_turn` counts provider passes since the current turn started
/// (0-based); `turn_start` is true only on the first pass of a turn.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReminderTrigger {
    pub turn_start: bool,
    pub pass_in_turn: usize,
    /// A new root user message arrived since the previous rendering.
    pub new_user_message: bool,
}

/// A reminder variant a feature wants delivered through
/// [`crate::features::reminder::ReminderService`].
#[derive(Clone)]
pub struct ReminderSpec {
    pub variant: &'static str,
    pub render: Arc<dyn Fn(&ReminderTrigger) -> Option<String> + Send + Sync + 'static>,
}

/// One self-registering agent mode.
pub trait Feature: Send + Sync {
    /// Stable name, used for registration diagnostics.
    fn name(&self) -> &'static str;

    /// Contribute tools to the session registry.
    fn register_tools(&self, _registry: &mut ToolRegistry) {}

    /// Reminder variants this feature wants rendered into `<system-reminder>`
    /// user messages.
    fn reminders(&self) -> Vec<ReminderSpec> {
        Vec::new()
    }

    fn hooks(&self) -> FeatureHooks {
        FeatureHooks::default()
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FeatureEntry {
    pub name: &'static str,
    pub feature: Arc<dyn Feature>,
    pub hooks: FeatureHooks,
}

/// Ordered set of features. Immutable after registration, so the engine can
/// fold it while holding no lock.
///
/// Cloning shares the same feature instances (they are `Arc`), which is what
/// lets a host-side handle keep operating on a live feature.
#[derive(Clone, Default)]
pub struct FeatureRegistry {
    entries: Vec<FeatureEntry>,
}

impl FeatureRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Register a feature and capture its hooks.
    ///
    /// Idempotent per feature name: re-registering a name replaces the earlier
    /// entry in place, so a bootstrap that runs twice cannot double-invoke a
    /// hook or double-register a tool.
    pub fn register(&mut self, feature: Arc<dyn Feature>) {
        let entry = FeatureEntry {
            name: feature.name(),
            hooks: feature.hooks(),
            feature,
        };
        match self.entries.iter_mut().find(|e| e.name == entry.name) {
            Some(slot) => *slot = entry,
            None => self.entries.push(entry),
        }
    }

    pub fn entries(&self) -> &[FeatureEntry] {
        &self.entries
    }

    /// The registered feature with this name.
    pub fn feature(&self, name: &str) -> Option<&Arc<dyn Feature>> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| &entry.feature)
    }

    /// Register each feature's tools, in registration order.
    pub fn register_tools(&self, registry: &mut ToolRegistry) {
        for entry in &self.entries {
            entry.feature.register_tools(registry);
        }
    }

    /// All reminder specs, flattened in registration order.
    pub fn reminders(&self) -> Vec<ReminderSpec> {
        self.entries
            .iter()
            .flat_map(|entry| entry.feature.reminders())
            .collect()
    }

    // -- hook folds ---------------------------------------------------------
    //
    // Every engine call site uses exactly one of these. They are deliberately
    // the only code that knows the hook shape: a call site never inspects which
    // feature it is driving.

    pub fn run_user_request(&self, facade: FacadeCtx) {
        for entry in &self.entries {
            for hook in &entry.hooks.on_user_request {
                hook(facade);
            }
        }
    }

    /// First denial in registration order wins; `None` means every gate allowed.
    pub fn first_denial(&self, dispatch: &DispatchCtx, facade: FacadeCtx) -> Option<ToolDenial> {
        for entry in &self.entries {
            for gate in &entry.hooks.dispatch_gate {
                if let Some(denial) = gate(dispatch, facade) {
                    return Some(denial);
                }
            }
        }
        None
    }

    /// Fold modifiers in registration order. Each feature sees the previous
    /// feature's result, so a later feature observes an earlier one's writes.
    pub fn apply_modifiers(
        &self,
        modifiers: &[Option<ContextModifier>],
        mut ctx: ModifierCtx,
    ) -> ModifierCtx {
        for modifier in modifiers.iter().flatten() {
            for entry in &self.entries {
                for hook in &entry.hooks.apply_modifier {
                    ctx = hook(modifier, ctx);
                }
            }
        }
        ctx
    }

    /// Fold request parameters in registration order.
    pub fn apply_request_gates(&self, mut params: RequestParams, facade: FacadeCtx) -> RequestParams {
        for entry in &self.entries {
            for gate in &entry.hooks.request_gates {
                params = gate(params, facade);
            }
        }
        params
    }

    pub async fn observe_tool_turn(&self, turn: ToolTurn, facade: FacadeCtx) {
        for entry in &self.entries {
            for hook in &entry.hooks.on_tool_turn {
                hook(turn.clone(), facade).await;
            }
        }
    }

    /// Chain natural-end decisions.
    ///
    /// Every hook is consulted even after one requests a continuation, because
    /// a goal's own bookkeeping must run regardless of whether another feature
    /// already decided to keep the turn alive. The first continuation wins; the
    /// `record_continuation` flags are OR-ed.
    pub async fn resolve_natural_end(
        &self,
        ctx: &NaturalEndCtx,
        facade: FacadeCtx,
    ) -> NaturalEndDecision {
        let mut combined = NaturalEndDecision::default();
        for entry in &self.entries {
            for hook in &entry.hooks.on_natural_end {
                let decision = hook(ctx.clone(), facade).await;
                combined.record_continuation |= decision.record_continuation;
                if combined.continuation.is_none() {
                    combined.continuation = decision.continuation;
                }
            }
        }
        combined
    }
}

impl std::fmt::Debug for FeatureRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeatureRegistry")
            .field(
                "features",
                &self.entries.iter().map(|e| e.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Shared handles
// ---------------------------------------------------------------------------

/// A boolean flag shared with a tool.
///
/// The feature service and its tools each hold a clone, so a tool can validate
/// a transition without a lock while the engine observes the value on its next
/// fold. It is a transparent wrapper over `Arc<AtomicBool>`: the engine's
/// façade and the existing hosts keep passing raw `Arc<AtomicBool>`.
#[derive(Debug, Clone, Default)]
pub struct SharedFlag(Arc<AtomicBool>);

impl SharedFlag {
    pub fn new(value: bool) -> Self {
        Self(Arc::new(AtomicBool::new(value)))
    }

    pub fn get(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn set(&self, value: bool) {
        self.0.store(value, Ordering::Release);
    }

    /// The underlying handle, so a caller that still holds an `Arc<AtomicBool>`
    /// (or a tool that takes one) shares this exact flag.
    pub fn as_arc(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
}

impl From<Arc<AtomicBool>> for SharedFlag {
    fn from(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }
}

/// A mutex-guarded slot shared between a feature service and its tools.
///
/// Poisoning is a hard programming error rather than a silent skip: every
/// writer is a short, panic-free critical section.
#[derive(Debug, Default)]
pub struct Shared<T>(Mutex<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.0.lock().expect("feature state lock poisoned"))
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut self.0.lock().expect("feature state lock poisoned"))
    }
}

impl<T: Clone> Shared<T> {
    pub fn snapshot(&self) -> T {
        self.with(Clone::clone)
    }
}

#[cfg(test)]
mod tests;

mod drawer;
mod nav;
mod submit;

use crate::composer::{ChatComposer, ComposerEvent};
use crate::drawer_preview::DrawerPreviewState;
use crate::feed::{
    ArtifactAction, ChatFeed, OpenDeliverables, OpenTask, QuoteMessage,
    RevealFile, WorkingIndicator, WorkingIndicatorEvent,
};
use flowy_agent::{
    AgentHandle, LlmConfig, Model, Plan, ProviderCatalog, ReceiptState,
    SessionOrigin,
};
use flowy_core::DocumentSession;
use flowy_i18n::I18n;
use flowy_ui::{ResolvedTheme, UiContext};
use gpui::{
    AppContext, Context, Entity, EventEmitter, IntoElement, Render, Task,
    Window,
};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

/// Which workspace surface the panel should show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudioSurface {
    Home,
    Chat,
    Board,
    Automation,
    Placeholder {
        title_key: &'static str,
        detail_key: &'static str,
    },
}

/// One rendered tier row.
///
/// Built by `flowy-app` from the local tier directory plus the current
/// catalog/entitlement, so the menu never inspects raw model ids or account
/// data itself. Labels are resolved from the row's tier at render time so a
/// locale switch is reflected immediately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierView {
    pub tier: Model,
    pub pro_badge: bool,
    pub locked: bool,
    pub unavailable: bool,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub enum StudioEvent {
    Notice(Option<&'static str>),
    Automation(Box<crate::AutomationUiAction>),
    ModelChanged {
        model: Model,
        plan: Plan,
        model_info_version: String,
    },
    OpenFlex,
    TierSelected(Model),
    RefreshModels,
    OpenIdeas,
    SessionsChanged,
    SkillsChanged,
    /// A turn reached a terminal state; credit balance may have changed.
    TurnFinished,
    ArtifactsChanged,
    PreviewArtifact(String),
    PreviewLocalFile(String),
    SaveArtifact(String),
    LocateArtifactSource {
        session_id: String,
        artifact_id: String,
    },
    /// Requests the host's unified session context menu at an explicit pointer
    /// position (right-click on Progress Board session rows).
    OpenSessionMenu {
        session_id: String,
        position: gpui::Point<gpui::Pixels>,
    },
    /// Requests the host to toggle the unified session context menu anchored
    /// to a measured `anchor_trigger` button on a Progress Board session row.
    ToggleSessionMenu {
        session_id: String,
        anchor_id: String,
    },
}

/// Domain inputs used when constructing a workspace. Window UI services are supplied separately.
pub struct StudioConfig {
    pub product: &'static str,
    pub i18n: I18n,
    pub model: Model,
    pub plan: Plan,
    pub model_info_version: String,
    pub model_info_open: bool,
    pub store_root: PathBuf,
    pub llm: LlmConfig,
}

impl std::fmt::Debug for StudioConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StudioConfig")
            .field("product", &self.product)
            .field("model", &self.model)
            .field("plan", &self.plan)
            .finish_non_exhaustive()
    }
}

/// A failed or waiting turn keeps its localized status line visible until resolved.
const fn working_line_visible(status: &flowy_agent::TurnStatus) -> bool {
    status.is_busy()
        || matches!(
            status,
            flowy_agent::TurnStatus::WaitingForUser
                | flowy_agent::TurnStatus::Error { .. }
                | flowy_agent::TurnStatus::DeliveryFailed
        )
}

/// Mutually exclusive transient popover open on [`StudioPanel`].
///
/// Encoding popovers as a single enum guarantees two cockpit menus can never
/// overlap visually or drift out of sync with `OverlayHost`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActivePopover {
    #[default]
    None,
    ModelInfo,
    ModelMenu,
    BgTasks,
    EnvStatus,
    AddMenu,
    Credits,
}

impl ActivePopover {
    #[must_use]
    pub const fn scope(self) -> Option<&'static str> {
        match self {
            // Closed state and information-only Credits hover card must not
            // claim popup focus scope (prevents hover focus theft).
            Self::None | Self::Credits => None,
            Self::ModelMenu => Some("model-list"),
            Self::ModelInfo => Some("model-info"),
            Self::AddMenu => Some("composer-add-menu"),
            Self::EnvStatus => Some("env-status-popover"),
            Self::BgTasks => Some("bg-tasks-popover"),
        }
    }
}

/// Self-contained UI state for the Master-Detail Board Inspector / narrow slide drawer.
#[derive(Clone, Debug, Default)]
pub struct BoardInspectorState {
    pub session: Option<String>,
    pub branch: Option<String>,
    pub worker_session: Option<String>,
    pub tab: crate::overview::BoardInspectorTab,
    pub drawer_open: bool,
    pub workspace_changes_expanded: bool,
    /// Bound to `"{report_id}:{artifact_path}"` so background snapshot refreshes
    /// that switch branches never carry an override acknowledgement onto another
    /// artifact (§8.3).
    pub override_ack_key: Option<String>,
}

impl BoardInspectorState {
    pub const fn close_drawer(&mut self) {
        self.drawer_open = false;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub const fn has_drawer(&self) -> bool {
        self.drawer_open && self.session.is_some()
    }
}

pub struct StudioPanel {
    pub ui: UiContext,
    pub theme: ResolvedTheme,
    pub prompt: Entity<gpui_kit::base::input::TextareaState>,
    pub(crate) feed: Entity<ChatFeed>,
    pub(crate) composer: Entity<ChatComposer>,
    /// Messages the host took responsibility for but has not run yet.
    ///
    /// Backed by the acceptance journal, not by this field: it is re-read from
    /// disk, so a restart still shows what was accepted.
    pub(crate) pending_receipts: Vec<flowy_agent::AcceptedMessage>,
    /// All non-terminal acceptance receipts (`Queued`, `Running`, `Retrying`,
    /// `Quarantined`, `Dead`) projected directly from
    /// `AgentHandle::visible_acceptances()`, surviving restarts cleanly.
    pub(crate) visible_receipts:
        Vec<(flowy_agent::AcceptedMessage, ReceiptState)>,
    /// Receipts currently being requeued via `CardAction::RetryDeadReceipt`.
    pub(crate) retrying_receipts: std::collections::HashSet<String>,
    /// Verification report IDs currently executing `CardAction::OverrideDeliver`
    /// on a background worker (prevents double-click re-entry).
    pub(crate) overriding_reports: std::collections::HashSet<String>,
    /// Multi-select object ids ticked per `(session_id, card_id)`.
    pub(crate) pending_card_selected:
        std::collections::HashMap<(String, String), Vec<String>>,
    /// `(session_id, card_id)` keys currently resolving via `CardAction::ResolveCard`.
    pub(crate) resolving_cards: std::collections::HashSet<(String, String)>,
    _composer_sub: gpui::Subscription,
    _feed_sub: gpui::Subscription,
    _reveal_sub: gpui::Subscription,
    _deliverables_sub: gpui::Subscription,
    _artifact_sub: gpui::Subscription,
    _working_sub: gpui::Subscription,
    submission: Option<u128>,
    pub(crate) submission_pending: bool,
    submission_generation: u64,
    pub(crate) submission_stop: Option<Arc<AtomicBool>>,
    /// Session whose durable delivery marker is currently being retried.
    pub(crate) retrying_delivery: Option<String>,
    pub(crate) working: Entity<WorkingIndicator>,
    pub(crate) status: flowy_agent::TurnStatus,
    last_elapsed: Option<u64>,
    /// Set once the agent reports its first `Initialized` update.
    initialized: bool,
    /// Throttles the level-triggered drain check in the pump.
    last_drain_check: Option<std::time::Instant>,
    _theme_sub: gpui::Subscription,
    _overlay_sub: gpui::Subscription,
    pub projects: Vec<crate::projects::Project>,
    pub active_project_id: Option<String>,
    project_store_failed: bool,
    pub document: DocumentSession,
    pub agent: AgentHandle,
    pub credits: crate::CreditsViewState,
    pub(crate) popover: ActivePopover,
    pub(crate) credits_close: Option<Task<()>>,
    pub(crate) board_snapshot: crate::ports::WorkBoardSnapshot,
    /// Event-driven cached Router child-Works / background-tasks snapshot so
    /// `Render::render` never scans `AgentHandle` locks per frame.
    pub(crate) popover_works: crate::router_works::PopoverWorksSnapshot,
    pub(crate) local_env: crate::ports::LocalEnvSnapshot,
    pub(crate) board_filter: crate::overview::BoardFilter,
    pub(crate) board_project_filter: Option<String>,
    pub(crate) board_day_filter: Option<crate::ports::LocalDateKey>,
    pub(crate) board_format_filter: Option<crate::ports::DeliverableFormat>,
    pub(crate) board_older_expanded: bool,
    pub(crate) board_width_px: f32,
    pub(crate) board_inspector: BoardInspectorState,
    pub(crate) trace_filter: crate::overview::TraceFilter,
    /// Dirty-cached structured chat lines for the currently inspected Board
    /// session/worker so `Render::render` never scans `AgentHandle` locks.
    pub(crate) cached_board_lines: Option<(String, Vec<flowy_agent::ChatLine>)>,
    /// Dirty-cached redacted traces for the currently open Trace tab so
    /// `Render::render` never touches kernel journal files on disk.
    pub(crate) cached_trace:
        Option<(String, Vec<crate::ports::RedactedTraceEntry>)>,
    pub(crate) drawer_tab: crate::deliverables::RightDrawerTab,
    pub(crate) right_drawer_open: bool,
    pub(crate) selected_skill: Option<crate::ports::SkillChip>,
    pub(crate) installed_skills: Vec<crate::ports::SkillChip>,
    pub(crate) board_generation: u64,
    pub(crate) last_board_refresh: Option<std::time::Instant>,
    pub(crate) last_popover_works_refresh: Option<std::time::Instant>,
    pub(crate) last_trace_refresh: Option<std::time::Instant>,
    pub tiers: Vec<TierView>,
    pub i18n: I18n,
    pub model: Model,
    pub catalog: ProviderCatalog,
    pub cloud_ready: bool,
    pub catalog_loading: bool,
    pub plan: Plan,
    pub model_info_version: String,
    pub store_root: PathBuf,
    pub product: &'static str,
    pub surface: StudioSurface,
    pub(crate) automation: crate::automation::AutomationDraft,
    pub(crate) automation_page: Option<crate::AutomationPage>,
    pub(crate) automation_editor:
        Option<crate::automation_page::DefinitionEditor>,
    pub(crate) automation_confirm: Option<flowy_automation::AutomationId>,
    pub notice: Option<&'static str>,
    /// Set once the user opens a conversation in this run. Startup initialization
    /// must not pull the app back into the last thread, but a session restored
    /// because the user asked for it should land on Chat.
    pub(crate) session_opened_by_user: bool,
    /// Files the deliverables drawer lists; `None` keeps it empty or closed.
    pub(crate) deliverables: Option<crate::deliverables::DeliverableScope>,
    /// Single background work or queued receipt inspected in the right sheet.
    pub(crate) inspected_work: Option<crate::deliverables::InspectedWork>,
    /// Asynchronous rich preview state for the right-hand deliverables drawer.
    pub(crate) drawer_preview: DrawerPreviewState,
    /// Scope filter for Workspace Files inside the Deliverables tab (`Latest` vs `All`).
    pub(crate) deliverables_view_scope:
        crate::deliverables::DeliverablesViewScope,
    /// Whether the user expanded the right drawer into its wide workbench width.
    pub(crate) drawer_wide: bool,
    /// Background-probed local file sizes keyed by canonical path.
    pub(crate) deliverable_sizes:
        std::collections::HashMap<String, Option<u64>>,
    /// Safe non-overlapping parent folder candidate eligible for "Save as project".
    pub(crate) promote_project_candidate: Option<String>,
    drawer_meta_generation: u64,
    drawer_meta_task: Option<Task<()>>,
    /// Per-session overrides flipping default expand state for step turn groups.
    pub(crate) step_turn_toggled:
        std::collections::HashMap<String, std::collections::HashSet<usize>>,
    /// Expanded individual step detail cards inside the Steps tab.
    pub(crate) expanded_step_items: std::collections::HashSet<String>,
    pub(crate) drawer_step_copied: Option<(String, u64)>,
    drawer_step_copy_epoch: u64,
    drawer_step_copy_task: Option<Task<()>>,
    /// Mirrors shell sidebar visibility for adaptive right-drawer width clamping.
    pub(crate) dock_visible: bool,
    /// Composer drafts keyed by session, so switching tasks does not leak text.
    drafts: std::collections::HashMap<String, String>,
    /// Single active WeChat-style reply quote shown above the composer.
    pub(crate) active_quote: Option<crate::composer::ActiveQuote>,
    /// Session id that `active_quote` currently belongs to (`None` for Home draft).
    quote_session: Option<String>,
    /// Per-session quoted reply drafts so switching tasks keeps quotes isolated.
    draft_quotes:
        std::collections::HashMap<String, crate::composer::ActiveQuote>,
    _quote_sub: gpui::Subscription,
    /// Prompt text waiting for the next render, which is the first point after
    /// most callers where a `Window` is available to install it.
    prompt_pending: Option<String>,
    _prompt_sub: gpui::Subscription,
    pump: Option<Task<()>>,
}

impl std::fmt::Debug for StudioPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StudioPanel")
            .field("surface", &self.surface)
            .field("model", &self.model)
            .field("plan", &self.plan)
            .field("status", &self.status)
            .field("theme", &self.theme)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<StudioEvent> for StudioPanel {}

impl StudioPanel {
    pub fn new(
        config: StudioConfig,
        ui: UiContext,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        Self::new_with_mode(config, ui, false, window, cx)
    }

    pub fn new_background(
        config: StudioConfig,
        ui: UiContext,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        Self::new_with_mode(config, ui, true, window, cx)
    }

    fn new_with_mode(
        config: StudioConfig,
        ui: UiContext,
        background: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        tracing::debug!(
            target: "flowy.boot",
            stage = "StudioPanel::new",
            store_root = %config.store_root.display(),
            ?config.model,
            ?config.plan,
            "enter"
        );
        let StudioConfig {
            product,
            i18n,
            model,
            plan,
            model_info_version,
            model_info_open,
            store_root,
            llm,
        } = config;
        // The placeholder has to be set through the generic setter: the
        // builder form only exists on the single-line state. Without it the
        // composer renders empty and screen readers get no name.
        let prompt_placeholder = i18n.t("chat.do_anything");
        let prompt = cx.new(|cx| {
            let mut state =
                gpui_kit::base::input::TextareaState::new(window, cx);
            state.set_placeholder(prompt_placeholder, window, cx);
            state
        });
        let prompt_sub = cx.subscribe(
            &prompt,
            |panel, _, _: &gpui_kit::base::input::InputEvent, cx| {
                if !panel.submission_pending {
                    panel.submission = None;
                }
                if panel.surface != StudioSurface::Chat {
                    cx.notify();
                }
            },
        );
        tracing::debug!(
            target: "flowy.boot",
            stage = "StudioPanel::new",
            "→ AgentHandle::with_config"
        );
        let agent = if background {
            let handle =
                AgentHandle::with_config_background(store_root.clone(), llm);
            handle.set_autonomous_drain(true);
            handle
        } else {
            AgentHandle::with_config(store_root.clone(), llm)
        };
        if !agent.is_initializing() && agent.active_session_id().is_none() {
            agent.set_workspace_for_new_sessions(None);
            agent.set_origin_for_new_sessions(SessionOrigin::Home);
        }
        // Startup always lands on a fresh task. The previous conversation stays
        // in the sidebar and is reopened on purpose, not by launching.
        let surface = StudioSurface::Home;
        tracing::debug!(
            target: "flowy.boot",
            stage = "StudioPanel::new",
            ?surface,
            "agent ready"
        );
        let theme = ui.theme.read(cx).resolved();
        let theme_sub = cx.observe(&ui.theme, |panel, entity, cx| {
            panel.theme = entity.read(cx).resolved();
            cx.notify();
        });
        let overlay_sub = cx.subscribe(
            &ui.overlays,
            |panel, _, event: &flowy_ui::OverlayEvent, cx| {
                let flowy_ui::OverlayEvent::CloseRequested { scope, .. } =
                    event;
                panel.dismiss_overlay(scope.as_ref(), cx);
            },
        );
        let feed = cx.new(|cx| {
            ChatFeed::new(
                ui.clone(),
                i18n.clone(),
                agent.active_session_id(),
                cx,
            )
        });
        let working = cx.new(|cx| WorkingIndicator::new(ui.clone(), cx));
        let working_sub = cx.subscribe(
            &working,
            |panel, _, event: &WorkingIndicatorEvent, cx| match event {
                WorkingIndicatorEvent::RetryDelivery => {
                    panel.retry_delivery(cx);
                }
            },
        );
        let feed_sub = cx.subscribe(&feed, |panel, _, event: &OpenTask, cx| {
            panel.open_session(&event.0, cx);
        });
        let quote_sub =
            cx.subscribe(&feed, |panel, _, event: &QuoteMessage, cx| {
                panel.set_active_quote(event.role, &event.text, cx);
            });
        let reveal_sub =
            cx.subscribe(&feed, |panel, _, event: &RevealFile, cx| {
                panel.reveal_file(&event.0, cx);
            });
        let deliverables_sub =
            cx.subscribe(&feed, |panel, _, event: &OpenDeliverables, cx| {
                panel.open_deliverables(event.0.clone(), cx);
            });
        let artifact_sub =
            cx.subscribe(&feed, |panel, _, event: &ArtifactAction, cx| {
                match event {
                    ArtifactAction::Preview(id) => {
                        cx.emit(StudioEvent::PreviewArtifact(id.clone()));
                    }
                    ArtifactAction::SaveAs(id) => {
                        cx.emit(StudioEvent::SaveArtifact(id.clone()));
                    }
                    ArtifactAction::LocateSource {
                        session_id,
                        artifact_id,
                    } => cx.emit(StudioEvent::LocateArtifactSource {
                        session_id: session_id.clone(),
                        artifact_id: artifact_id.clone(),
                    }),
                    ArtifactAction::OverrideDeliver {
                        session_id,
                        report_id,
                        artifact_path,
                    } => {
                        let target = session_id
                            .clone()
                            .or_else(|| panel.agent.active_session_id());
                        if let Some(owner) = target {
                            panel.confirm_override_for_session(
                                &owner,
                                report_id,
                                artifact_path,
                                cx,
                            );
                        } else {
                            panel.emit_notice(Some("chat.publish.context"), cx);
                        }
                    }
                }
            });
        let composer = cx.new(|cx| {
            ChatComposer::new(
                ui.clone(),
                prompt.clone(),
                i18n.clone(),
                model,
                cx,
            )
        });
        let composer_sub =
            cx.subscribe(&composer, |panel, _, event: &ComposerEvent, cx| {
                match event {
                    ComposerEvent::Send => {
                        tracing::debug!(
                            target: "flowy.prompt",
                            stage = "ui.composer.Send",
                            "send"
                        );
                        panel.send_prompt(cx);
                    }
                    ComposerEvent::Stop => panel.stop_turn(cx),
                    ComposerEvent::ModelMenu => {
                        panel.toggle_popover(ActivePopover::ModelMenu);
                        cx.notify();
                    }
                    ComposerEvent::AddMenu => {
                        panel.toggle_popover(ActivePopover::AddMenu);
                        cx.notify();
                    }
                    ComposerEvent::OpenAddMenu => {
                        panel.open_popover(ActivePopover::AddMenu);
                        cx.notify();
                    }
                    ComposerEvent::ClearWorkspace => {
                        panel.close_all_popovers();
                        panel.bind_default_workspace(cx);
                    }
                    ComposerEvent::ClearSkill => {
                        panel.selected_skill = None;
                        cx.notify();
                    }
                    ComposerEvent::ClearQuote => {
                        panel.active_quote = None;
                        cx.notify();
                    }
                    ComposerEvent::CloudNotReady => {
                        panel.open_popover(ActivePopover::ModelMenu);
                        panel.emit_notice(
                            Some("notice.model_setup_required"),
                            cx,
                        );
                    }
                }
            });
        let status = agent.turn_status();
        let project_load = crate::projects::load(&store_root);
        let (projects, project_store_failed) = match project_load {
            Ok(projects) => (projects, false),
            Err(_) => (Vec::new(), true),
        };
        let active_workspace = agent.active_workspace();
        let active_project_id = active_workspace
            .as_ref()
            .map(|workspace| workspace.project_id.clone());
        let initial_notice = if project_store_failed {
            Some("project.storage_error")
        } else if active_workspace.as_ref().is_some_and(|workspace| {
            !crate::projects::root_available(&workspace.root)
        }) {
            Some("project.folder_unavailable")
        } else {
            None
        };
        // Read the journal up front: an accepted message outlives the process,
        // so it must be visible again before anything is submitted this run.
        let pending_receipts = agent.pending_acceptances();
        let visible_receipts = agent.visible_acceptances();
        let automation =
            crate::automation::AutomationDraft::new(&i18n, window, cx);
        let mut panel = Self {
            ui,
            theme,
            feed,
            composer,
            pending_receipts,
            visible_receipts,
            retrying_receipts: std::collections::HashSet::new(),
            overriding_reports: std::collections::HashSet::new(),
            pending_card_selected: std::collections::HashMap::new(),
            resolving_cards: std::collections::HashSet::new(),
            _composer_sub: composer_sub,
            _feed_sub: feed_sub,
            _reveal_sub: reveal_sub,
            _deliverables_sub: deliverables_sub,
            _artifact_sub: artifact_sub,
            _working_sub: working_sub,
            submission: None,
            submission_pending: false,
            submission_generation: 0,
            submission_stop: None,
            retrying_delivery: None,
            working,
            status,
            last_elapsed: None,
            initialized: false,
            last_drain_check: None,
            _theme_sub: theme_sub,
            _overlay_sub: overlay_sub,
            prompt,
            projects,
            active_project_id,
            project_store_failed,
            document: DocumentSession::new(product),
            agent,
            credits: crate::CreditsViewState::Unavailable,
            popover: if model_info_open {
                ActivePopover::ModelInfo
            } else {
                ActivePopover::None
            },
            credits_close: None,
            board_snapshot: crate::ports::WorkBoardSnapshot::default(),
            popover_works: crate::router_works::PopoverWorksSnapshot::default(),
            local_env: crate::ports::LocalEnvSnapshot::default(),
            board_filter: crate::overview::BoardFilter::All,
            board_project_filter: None,
            board_day_filter: None,
            board_format_filter: None,
            board_older_expanded: false,
            board_width_px: 1040.0,
            board_inspector: BoardInspectorState::default(),
            trace_filter: crate::overview::TraceFilter::All,
            cached_board_lines: None,
            cached_trace: None,
            drawer_tab: crate::deliverables::RightDrawerTab::Deliverables,
            right_drawer_open: false,
            selected_skill: None,
            installed_skills: Vec::new(),
            board_generation: 0,
            last_board_refresh: None,
            last_popover_works_refresh: None,
            last_trace_refresh: None,
            tiers: Vec::new(),
            i18n,
            model,
            catalog: ProviderCatalog::new(),
            cloud_ready: false,
            catalog_loading: false,
            plan,
            model_info_version,
            store_root,
            product,
            surface,
            automation,
            automation_page: None,
            automation_editor: None,
            automation_confirm: None,
            notice: initial_notice,
            session_opened_by_user: false,
            deliverables: None,
            inspected_work: None,
            drawer_preview: DrawerPreviewState::default(),
            deliverables_view_scope:
                crate::deliverables::DeliverablesViewScope::Latest,
            drawer_wide: false,
            deliverable_sizes: std::collections::HashMap::new(),
            promote_project_candidate: None,
            drawer_meta_generation: 0,
            drawer_meta_task: None,
            step_turn_toggled: std::collections::HashMap::new(),
            expanded_step_items: std::collections::HashSet::new(),
            drawer_step_copied: None,
            drawer_step_copy_epoch: 0,
            drawer_step_copy_task: None,
            dock_visible: true,
            drafts: std::collections::HashMap::new(),
            active_quote: None,
            quote_session: None,
            draft_quotes: std::collections::HashMap::new(),
            _quote_sub: quote_sub,
            prompt_pending: None,
            _prompt_sub: prompt_sub,
            pump: None,
        };
        panel.start_pump(cx);
        panel.refresh_chat(cx);
        panel.refresh_board_snapshot();
        panel.refresh_local_env();
        // A persisted failed status must be visible on the first frame too.
        panel.refresh_working(cx);
        panel
    }

    fn start_pump(&mut self, cx: &Context<'_, Self>) {
        self.pump = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let ok = weak.update(cx, |panel, cx| {
                    let updates = panel.agent.drain_updates();
                    if !updates.is_empty() {
                        panel.process_agent_updates(updates, cx);
                    }
                    let elapsed = panel.agent.working_elapsed_secs();
                    if panel.last_elapsed != elapsed {
                        panel.last_elapsed = elapsed;
                        panel.refresh_working(cx);
                    }
                    if !panel.agent.autonomous_drain() {
                        // Legacy caller-driven drain kept only for deterministic
                        // `StudioPanel::new` unit tests; production relies on
                        // kernel autonomous drain + `AgentUpdate::Receipt`.
                        let now = std::time::Instant::now();
                        let due = panel.last_drain_check.is_none_or(|last| {
                            now.duration_since(last) >= Duration::from_secs(5)
                        });
                        if due
                            && panel.initialized
                            && !panel.submission_pending
                            && !panel.status.is_busy()
                        {
                            panel.last_drain_check = Some(now);
                            panel.refresh_pending_receipts(cx);
                        }
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        }));
    }

    pub(crate) fn process_agent_updates(
        &mut self,
        updates: Vec<flowy_agent::AgentUpdate>,
        cx: &mut Context<'_, Self>,
    ) {
        if updates.is_empty() {
            return;
        }
        let initialized = updates.iter().any(|update| {
            matches!(update, flowy_agent::AgentUpdate::Initialized)
        });
        if initialized {
            self.initialized = true;
            self.active_project_id = self
                .agent
                .active_workspace()
                .map(|workspace| workspace.project_id);
            // Startup always lands on Home: an existing history must not pull
            // the app back into the last thread. A session the user opened in
            // this run may restore into Chat, and an emptied conversation
            // steps back down to Home.
            if self.session_opened_by_user
                && matches!(
                    self.surface,
                    StudioSurface::Home | StudioSurface::Chat
                )
            {
                if self.agent.has_messages() {
                    self.surface = StudioSurface::Chat;
                } else {
                    self.surface = StudioSurface::Home;
                }
            } else if matches!(self.surface, StudioSurface::Chat)
                && !self.agent.has_messages()
            {
                self.surface = StudioSurface::Home;
            }
            if self.agent.active_session_id().is_none() {
                self.agent.set_workspace_for_new_sessions(None);
                self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
            }
            if self.notice == Some("library.loading") {
                self.emit_notice(None, cx);
            }
        }
        // Background activity updates session contents, not the selected surface.
        if initialized
            || updates.iter().any(|update| {
                matches!(update, flowy_agent::AgentUpdate::Sessions)
            })
        {
            cx.emit(StudioEvent::SessionsChanged);
        }
        if updates
            .iter()
            .any(|update| matches!(update, flowy_agent::AgentUpdate::Artifacts))
        {
            cx.emit(StudioEvent::ArtifactsChanged);
        }
        let mut receipt_changed = false;
        let mut receipt_completed = false;
        let mut receipt_notice: Option<(&'static str, u8)> = None;
        let mut set_receipt_notice = |key: &'static str, priority: u8| {
            if receipt_notice.is_none_or(|(_, cur)| priority > cur) {
                receipt_notice = Some((key, priority));
            }
        };
        for update in &updates {
            if let flowy_agent::AgentUpdate::Receipt { state, .. } = update {
                receipt_changed = true;
                match state {
                    ReceiptState::Dead => {
                        set_receipt_notice("notice.receipt_gave_up", 3);
                    }
                    ReceiptState::Quarantined => {
                        set_receipt_notice("notice.receipt_quarantined", 2);
                    }
                    ReceiptState::Retrying if self.agent.autonomous_drain() => {
                        set_receipt_notice("notice.receipt_retrying", 1);
                    }
                    ReceiptState::Completed => {
                        receipt_completed = true;
                    }
                    _ => {}
                }
            }
        }
        if receipt_changed || (initialized && self.agent.autonomous_drain()) {
            self.sync_receipts();
        }
        if let Some((key, _)) = receipt_notice {
            self.emit_notice(Some(key), cx);
        }
        if receipt_completed {
            cx.emit(StudioEvent::TurnFinished);
        }
        if updates
            .iter()
            .any(|update| matches!(update, flowy_agent::AgentUpdate::Skills))
        {
            cx.emit(StudioEvent::SkillsChanged);
        }
        if updates.iter().any(|update| {
            matches!(update, flowy_agent::AgentUpdate::StorageError)
        }) {
            self.emit_notice(Some("notice.session_storage_failed"), cx);
        }
        let structural_update = initialized
            || receipt_changed
            || updates.iter().any(|update| {
                matches!(
                    update,
                    flowy_agent::AgentUpdate::Sessions
                        | flowy_agent::AgentUpdate::Artifacts
                        | flowy_agent::AgentUpdate::StorageError
                )
            });
        let prev_status = self.status.clone();
        self.refresh_chat(cx);
        let status_changed = self.status != prev_status;
        if structural_update || status_changed {
            self.refresh_board_snapshot();
            self.last_trace_refresh = Some(std::time::Instant::now());
        } else {
            let active_sid = self.agent.active_session_id();
            let background_session_updated = updates.iter().any(|update| {
                matches!(
                    update,
                    flowy_agent::AgentUpdate::Session { id }
                        if active_sid.as_deref() != Some(id.as_str())
                )
            });
            if (background_session_updated || self.is_bg_tasks_open())
                && self.last_popover_works_refresh.is_none_or(|last| {
                    last.elapsed() >= Duration::from_millis(400)
                })
            {
                self.refresh_popover_works();
                cx.notify();
            }
            if self.surface == StudioSurface::Board
                && self.last_board_refresh.is_none_or(|last| {
                    last.elapsed() >= Duration::from_millis(1200)
                })
            {
                // While streaming deltas, only refresh the board when it is open
                // and at most once per 1.2s to avoid per-token work.
                self.refresh_board_snapshot();
                self.last_trace_refresh = Some(std::time::Instant::now());
            } else if let Some(trace_sid) = self.active_trace_session_id()
                && updates.iter().any(|update| {
                    matches!(
                        update,
                        flowy_agent::AgentUpdate::Session { id }
                            if id == &trace_sid
                    )
                })
                && self.last_trace_refresh.is_none_or(|last| {
                    last.elapsed() >= Duration::from_millis(300)
                })
            {
                self.refresh_trace_cache();
                self.last_trace_refresh = Some(std::time::Instant::now());
                cx.notify();
            }
        }
        if !self.agent.autonomous_drain() {
            let background_claim_settled = self.agent.has_active_claim()
                && self
                    .agent
                    .claimed_session_status()
                    .is_none_or(|status| !status.is_busy());
            if initialized || background_claim_settled {
                // Startup is a level, not an edge: a receipt accepted by the
                // previous process must start without a second submit, and a
                // claim running in a background session must settle when it ends.
                self.refresh_pending_receipts(cx);
            }
        } else if initialized {
            cx.notify();
        }
    }

    pub(crate) fn refresh_chat(&mut self, cx: &mut Context<'_, Self>) {
        self.sync_quote_session();
        let session = self.agent.active_session_id();
        let history = self.agent.chat_history();
        let status = history.status.clone();
        let suppress_trailing_stopped = session
            .as_deref()
            .and_then(|sid| self.agent.session_stop_handoff(sid))
            .is_some_and(|manifest| {
                crate::pending_card::stop_handoff_has_details(&manifest)
            });
        self.feed.update(cx, |feed, cx| {
            feed.set_suppress_trailing_stopped(suppress_trailing_stopped);
            feed.sync_history(session, history, cx);
        });
        self.sync_right_drawer_deliverables(cx);
        if status != self.status {
            let was_busy = self.status.is_busy();
            self.status = status;
            self.refresh_working(cx);
            cx.notify();
            if was_busy && !self.status.is_busy() {
                if self.agent.autonomous_drain() {
                    self.sync_receipts();
                } else {
                    // Legacy caller-driven drain path for deterministic unit tests.
                    self.refresh_pending_receipts(cx);
                }
                cx.emit(StudioEvent::TurnFinished);
            }
        }
    }

    pub fn skill_catalog(&self) -> Vec<flowy_agent::InstalledSkill> {
        self.agent.skill_catalog()
    }

    pub fn set_skill_catalog(
        &mut self,
        skills: Vec<flowy_agent::InstalledSkill>,
        cx: &mut Context<'_, Self>,
    ) {
        self.agent.replace_skill_catalog(skills);
        cx.emit(StudioEvent::SkillsChanged);
        cx.notify();
    }

    pub(crate) fn refresh_working(&self, cx: &mut Context<'_, Self>) {
        let active_session = self.agent.active_session_id();
        let retrying_current = self
            .retrying_delivery
            .as_deref()
            .is_some_and(|session| active_session.as_deref() == Some(session));
        let label = if self.submission_pending {
            self.t("chat.status_running")
        } else if retrying_current {
            self.t("chat.status_saving_files")
        } else {
            self.working_label(&self.status)
        };
        let error = !self.submission_pending
            && !retrying_current
            && matches!(
                self.status,
                flowy_agent::TurnStatus::Error { .. }
                    | flowy_agent::TurnStatus::DeliveryFailed
            );
        let visible = self.submission_pending
            || retrying_current
            || working_line_visible(&self.status);
        let retry = !self.submission_pending
            && self.retrying_delivery.is_none()
            && matches!(self.status, flowy_agent::TurnStatus::DeliveryFailed);
        let retry_label = self.t("chat.retry_delivery");
        self.working.update(cx, |working, cx| {
            working.set_label(label, visible, error, cx);
            working.set_retry(retry, retry_label, cx);
        });
    }

    pub fn sync_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n.clone();
        self.composer
            .update(cx, |composer, cx| composer.sync_i18n(i18n.clone(), cx));
        self.feed.update(cx, |feed, cx| feed.sync_i18n(i18n, cx));
        self.refresh_working(cx);
        self.refresh_board_snapshot();
        cx.notify();
    }

    pub fn sync_model(
        &mut self,
        model: Model,
        plan: Plan,
        model_info_version: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.model = model;
        self.plan = plan;
        self.model_info_version = model_info_version;
        cx.notify();
    }

    pub fn set_notice(
        &mut self,
        notice: Option<&'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        let notice = notice.or_else(|| {
            self.project_store_failed.then_some("project.storage_error")
        });
        if self.notice != notice {
            self.notice = notice;
            cx.notify();
        }
    }

    pub fn credits_label(&self) -> String {
        self.credits.label(&self.i18n)
    }

    /// Label for the selected tier. Model ids stay inside the routing layer;
    /// the menu and chip only ever show product tier names.
    pub(crate) fn model_label(&self) -> String {
        self.t(self.model.name_key())
    }

    /// Replaces the tier rows after a catalog or entitlement refresh.
    pub fn set_tiers(
        &mut self,
        tiers: Vec<TierView>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.tiers != tiers {
            self.tiers = tiers;
            cx.notify();
        }
    }

    pub(crate) fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    pub(crate) fn tf(&self, key: &str, vars: &[(&str, &str)]) -> String {
        self.i18n.tf(key, vars)
    }

    /// Open the validated local folder holding `path` (rejecting executables,
    /// URIs, UNC shares, device namespaces, and symlinks / NTFS reparse points).
    pub(crate) fn reveal_file(
        &mut self,
        path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(folder) = crate::ports::resolve_safe_parent_dir(path) else {
            self.emit_notice(Some("project.view_failed"), cx);
            return;
        };
        if open::that(&folder).is_err() {
            self.emit_notice(Some("project.view_failed"), cx);
        }
    }

    pub(crate) fn emit_notice(
        &mut self,
        notice: Option<&'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        let notice = notice.or_else(|| {
            self.project_store_failed.then_some("project.storage_error")
        });
        self.notice = notice;
        cx.emit(StudioEvent::Notice(notice));
        cx.notify();
    }

    pub(crate) fn emit_model(&self, cx: &mut Context<'_, Self>) {
        cx.emit(StudioEvent::ModelChanged {
            model: self.model,
            plan: self.plan,
            model_info_version: self.model_info_version.clone(),
        });
    }

    pub fn note(&mut self, key: &'static str, cx: &mut Context<'_, Self>) {
        self.emit_notice(Some(key), cx);
    }

    pub(crate) fn placeholder_title(&self) -> String {
        match self.surface {
            StudioSurface::Placeholder { title_key, .. } => self.t(title_key),
            _ => self.t("studio.brand"),
        }
    }

    pub(crate) fn placeholder_detail(&self) -> String {
        match self.surface {
            StudioSurface::Placeholder { detail_key, .. } => self.t(detail_key),
            _ => String::new(),
        }
    }
}

impl Render for StudioPanel {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        self.apply_prompt_pending(window, cx);
        if self.surface == StudioSurface::Automation {
            let placeholder = self.t("auto.prompt_placeholder");
            self.automation.sync_editor(
                placeholder.into(),
                self.theme,
                window,
                cx,
            );
        }
        let model_label = self.model_label();
        let can_submit = self.can_submit_prompt();
        let workspace_label = self
            .active_project_id
            .as_deref()
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map_or_else(
                || self.t("composer.workspace_default"),
                |project| project.name.clone(),
            );
        let workspace_bound = self.active_project_id.is_some();
        let skill_label = self
            .selected_skill
            .as_ref()
            .map(|skill| gpui::SharedString::from(skill.title.clone()));
        let composer_state = crate::composer::ComposerViewState {
            model: self.model,
            model_menu_open: self.is_model_menu_open(),
            running: self.submission_pending || self.status.is_busy(),
            stopping: matches!(self.status, flowy_agent::TurnStatus::Stopping),
            model_label: gpui::SharedString::from(model_label),
            cloud_ready: can_submit,
            workspace_label: gpui::SharedString::from(workspace_label),
            workspace_bound,
            skill_label,
            active_quote: self.active_quote.clone(),
            add_menu_open: self.is_add_menu_open(),
        };
        self.composer.update(cx, |composer, cx| {
            composer.sync_state(composer_state, cx);
        });
        self.workspace(window, cx)
    }
}

#[cfg(test)]
mod tests;

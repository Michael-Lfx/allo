use crate::composer::{ChatComposer, ComposerEvent};
use crate::drawer_preview::{
    DrawerPreviewBody, DrawerPreviewState, PreviewMode,
};
use crate::feed::{
    ArtifactAction, ChatFeed, OpenDeliverables, OpenTask, QuoteMessage,
    RevealFile, WorkingIndicator, WorkingIndicatorEvent,
};
use flowy_agent::{
    AgentHandle, AgentWorkspace, CardAction, FileEntry, LlmConfig, Model, Plan,
    ProviderCatalog, ReceiptState, SessionOrigin, TaskTarget,
};
use flowy_core::DocumentSession;
use flowy_i18n::I18n;
use flowy_ui::{ResolvedTheme, UiContext};
use gpui::{
    Context, Entity, EventEmitter, Focusable, IntoElement, Task, Window,
    prelude::*,
};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

fn new_submission_id() -> u128 {
    static NEXT: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (timestamp << 32) | u128::from(std::process::id() ^ (sequence as u32))
}

/// Formats an outgoing prompt using `flowy-agent`'s real `/skill:<owner>/<slug>`
/// invocation protocol when an installed skill chip is mounted.
fn format_skill_prompt(
    selected: Option<&crate::ports::SkillChip>,
    trimmed: &str,
) -> String {
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(skill) = selected
        && !trimmed.starts_with("/skill:")
    {
        return format!("/skill:{} {trimmed}", skill.id);
    }
    trimmed.to_owned()
}

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
            if self.session_opened_by_user {
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

    fn sync_quote_session(&mut self) {
        let current = self.agent.active_session_id();
        if current != self.quote_session {
            let previous = self.quote_session.take();
            self.stash_active_quote(previous);
            self.active_quote = current
                .as_deref()
                .and_then(|id| self.draft_quotes.remove(id));
            self.quote_session = current;
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

    pub(crate) fn invalidate_submission(&mut self, cx: &mut Context<'_, Self>) {
        self.submission_generation = self.submission_generation.wrapping_add(1);
        self.submission_pending = false;
        self.submission_stop = None;
        self.submission = None;
        self.refresh_working(cx);
    }

    /// Whether the composer should allow submitting immediately (or queueing
    /// during cold-start catalog loading) rather than showing the model-setup
    /// amber warning state.
    #[must_use]
    pub(crate) fn can_submit_prompt(&self) -> bool {
        self.cloud_ready
            || self.catalog_loading
            || !self.agent.requires_cloud_ready()
    }

    pub fn set_surface(
        &mut self,
        surface: StudioSurface,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(surface, StudioSurface::Chat) {
            self.session_opened_by_user = true;
        }
        if self.surface != surface {
            self.invalidate_submission(cx);
            self.surface = surface;
            self.refresh_popover_works();
            cx.notify();
        }
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

    pub(crate) fn clear_right_sheet(&mut self) {
        self.right_drawer_open = false;
        self.deliverables = None;
        self.inspected_work = None;
        self.drawer_preview.clear();
        self.drawer_meta_generation =
            self.drawer_meta_generation.wrapping_add(1);
        self.drawer_meta_task = None;
        self.deliverable_sizes.clear();
        self.promote_project_candidate = None;
        self.refresh_trace_cache();
    }

    /// Binds (or clears) the pending draft project workspace and origin for
    /// lazily created Router sessions (Q1).
    pub(crate) fn apply_draft_project(
        &mut self,
        project: Option<&crate::projects::Project>,
    ) {
        if let Some(project) = project {
            let project_id = project.id.clone();
            self.active_project_id = Some(project_id.clone());
            self.agent
                .set_workspace_for_new_sessions(Some(AgentWorkspace {
                    project_id: project.id.clone(),
                    root: project.root.clone(),
                }));
            self.agent
                .set_origin_for_new_sessions(SessionOrigin::Project {
                    project_id: Some(project_id),
                });
        } else {
            self.active_project_id = None;
            self.agent.set_workspace_for_new_sessions(None);
            self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
        }
    }

    pub(crate) fn set_active_quote(
        &mut self,
        role: crate::composer::QuoteRole,
        raw_text: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let excerpt = crate::composer::make_quote_excerpt(raw_text);
        if excerpt.is_empty() {
            return;
        }
        self.active_quote =
            Some(crate::composer::ActiveQuote { role, excerpt });
        self.quote_session = self.agent.active_session_id();
        cx.notify();
    }

    fn stash_active_quote(&mut self, session_id: Option<String>) {
        if let Some(id) = session_id {
            if let Some(quote) = self.active_quote.take() {
                self.draft_quotes.insert(id, quote);
            } else {
                self.draft_quotes.remove(&id);
            }
        } else {
            self.active_quote = None;
        }
    }

    pub fn reset_session(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(current) = self.agent.active_session_id() {
            self.draft_quotes.remove(&current);
        }
        self.active_quote = None;
        self.quote_session = None;
        self.invalidate_submission(cx);
        self.close_all_popovers();
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.document = DocumentSession::new(self.product);
        let _ = self.agent.clear_active();
        self.apply_draft_project(None);
        self.surface = StudioSurface::Home;
        self.refresh_chat(cx);
        self.refresh_board_snapshot();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    pub fn new_task(&mut self, cx: &mut Context<'_, Self>) {
        let inherited_project = self
            .active_project_id
            .as_deref()
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned();
        match self.agent.clear_active() {
            Ok(()) => {
                self.invalidate_submission(cx);
                self.close_all_popovers();
                self.board_inspector.close_drawer();
                self.clear_right_sheet();
                self.apply_draft_project(inherited_project.as_ref());
                self.surface = StudioSurface::Home;
                self.refresh_chat(cx);
                self.refresh_board_snapshot();
                self.emit_notice(None, cx);
                self.set_prompt_text("", cx);
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    /// Present a global-home state after the host has cleared the active
    /// session and prepared its origin on a background worker.
    pub fn show_new_home(&mut self, cx: &mut Context<'_, Self>) {
        self.invalidate_submission(cx);
        self.close_all_popovers();
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.active_project_id = None;
        self.document = DocumentSession::new(self.product);
        self.submission = None;
        self.surface = StudioSurface::Home;
        self.set_prompt_text("", cx);
        self.refresh_chat(cx);
        self.refresh_board_snapshot();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    /// Toggles the floating background-tasks popover anchored above the composer.
    pub fn toggle_bg_tasks(&mut self, cx: &mut Context<'_, Self>) {
        self.toggle_popover(ActivePopover::BgTasks);
        if self.is_bg_tasks_open() {
            self.refresh_popover_works();
        }
        cx.notify();
    }

    /// Running sessions shown in the background-works popover: on Chat,
    /// excludes the active foreground session (already surfaced by the inline
    /// process capsule and `WorkingIndicator`); on Home, includes all running
    /// sessions so folded work stays reachable.
    #[must_use]
    pub(crate) fn background_running_session_ids(&self) -> Vec<String> {
        let active = self.agent.active_session_id();
        let exclude_active = self.surface == StudioSurface::Chat;
        let mut ids: Vec<String> = self
            .agent
            .busy_session_ids()
            .into_iter()
            .filter(|id| {
                !(exclude_active && active.as_deref() == Some(id.as_str()))
            })
            .collect();
        if !exclude_active
            && ids.is_empty()
            && (self.status.is_busy() || self.submission_pending)
            && let Some(active_id) = active
        {
            ids.push(active_id);
        }
        ids
    }

    /// Visible receipts shown in the background-tasks popover, skipping any
    /// `Running` receipt whose target session is already represented in
    /// `running_ids` so running work is never listed twice.
    pub(crate) fn popover_visible_receipts<'a>(
        &'a self,
        running_ids: &[String],
    ) -> Vec<(&'a flowy_agent::AcceptedMessage, ReceiptState)> {
        self.visible_receipts
            .iter()
            .filter(|(receipt, state)| {
                if *state == ReceiptState::Running {
                    let already_listed =
                        receipt.target_session_id.as_deref().is_some_and(
                            |sid| running_ids.iter().any(|id| id == sid),
                        );
                    return !already_listed;
                }
                !matches!(
                    state,
                    ReceiptState::Completed | ReceiptState::Cancelled
                )
            })
            .map(|(receipt, state)| (receipt, *state))
            .collect()
    }

    /// Total active/attention/queued works shown in the background-tasks pill
    /// and Home resume pill (backed by the cached `popover_works` snapshot).
    #[must_use]
    pub fn background_work_count(&self) -> usize {
        self.popover_total_count()
    }

    /// Total active/queued works across all sessions (including foreground),
    /// for the sidebar Chief Assistant status badge.
    #[must_use]
    pub fn total_active_work_count(&self) -> usize {
        let busy_ids = self.agent.busy_session_ids();
        let mut running = busy_ids.len();
        if (self.status.is_busy() || self.submission_pending) && running == 0 {
            running = 1;
        }
        running + self.popover_visible_receipts(&busy_ids).len()
    }

    /// Jumps to the Chief Assistant's active default-workspace thread, or lands
    /// calmly on the Home stage ready for a fresh instruction.
    pub fn open_chief_assistant(&mut self, cx: &mut Context<'_, Self>) {
        self.close_all_popovers();
        let sessions = self.agent.list_sessions();
        let active_is_live_default = self
            .agent
            .active_session_id()
            .as_deref()
            .is_some_and(|active_id| {
                sessions.iter().any(|s| {
                    s.id == active_id && !s.archived && s.project_id.is_none()
                })
            });
        if active_is_live_default
            && self.active_project_id.is_none()
            && self.agent.has_messages()
        {
            self.session_opened_by_user = true;
            self.surface = StudioSurface::Chat;
            self.refresh_chat(cx);
            self.refresh_popover_works();
            cx.notify();
            return;
        }
        let latest_default = sessions
            .into_iter()
            .find(|session| session.project_id.is_none() && !session.archived);
        if let Some(session) = latest_default {
            self.open_session(&session.id, cx);
        } else {
            self.reset_session(cx);
        }
    }

    /// Marks the cloud route ready and refreshes receipts/board state.
    /// In autonomous mode, `AgentHandle::apply_config` wakes draining itself;
    /// deterministic unit tests still use the legacy check below.
    pub fn notify_cloud_ready(&mut self, cx: &mut Context<'_, Self>) {
        self.cloud_ready = true;
        self.refresh_local_env();
        if self.agent.autonomous_drain() {
            self.sync_receipts();
            self.refresh_board_snapshot();
            cx.notify();
        } else if self.initialized {
            self.refresh_pending_receipts(cx);
        } else {
            cx.notify();
        }
    }

    /// Re-read the acceptance journal, finish a settled claim, and start the
    /// next receipt when the agent is idle.
    fn refresh_pending_receipts(&mut self, cx: &mut Context<'_, Self>) {
        // A manual submit in flight must not interleave with a queued receipt;
        // the pump re-checks when it settles.
        if self.submission_pending {
            self.sync_receipts();
            cx.notify();
            return;
        }
        if self.agent.has_active_claim() {
            let claimed_status = self.agent.claimed_session_status();
            if claimed_status
                .as_ref()
                .is_some_and(flowy_agent::TurnStatus::is_busy)
            {
                self.sync_receipts();
                cx.notify();
                return;
            }
            // Completed and DeliveryFailed already finished their LLM work
            // (DeliveryFailed has its own Retry Delivery surface). Stopped
            // (e.g. provider stream abort), Error, and Idle must NOT be stamped
            // Completed so queued prompts are never silently lost.
            let attempts = self.agent.accepted_attempts().unwrap_or(0);
            let completed = claimed_status.is_some_and(|status| {
                matches!(
                    status,
                    flowy_agent::TurnStatus::Completed
                        | flowy_agent::TurnStatus::DeliveryFailed
                )
            });
            match self.agent.finish_accepted(completed) {
                Ok(true) => {
                    self.emit_notice(Some("notice.receipt_gave_up"), cx);
                }
                Ok(false) if !completed && attempts == 3 => {
                    self.emit_notice(Some("notice.receipt_retrying"), cx);
                }
                Ok(false) => {}
                Err(error) => self.emit_notice(Some(error), cx),
            }
        }
        let can_drain = !self.status.is_busy()
            && !self.catalog_loading
            && (self.cloud_ready || !self.agent.requires_cloud_ready());
        if can_drain && !self.agent.pending_acceptances().is_empty() {
            let previous = self.agent.active_session_id();
            match self.agent.run_accepted_once() {
                Ok(Some(run)) => {
                    if self.surface == StudioSurface::Home && previous.is_none()
                    {
                        // Draining in the background from a blank Home stage
                        // must not bind the user's next Home prompt to the
                        // drained session.
                        let _ = self.agent.clear_active();
                    }
                    self.sync_receipts();
                    self.refresh_chat(cx);
                    if previous.as_deref() != Some(run.session_id.as_str()) {
                        self.emit_notice(Some("notice.accepted_ran"), cx);
                    }
                    return;
                }
                Ok(None) => {}
                Err(error) => self.emit_notice(Some(error), cx),
            }
        }
        self.sync_receipts();
        cx.notify();
    }

    pub fn open_session(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        self.session_opened_by_user = true;
        // Reopening the current session must not round-trip its own draft.
        let same = self.agent.active_session_id().as_deref() == Some(id);
        let previous_session = if same {
            None
        } else {
            self.agent.active_session_id()
        };
        if let Some(current) = &previous_session {
            let draft = self.prompt_text(cx);
            self.drafts.insert(current.clone(), draft);
        }
        match self.agent.select_session(id) {
            Ok(()) => {
                if !same {
                    self.sync_quote_session();
                    // Restore whatever this session had typed before.
                    let draft =
                        self.drafts.get(id).cloned().unwrap_or_default();
                    self.fill_prompt(&draft, cx);
                    self.clear_right_sheet();
                }
                self.invalidate_submission(cx);
                // The journal is global, but re-read it on every open so the
                // strip can never show a state older than the click that
                // brought the user here.
                self.sync_receipts();
                let workspace = self.agent.active_workspace();
                self.active_project_id =
                    workspace.as_ref().map(|value| value.project_id.clone());
                self.surface = if self.agent.has_messages() {
                    StudioSurface::Chat
                } else {
                    StudioSurface::Home
                };
                self.refresh_chat(cx);
                self.refresh_trace_cache();
                self.emit_notice(
                    workspace
                        .as_ref()
                        .filter(|value| {
                            !crate::projects::root_available(&value.root)
                        })
                        .map(|_| "project.folder_unavailable"),
                    cx,
                );
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    /// Replace the file metadata projection without duplicating managed paths
    /// or reading disk on the UI thread.
    pub fn set_artifacts(
        &mut self,
        artifacts: Vec<crate::library::LibraryItem>,
        cx: &mut Context<'_, Self>,
    ) {
        self.feed
            .update(cx, |feed, cx| feed.set_artifacts(artifacts, cx));
        if self.right_drawer_open {
            self.refresh_drawer_metadata(cx);
            cx.notify();
        }
    }

    /// Explicit override bound to a specific session and real non-empty
    /// verification `report_id` (required by kernel `f8fb6b491`). Runs on a
    /// background worker with an in-flight guard so large SHA-256 hashes and
    /// `sync_all` never stall the GPUI frame or accept duplicate clicks.
    pub(crate) fn confirm_override_for_session(
        &mut self,
        session_id: &str,
        report_id: &str,
        artifact_path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if report_id.trim().is_empty() {
            self.emit_notice(Some("verify.override.missing_report"), cx);
            return;
        }
        if !self.overriding_reports.insert(report_id.to_owned()) {
            return;
        }
        cx.notify();
        let agent = self.agent.clone();
        let sid = session_id.to_owned();
        let rid = report_id.to_owned();
        let apath = artifact_path.to_owned();
        let guard_key = rid.clone();
        let job = cx.background_executor().spawn(async move {
            let result = agent.apply_card_action(CardAction::OverrideDeliver {
                session_id: sid,
                report_id: rid,
                artifact_path: apath,
            });
            let updates = agent.drain_updates();
            (result, updates)
        });
        cx.spawn(async move |weak, cx| {
            let (result, updates) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                panel.overriding_reports.remove(guard_key.as_str());
                panel.process_agent_updates(updates, cx);
                match result {
                    Ok(_) => {
                        panel.board_inspector.override_ack_key = None;
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                        panel.emit_notice(None, cx);
                        cx.emit(StudioEvent::ArtifactsChanged);
                    }
                    Err(error) => panel.emit_notice(Some(error), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Resolve a durable artifact anchor only after the session projection is
    /// ready. The feed translates it to the visible virtual-list row.
    pub fn focus_artifact(
        &mut self,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        self.feed
            .update(cx, |feed, cx| feed.focus_artifact(artifact_id, cx))
    }

    pub fn artifact_focus(
        &self,
        artifact_id: &str,
        cx: &gpui::App,
    ) -> Option<gpui::FocusHandle> {
        self.feed.read(cx).artifact_focus(artifact_id)
    }

    /// Refresh the projection after the host has deleted a conversation on a
    /// background worker. This method never mutates durable history itself.
    pub fn refresh_sessions_after_delete(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        let sessions = self.agent.list_sessions();
        self.draft_quotes
            .retain(|sid, _| sessions.iter().any(|s| s.id == *sid));
        if self.agent.active_session_id().is_none() {
            self.active_quote = None;
            self.quote_session = None;
            self.invalidate_submission(cx);
            self.clear_right_sheet();
            self.surface = StudioSurface::Home;
            self.document = DocumentSession::new(self.product);
            self.apply_draft_project(None);
            self.set_prompt_text("", cx);
        } else if let Some(crate::deliverables::InspectedWork::Session {
            id,
            ..
        }) = &self.inspected_work
        {
            let still_exists = sessions.iter().any(|session| {
                session.id == *id
                    || (self.agent.session_role(&session.id)
                        == flowy_agent::SessionRole::Router
                        && self
                            .agent
                            .child_works(&session.id)
                            .iter()
                            .any(|work| work.work_id == *id))
            });
            if !still_exists {
                self.clear_right_sheet();
            }
        }
        self.refresh_chat(cx);
        self.refresh_board_snapshot();
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    pub fn open_artifact_source(
        &mut self,
        session_id: &str,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        let target_root = self.agent.root_session_id(session_id);
        if !self
            .agent
            .list_sessions()
            .iter()
            .any(|session| session.id == target_root)
        {
            return Err("library.source.unavailable");
        }
        let previous = self.agent.active_session_id();
        self.agent.select_session(&target_root)?;
        self.invalidate_submission(cx);
        self.refresh_chat(cx);
        if !self.focus_artifact(artifact_id, cx) {
            if let Some(previous) = previous {
                let _ = self.agent.select_session(&previous);
            } else {
                let _ = self.agent.clear_active();
            }
            self.refresh_chat(cx);
            return Err("library.source.unavailable");
        }
        self.active_project_id = self
            .agent
            .active_workspace()
            .map(|workspace| workspace.project_id);
        self.session_opened_by_user = true;
        self.surface = StudioSurface::Chat;
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.refresh_trace_cache();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    /// Render and focus the session that the host has already selected and
    /// verified on a worker. This method only reads the in-memory projection.
    pub fn show_selected_artifact(
        &mut self,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        self.invalidate_submission(cx);
        self.refresh_chat(cx);
        if !self.focus_artifact(artifact_id, cx) {
            return Err("library.source.unavailable");
        }
        self.active_project_id = self
            .agent
            .active_workspace()
            .map(|workspace| workspace.project_id);
        self.session_opened_by_user = true;
        self.surface = StudioSurface::Chat;
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.refresh_trace_cache();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    fn draft_workspace(&self) -> Option<AgentWorkspace> {
        if let Some(pid) = self.active_project_id.as_deref() {
            self.projects
                .iter()
                .find(|p| p.id == pid)
                .map(|p| AgentWorkspace {
                    project_id: p.id.clone(),
                    root: p.root.clone(),
                })
        } else {
            self.agent.active_workspace()
        }
    }

    pub fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
        if self.submission_pending {
            return;
        }
        if self.agent.is_initializing() {
            self.emit_notice(Some("library.loading"), cx);
            return;
        }
        let model = self.catalog.selected_model().map(|m| m.id.clone());
        // Text queued for the next render has not reached the prompt state yet;
        // it is still what the user asked to send.
        let typed = self.prompt_text(cx);
        let trimmed = typed.trim();
        let trimmed_draft = trimmed.to_owned();
        let after_skill =
            format_skill_prompt(self.selected_skill.as_ref(), trimmed);
        let submitted_quote = self.active_quote.clone();
        let text = crate::composer::compose_quoted_prompt(
            submitted_quote.as_ref(),
            &after_skill,
        );
        tracing::debug!(
            target: "flowy.prompt",
            stage = "StudioPanel::send_prompt",
            cloud_ready = self.cloud_ready,
            model = ?model,
            chars = text.chars().count(),
            "enter"
        );
        if after_skill.is_empty() {
            tracing::debug!(
                target: "flowy.prompt",
                stage = "StudioPanel::send_prompt",
                reason = "empty",
                "reject"
            );
            self.emit_notice(Some("notice.prompt_empty"), cx);
            return;
        }
        let active_session = self.agent.active_session_id();
        let active_role = active_session
            .as_deref()
            .map(|sid| self.agent.session_role(sid));
        let is_legacy_standard =
            active_role == Some(flowy_agent::SessionRole::Standard);
        let use_router_path = !is_legacy_standard;

        // Only pause sending when the active conversation ITSELF has a pending
        // card (Router's own disambiguation card or Standard's card). Child
        // Worker approval cards never block the main Router conversation (Q4).
        let has_own_pending_card =
            matches!(self.status, flowy_agent::TurnStatus::WaitingForUser)
                || active_session.as_deref().is_some_and(|sid| {
                    !self.agent.session_pending_cards(sid).is_empty()
                });
        if has_own_pending_card {
            self.emit_notice(Some("notice.card_pending"), cx);
            return;
        }
        let running = self.status.is_busy();
        let waiting_for_cloud = self.catalog_loading
            || ((!self.cloud_ready || model.is_none())
                && self.agent.requires_cloud_ready());
        // Legacy Standard sessions defer whenever running or awaiting cloud
        // startup; fresh drafts defer whenever awaiting cloud startup; existing
        // Router sessions defer only during cold-start catalog load (warm busy
        // Routers continue through `pending_router_followups`).
        let should_defer = if is_legacy_standard {
            running || waiting_for_cloud
        } else {
            waiting_for_cloud
                && (active_session.is_none() || self.catalog_loading)
        };
        if should_defer {
            let draft_workspace = self.draft_workspace();
            let accepted = if is_legacy_standard {
                self.agent.accept_deferred(&text)
            } else if let Some(router_id) = active_session.as_deref() {
                self.agent.accept_deferred_router(router_id, &text)
            } else {
                let draft_origin = match &draft_workspace {
                    Some(ws) => flowy_agent::SessionOrigin::Project {
                        project_id: Some(ws.project_id.clone()),
                    },
                    None => flowy_agent::SessionOrigin::Home,
                };
                self.agent.accept_deferred_draft(
                    &text,
                    draft_origin,
                    draft_workspace,
                    flowy_agent::SessionRole::Router,
                )
            };
            match accepted {
                Ok(receipt) => {
                    tracing::debug!(
                        target: "flowy.prompt",
                        stage = "StudioPanel::send_prompt",
                        receipt = %receipt.id,
                        legacy = is_legacy_standard,
                        "accepted-deferred"
                    );
                    self.set_prompt_text("", cx);
                    self.active_quote = None;
                    if let Some(sid) = active_session.as_deref() {
                        self.draft_quotes.remove(sid);
                    }
                    if self.agent.autonomous_drain() {
                        self.sync_receipts();
                        self.refresh_board_snapshot();
                        cx.notify();
                    } else {
                        self.refresh_pending_receipts(cx);
                    }
                }
                Err(error) => self.emit_notice(Some(error), cx),
            }
            return;
        }
        if !self.cloud_ready || model.is_none() {
            tracing::debug!(
                target: "flowy.prompt",
                stage = "StudioPanel::send_prompt",
                reason = "cloud_or_model",
                "reject"
            );
            self.emit_notice(Some("notice.agent_failed"), cx);
            return;
        }
        let id = *self.submission.get_or_insert_with(new_submission_id);
        self.submission_generation = self.submission_generation.wrapping_add(1);
        let generation = self.submission_generation;
        self.submission_pending = true;
        let stop = Arc::new(AtomicBool::new(false));
        self.submission_stop = Some(stop.clone());
        self.refresh_working(cx);
        self.refresh_popover_works();
        cx.notify();
        let agent = self.agent.clone();
        let expected_generation = agent.context_generation();
        let expected_active = active_session;
        let expected_workspace = self.draft_workspace();
        let submitted_text = text.clone();
        let expected = expected_active.clone();
        let worker_stop = stop.clone();
        let worker_agent = agent.clone();
        let job = cx.background_executor().spawn(async move {
            let result = if worker_stop.load(Ordering::Acquire) {
                Err("notice.agent_busy")
            } else if expected_workspace.as_ref().is_some_and(|workspace| {
                !crate::projects::root_available(&workspace.root)
            }) {
                Err("project.folder_unavailable")
            } else if use_router_path {
                worker_agent.submit_router_chat_if_active(
                    id,
                    &submitted_text,
                    expected.as_deref(),
                    expected_generation,
                    expected_workspace,
                )
            } else {
                worker_agent.submit_chat_if_active(
                    id,
                    &submitted_text,
                    expected.as_deref(),
                    expected_generation,
                )
            };
            let stopped = if let Ok(session_id) = &result
                && worker_stop.load(Ordering::Acquire)
            {
                if use_router_path {
                    let _ = worker_agent.stop_router_reply(session_id);
                } else {
                    let _ = worker_agent
                        .cancel_task(TaskTarget::Session(session_id.clone()));
                }
                true
            } else {
                false
            };
            (result, stopped)
        });
        cx.spawn(async move |weak, cx| {
            let (result, stopped) = job.await;
            // A Stop click can arrive after the worker returns its receipt but
            // before this UI continuation runs. Keep it bound to that receipt.
            if !stopped
                && stop.load(Ordering::Acquire)
                && let Ok(session_id) = &result
            {
                let session_id = session_id.clone();
                cx.background_executor()
                    .spawn(async move {
                        if use_router_path {
                            let _ = agent.stop_router_reply(&session_id);
                        } else {
                            let _ = agent
                                .cancel_task(TaskTarget::Session(session_id));
                        }
                    })
                    .detach();
            }
            let _ = weak.update(cx, |panel, cx| {
                if generation != panel.submission_generation
                    || !panel.submission_pending
                    || panel.submission != Some(id)
                {
                    return;
                }
                panel.submission_pending = false;
                panel.submission_stop = None;
                let active = panel.agent.active_session_id();
                let draft_matches =
                    panel.prompt_text(cx).trim() == trimmed_draft;
                match result {
                    Ok(session_id)
                        if active.as_deref() == Some(session_id.as_str()) =>
                    {
                        panel.submission = None;
                        if draft_matches {
                            panel.set_prompt_text("", cx);
                        }
                        if panel.active_quote == submitted_quote {
                            panel.active_quote = None;
                        }
                        if submitted_quote.is_some()
                            && panel.draft_quotes.get(&session_id)
                                == submitted_quote.as_ref()
                        {
                            panel.draft_quotes.remove(&session_id);
                        }
                        panel.session_opened_by_user = true;
                        panel.surface = StudioSurface::Chat;
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                        panel.emit_notice(None, cx);
                        cx.emit(StudioEvent::SessionsChanged);
                    }
                    Ok(session_id) => {
                        // User switched context mid-send: keep the new context's
                        // draft untouched while refreshing sessions/board.
                        panel.submission = None;
                        if submitted_quote.is_some()
                            && panel.draft_quotes.get(&session_id)
                                == submitted_quote.as_ref()
                        {
                            panel.draft_quotes.remove(&session_id);
                        }
                        panel.refresh_board_snapshot();
                        cx.emit(StudioEvent::SessionsChanged);
                    }
                    Err(key)
                        if active == expected_active
                            || expected_active.is_none() =>
                    {
                        if !draft_matches {
                            panel.submission = None;
                        }
                        panel.emit_notice(Some(key), cx);
                    }
                    _ => panel.submission = None,
                }
                panel.refresh_working(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn retry_delivery(&mut self, cx: &mut Context<'_, Self>) {
        if !matches!(self.status, flowy_agent::TurnStatus::DeliveryFailed) {
            return;
        }
        let Some(expected) = self.agent.active_session_id() else {
            return;
        };
        self.retry_delivery_for_session(&expected, cx);
    }

    pub fn stop_turn(&mut self, cx: &mut Context<'_, Self>) {
        if self.submission_pending {
            if let Some(stop) = &self.submission_stop {
                stop.store(true, Ordering::Release);
            }
            cx.notify();
        } else if let Some(session_id) = self.agent.active_session_id() {
            self.stop_session_turn(&session_id, cx);
        }
    }

    pub fn fill_prompt(&mut self, text: &str, cx: &mut Context<'_, Self>) {
        self.set_prompt_text(text, cx);
    }

    /// The prompt text the user sees, including text queued for the next render.
    fn prompt_text(&self, cx: &Context<'_, Self>) -> String {
        match self.prompt_pending.as_deref() {
            Some(pending) => pending.to_owned(),
            None => self.prompt.read(cx).value().to_string(),
        }
    }

    /// Queue prompt text for the render pass.
    ///
    /// The editing state installs programmatic text through `set_value`, which
    /// needs a `&mut Window`; most prompt entry points (session restore,
    /// background submission callbacks) run without one, so the text is applied
    /// at the top of `render`.
    fn set_prompt_text(
        &mut self,
        text: impl Into<String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.prompt_pending = Some(text.into());
        cx.notify();
    }

    /// Install queued prompt text once a window is available.
    fn apply_prompt_pending(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(text) = self.prompt_pending.take() else {
            return;
        };
        self.prompt.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            // `set_value` resets a multi-line selection to the start; a
            // programmatic fill leaves the caret where typing continues.
            let end = state.value().len();
            state.set_selected_range(end..end, cx);
        });
    }

    pub fn fill_prompt_key(
        &mut self,
        key: &'static str,
        cx: &mut Context<'_, Self>,
    ) {
        let text = self.t(key);
        self.fill_prompt(&text, cx);
    }

    /// Fills the prompt from a starter. Picking one means "show me this
    /// prompt", so an existing draft is replaced without asking.
    pub(crate) fn request_fill_prompt(
        &mut self,
        text: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.fill_prompt(&text, cx);
    }

    /// Whether the user opened a conversation in this run. The app shell uses
    /// this to keep launch on Home while still restoring an opened session.
    pub const fn session_opened_by_user(&self) -> bool {
        self.session_opened_by_user
    }
    pub(crate) fn open_deliverables(
        &mut self,
        files: Vec<FileEntry>,
        cx: &mut Context<'_, Self>,
    ) {
        let had_input = !files.is_empty();
        let filtered = self.feed.read(cx).filter_settled_deliverables(files);
        if had_input && filtered.is_empty() {
            if self
                .drawer_preview
                .file
                .as_deref()
                .is_some_and(|path| self.feed.read(cx).is_path_failed(path))
            {
                self.drawer_preview.clear();
            }
            self.emit_notice(Some("verify.card.failed"), cx);
            return;
        }
        let quick_single = (filtered.len() == 1
            && self.feed.read(cx).failed_verifications().is_empty()
            && self.feed.read(cx).session_artifacts().is_empty())
        .then(|| filtered[0].path.clone());
        self.clear_right_sheet();
        self.right_drawer_open = true;
        self.deliverables = Some(
            crate::deliverables::DeliverableScope::TurnSnapshot(filtered),
        );
        self.deliverables_view_scope =
            crate::deliverables::DeliverablesViewScope::Latest;
        self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        self.close_all_popovers();
        self.refresh_drawer_metadata(cx);
        if let Some(single_path) = quick_single {
            self.set_preview_file(Some(single_path), cx);
        } else {
            cx.notify();
        }
    }

    /// Opens the right-hand inspector sheet for one running session or queued
    /// receipt without interrupting the user's current conversation.
    pub(crate) fn open_work_inspector(
        &mut self,
        work: crate::deliverables::InspectedWork,
        cx: &mut Context<'_, Self>,
    ) {
        self.clear_right_sheet();
        self.right_drawer_open = true;
        self.inspected_work = Some(work);
        self.drawer_tab = crate::deliverables::RightDrawerTab::Steps;
        self.close_all_popovers();
        cx.notify();
    }

    /// Toggles the unified right-hand 3-mode drawer (Deliverables / Steps / Trace),
    /// even before the turn has produced files so Steps and Trace stay reachable.
    pub(crate) fn toggle_deliverables(&mut self, cx: &mut Context<'_, Self>) {
        if self.right_drawer_open {
            self.close_right_sheet(cx);
            return;
        }
        let (latest_files, has_all, has_failures, has_artifacts) = {
            let feed = self.feed.read(cx);
            (
                feed.latest_deliverables(),
                feed.all_deliverable_items().is_some_and(|v| !v.is_empty()),
                !feed.failed_verifications().is_empty(),
                !feed.session_artifacts().is_empty(),
            )
        };
        self.clear_right_sheet();
        self.right_drawer_open = true;
        if let Some(files) = latest_files {
            self.deliverables =
                Some(crate::deliverables::DeliverableScope::Latest(files));
            self.deliverables_view_scope =
                crate::deliverables::DeliverablesViewScope::Latest;
            self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        } else if has_all || has_failures || has_artifacts {
            self.deliverables_view_scope = if has_all {
                crate::deliverables::DeliverablesViewScope::All
            } else {
                crate::deliverables::DeliverablesViewScope::Latest
            };
            self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        } else {
            self.drawer_tab = crate::deliverables::RightDrawerTab::Steps;
        }
        self.refresh_drawer_metadata(cx);
        self.close_all_popovers();
        cx.notify();
    }

    /// Closes whichever right-hand sheet is open (deliverables or single-work
    /// step inspector).
    pub(crate) fn close_right_sheet(&mut self, cx: &mut Context<'_, Self>) {
        self.clear_right_sheet();
        cx.notify();
    }

    /// Syncs whether the left dock sidebar is visible so the right drawer can
    /// clamp its width to keep the central chat canvas legible.
    pub fn set_dock_visible(
        &mut self,
        visible: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if self.dock_visible != visible {
            self.dock_visible = visible;
            cx.notify();
        }
    }

    /// Number of deliverables available in the current session's latest turn.
    pub(crate) fn deliverable_count(&self, cx: &Context<'_, Self>) -> usize {
        self.feed.read(cx).latest_deliverable_count()
    }

    /// Picks a file to preview asynchronously inside the drawer; `None` returns
    /// to the deliverables list.
    pub(crate) fn set_preview_file(
        &mut self,
        path: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(target) = path.as_deref()
            && self.feed.read(cx).is_path_failed(target)
        {
            self.drawer_preview.clear();
            self.emit_notice(Some("verify.card.failed"), cx);
            return;
        }
        self.drawer_preview.clear();
        let Some(target) = path else {
            cx.notify();
            return;
        };
        let generation = self.drawer_preview.generation;
        self.drawer_preview.file = Some(target.clone());
        self.drawer_preview.mode = PreviewMode::Rendered;
        self.drawer_preview.body = Some(DrawerPreviewBody::Loading);
        let load_path = target;
        let job = cx.background_executor().spawn(async move {
            crate::drawer_preview::load_drawer_preview(&load_path)
        });
        self.drawer_preview.task =
            Some(cx.spawn(async move |weak, cx| {
                let loaded = job.await;
                let _ =
                    weak.update(cx, |panel, cx| {
                        if panel.drawer_preview.generation != generation {
                            return;
                        }
                        if panel.drawer_preview.file.as_deref().is_some_and(
                            |p| panel.feed.read(cx).is_path_failed(p),
                        ) {
                            panel.drawer_preview.clear();
                            panel.emit_notice(Some("verify.card.failed"), cx);
                            return;
                        }
                        panel.drawer_preview.body = Some(loaded);
                        panel.drawer_preview.task = None;
                        cx.notify();
                    });
            }));
        cx.notify();
    }

    pub(crate) fn toggle_drawer_preview_mode(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        self.drawer_preview.mode = match self.drawer_preview.mode {
            PreviewMode::Rendered => PreviewMode::Source,
            PreviewMode::Source => PreviewMode::Rendered,
        };
        cx.notify();
    }

    pub(crate) fn copy_drawer_preview(
        &mut self,
        generation: u64,
        text: String,
        cx: &mut Context<'_, Self>,
    ) {
        if self.drawer_preview.generation != generation {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.drawer_preview.copied_generation = Some(generation);
        self.drawer_preview.copy_task =
            Some(cx.spawn(async move |weak, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(1500))
                    .await;
                let _ = weak.update(cx, |panel, cx| {
                    if panel.drawer_preview.copied_generation
                        == Some(generation)
                    {
                        panel.drawer_preview.copied_generation = None;
                        panel.drawer_preview.copy_task = None;
                        cx.notify();
                    }
                });
            }));
        cx.notify();
    }

    pub(crate) fn toggle_drawer_width(&mut self, cx: &mut Context<'_, Self>) {
        self.drawer_wide = !self.drawer_wide;
        cx.notify();
    }

    pub(crate) fn set_deliverables_view_scope(
        &mut self,
        scope: crate::deliverables::DeliverablesViewScope,
        cx: &mut Context<'_, Self>,
    ) {
        self.deliverables_view_scope = scope;
        if matches!(
            self.deliverables,
            Some(crate::deliverables::DeliverableScope::TurnSnapshot(_))
        ) {
            self.deliverables = self
                .feed
                .read(cx)
                .latest_deliverables()
                .map(crate::deliverables::DeliverableScope::Latest);
        }
        self.refresh_drawer_metadata(cx);
        cx.notify();
    }

    pub(crate) fn toggle_step_turn(
        &mut self,
        session_id: &str,
        turn_ix: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let entry = self
            .step_turn_toggled
            .entry(session_id.to_owned())
            .or_default();
        if !entry.remove(&turn_ix) {
            entry.insert(turn_ix);
        }
        cx.notify();
    }

    pub(crate) fn toggle_step_item(
        &mut self,
        item_id: String,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.expanded_step_items.remove(&item_id) {
            self.expanded_step_items.insert(item_id);
        }
        cx.notify();
    }

    pub(crate) fn copy_drawer_step_text(
        &mut self,
        item_id: String,
        text: String,
        cx: &mut Context<'_, Self>,
    ) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.drawer_step_copy_epoch =
            self.drawer_step_copy_epoch.wrapping_add(1);
        let epoch = self.drawer_step_copy_epoch;
        self.drawer_step_copied = Some((item_id.clone(), epoch));
        self.drawer_step_copy_task = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = weak.update(cx, |panel, cx| {
                if panel.drawer_step_copied == Some((item_id, epoch)) {
                    panel.drawer_step_copied = None;
                    panel.drawer_step_copy_task = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(crate) fn open_step_file_in_deliverables(
        &mut self,
        path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        self.inspected_work = None;
        if self.deliverables.is_none() {
            self.deliverables = self
                .feed
                .read(cx)
                .latest_deliverables()
                .map(crate::deliverables::DeliverableScope::Latest);
        }
        if self.feed.read(cx).is_path_failed(path) {
            self.drawer_preview.clear();
            self.emit_notice(Some("verify.card.failed"), cx);
            return;
        }
        let target_key = crate::ports::canonical_path_key(path);
        let in_latest =
            self.feed.read(cx).latest_deliverable_items().is_some_and(
                |items| {
                    items.iter().any(|item| {
                        crate::ports::canonical_path_key(&item.entry.path)
                            == target_key
                    })
                },
            );
        if !in_latest {
            self.deliverables_view_scope =
                crate::deliverables::DeliverablesViewScope::All;
        }
        self.refresh_drawer_metadata(cx);
        self.set_preview_file(Some(path.to_owned()), cx);
    }

    pub(crate) fn quote_file_to_composer(
        &mut self,
        path: &str,
        window: Option<&mut Window>,
        cx: &mut Context<'_, Self>,
    ) {
        let name = crate::ports::basename(path);
        let display = crate::ports::display_path_str(path);
        let quote_text = self.tf(
            "drawer.quote_file_text",
            &[("name", &name), ("path", &display)],
        );
        self.set_active_quote(
            crate::composer::QuoteRole::Assistant,
            &quote_text,
            cx,
        );
        if let Some(window) = window {
            self.prompt.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    pub(crate) fn request_fix_in_chat(
        &mut self,
        summary: String,
        path: Option<String>,
        window: Option<&mut Window>,
        cx: &mut Context<'_, Self>,
    ) {
        let fix_text = if let Some(path) = path.as_deref() {
            let name = crate::ports::basename(path);
            self.tf(
                "drawer.fix_prompt",
                &[("name", &name), ("issue", &summary)],
            )
        } else {
            self.tf("drawer.fix_prompt_generic", &[("issue", &summary)])
        };
        if self.prompt_text(cx).trim().is_empty() {
            self.fill_prompt(&fix_text, cx);
        } else {
            // Preserve the user's in-progress draft and attach fix context as a quote.
            self.set_active_quote(
                crate::composer::QuoteRole::Assistant,
                &fix_text,
                cx,
            );
        }
        if let Some(window) = window {
            self.prompt.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    pub(crate) fn locate_verification_card(
        &self,
        report_id: &str,
        artifact_path: &str,
        window: Option<&mut Window>,
        cx: &mut Context<'_, Self>,
    ) {
        let found = self.feed.update(cx, |feed, cx| {
            feed.focus_verification_row(report_id, artifact_path, cx)
        });
        if found && let Some(window) = window {
            window.refresh();
        }
        cx.notify();
    }

    pub(crate) fn visible_workspace_deliverables(
        &self,
        cx: &Context<'_, Self>,
    ) -> Vec<crate::feed::DeliverableItem> {
        let feed = self.feed.read(cx);
        match &self.deliverables {
            Some(crate::deliverables::DeliverableScope::TurnSnapshot(
                files,
            )) => feed.filter_settled_deliverable_items(files.clone()),
            _ => match self.deliverables_view_scope {
                crate::deliverables::DeliverablesViewScope::Latest => feed
                    .latest_deliverable_items()
                    .or_else(|| feed.all_deliverable_items())
                    .unwrap_or_default(),
                crate::deliverables::DeliverablesViewScope::All => {
                    feed.all_deliverable_items().unwrap_or_default()
                }
            },
        }
    }

    pub(crate) fn refresh_drawer_metadata(&mut self, cx: &Context<'_, Self>) {
        self.drawer_meta_generation =
            self.drawer_meta_generation.wrapping_add(1);
        let generation = self.drawer_meta_generation;
        let items = self.visible_workspace_deliverables(cx);
        let paths: Vec<String> =
            items.into_iter().map(|item| item.entry.path).collect();
        if paths.is_empty() {
            self.deliverable_sizes.clear();
            self.promote_project_candidate = None;
            self.drawer_meta_task = None;
            return;
        }
        let check_promote = self.active_project_id.is_none();
        let store_root = self.store_root.clone();
        let job = cx.background_executor().spawn(async move {
            let mut sizes =
                std::collections::HashMap::with_capacity(paths.len());
            for path in &paths {
                let key = crate::ports::canonical_path_key(path);
                let size = crate::ports::resolve_safe_local_file(path)
                    .and_then(|safe| {
                        std::fs::symlink_metadata(&safe).ok().map(|m| (safe, m))
                    })
                    .filter(|(safe, meta)| {
                        meta.is_file()
                            && !crate::ports::is_link_or_unsafe_reparse(
                                safe, meta,
                            )
                    })
                    .map(|(_, meta)| meta.len());
                sizes.insert(key, size);
            }
            let promote_candidate = if check_promote {
                paths.first().and_then(|first| {
                    let parent_dir =
                        crate::ports::resolve_safe_parent_dir(first)?;
                    if parent_dir.parent().is_some()
                        && !crate::ports::is_same_or_overlapping_dir(
                            &parent_dir,
                            &store_root,
                        )
                    {
                        Some(first.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            };
            (sizes, promote_candidate)
        });
        self.drawer_meta_task = Some(cx.spawn(async move |weak, cx| {
            let (sizes, promote_candidate) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                if panel.drawer_meta_generation != generation {
                    return;
                }
                panel.deliverable_sizes = sizes;
                panel.promote_project_candidate = promote_candidate;
                panel.drawer_meta_task = None;
                cx.notify();
            });
        }));
    }

    fn sync_right_drawer_deliverables(&mut self, cx: &mut Context<'_, Self>) {
        if !self.right_drawer_open {
            return;
        }
        let mut changed = false;
        let next = match &self.deliverables {
            Some(crate::deliverables::DeliverableScope::TurnSnapshot(
                existing,
            )) => {
                let filtered = self
                    .feed
                    .read(cx)
                    .filter_settled_deliverables(existing.clone());
                (!filtered.is_empty()).then_some(
                    crate::deliverables::DeliverableScope::TurnSnapshot(
                        filtered,
                    ),
                )
            }
            Some(crate::deliverables::DeliverableScope::Latest(_)) | None => {
                self.feed
                    .read(cx)
                    .latest_deliverables()
                    .map(crate::deliverables::DeliverableScope::Latest)
            }
        };
        if next != self.deliverables {
            self.deliverables = next;
            self.refresh_drawer_metadata(cx);
            changed = true;
        }
        if self
            .drawer_preview
            .file
            .as_deref()
            .is_some_and(|path| self.feed.read(cx).is_path_failed(path))
        {
            self.drawer_preview.clear();
            self.emit_notice(Some("verify.card.failed"), cx);
            changed = false;
        }
        if changed {
            cx.notify();
        }
    }

    pub fn create_project(
        &mut self,
        name: &str,
        folder: &std::path::Path,
        cx: &mut Context<'_, Self>,
    ) -> Result<crate::projects::Project, &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        let project = crate::projects::create(
            &self.store_root,
            &mut self.projects,
            name,
            folder,
        )
        .map_err(|error| error.notice_key())?;
        self.open_project(&project.id, cx)?;
        self.refresh_board_snapshot();
        self.refresh_local_env();
        Ok(project)
    }

    pub fn update_project(
        &mut self,
        id: &str,
        name: &str,
        folder: &std::path::Path,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        let project = crate::projects::update(
            &self.store_root,
            &mut self.projects,
            id,
            name,
            folder,
        )
        .map_err(|error| error.notice_key())?;
        if self.active_project_id.as_deref() == Some(id) {
            self.agent
                .set_workspace_for_new_sessions(Some(AgentWorkspace {
                    project_id: project.id,
                    root: project.root,
                }));
        }
        self.refresh_board_snapshot();
        self.refresh_local_env();
        cx.notify();
        Ok(())
    }

    pub fn remove_project(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        let was_active = self.active_project_id.as_deref() == Some(id);
        crate::projects::remove(&self.store_root, &mut self.projects, id)
            .map_err(|error| error.notice_key())?;
        if self.board_project_filter.as_deref() == Some(id) {
            self.board_project_filter = None;
        }
        if was_active {
            if let Some(sid) = self.agent.active_session_id() {
                self.draft_quotes.remove(&sid);
            }
            self.active_quote = None;
            self.quote_session = None;
            self.active_project_id = None;
            let _ = self.agent.clear_active();
            self.agent.set_workspace_for_new_sessions(None);
            self.surface = StudioSurface::Home;
            self.board_inspector.close_drawer();
            self.clear_right_sheet();
            self.refresh_chat(cx);
            self.emit_notice(None, cx);
        }
        self.refresh_board_snapshot();
        self.refresh_local_env();
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    pub fn set_project_pinned(
        &mut self,
        id: &str,
        pinned: bool,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        crate::projects::set_pinned(
            &self.store_root,
            &mut self.projects,
            id,
            pinned,
        )
        .map_err(|error| error.notice_key())?;
        cx.notify();
        Ok(())
    }

    pub fn view_project(&self, id: &str) -> Result<(), &'static str> {
        let project = self
            .projects
            .iter()
            .find(|project| project.id == id)
            .ok_or("project.folder_unavailable")?;
        if !crate::projects::root_available(&project.root) {
            return Err("project.folder_unavailable");
        }
        let safe_dir = crate::ports::resolve_safe_local_dir(&project.root)
            .ok_or("project.folder_unavailable")?;
        open::that(&safe_dir).map_err(|_| "project.view_failed")
    }

    pub fn open_project(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
        else {
            return Err("project.folder_unavailable");
        };
        if !crate::projects::root_available(&project.root) {
            self.emit_notice(Some("project.folder_unavailable"), cx);
            return Err("project.folder_unavailable");
        }
        let clear_error = self.agent.clear_active().err();
        self.invalidate_submission(cx);
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.apply_draft_project(Some(&project));
        self.surface = StudioSurface::Home;
        self.set_prompt_text("", cx);
        self.refresh_chat(cx);
        self.refresh_popover_works();
        self.refresh_local_env();
        self.emit_notice(clear_error, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    pub fn note(&mut self, key: &'static str, cx: &mut Context<'_, Self>) {
        self.emit_notice(Some(key), cx);
    }

    pub fn open_thread(&mut self, title: String, cx: &mut Context<'_, Self>) {
        self.set_prompt_text(title, cx);
        self.send_prompt(cx);
    }

    pub fn clear_thread(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(sid) = self.agent.active_session_id() {
            self.draft_quotes.remove(&sid);
        }
        self.active_quote = None;
        self.quote_session = None;
        self.new_task(cx);
    }

    pub fn new_conversation(&mut self, cx: &mut Context<'_, Self>) {
        self.new_task(cx);
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
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::{cell::RefCell, rc::Rc};

    /// Builds a panel inside its own window: `StudioPanel::new` needs the
    /// window that owns its deferred work, and the tests only ever read state.
    fn open_panel(
        cx: &TestAppContext,
        build: impl FnOnce(
            &mut Window,
            &mut Context<'_, StudioPanel>,
        ) -> StudioPanel,
    ) -> Entity<StudioPanel> {
        let mut panel = None;
        cx.update(|cx| {
            let slot = &mut panel;
            cx.open_window(
                gpui::WindowOptions::default(),
                move |window, cx| {
                    let built = cx.new(|cx| build(window, cx));
                    *slot = Some(built.clone());
                    built
                },
            )
            .expect("the test window opens");
        });
        panel.expect("the panel was built with its window")
    }

    /// The prompt text the user sees: queued programmatic text wins over the
    /// editor state, which only receives it at the top of `render`.
    fn visible_prompt(panel: &StudioPanel, cx: &gpui::App) -> String {
        match panel.prompt_pending.as_deref() {
            Some(pending) => pending.to_owned(),
            None => panel.prompt.read(cx).value().to_string(),
        }
    }

    fn submission_panel(
        cx: &mut TestAppContext,
        tag: &str,
    ) -> (Entity<StudioPanel>, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-submit-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            panel.catalog.replace(vec![flowy_agent::CatalogModel {
                id: "test-model".into(),
                name: "Test model".into(),
                ..Default::default()
            }]);
            panel.cloud_ready = true;
            cx.notify();
        });
        (panel, root)
    }

    #[gpui::test]
    fn repeated_send_while_submitting_persists_one_user_message(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "duplicate");
        panel.update(cx, |panel, cx| {
            panel.fill_prompt("one request", cx);
            panel.send_prompt(cx);
            let id = panel.submission;
            panel.send_prompt(cx);
            assert_eq!(panel.submission, id);
            assert!(panel.submission_pending);
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            assert!(!panel.submission_pending);
            assert_eq!(panel.prompt.read(cx).value(), "");
            assert_eq!(
                panel
                    .agent
                    .chat_history()
                    .rows
                    .iter()
                    .filter(|row| matches!(
                        row,
                        flowy_agent::ChatLine::User { .. }
                    ))
                    .count(),
                1
            );
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn a_busy_submission_shows_up_in_the_pending_strip(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "pending-strip");
        panel.update(cx, |panel, cx| {
            let id = panel.agent.create_home_session().unwrap();
            panel.open_session(&id, cx);
            panel.status = flowy_agent::TurnStatus::Running;
            panel.refresh_working(cx);
            assert_eq!(panel.pending_receipts.len(), 0);

            panel.fill_prompt("queued while the turn runs", cx);
            panel.send_prompt(cx);

            // The strip is rendered from this field, so a non-empty field is
            // what "the user can find it" means at the UI layer.
            assert_eq!(panel.pending_receipts.len(), 1);
            assert_eq!(
                panel.pending_receipts[0].text,
                "queued while the turn runs"
            );
            // The old lie is gone: no "added to this session's task list"
            // notice is raised for a list that is now real.
            assert_eq!(panel.notice, None);
        });

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn a_restarted_panel_rebuilds_the_pending_strip_from_the_journal(
        cx: &mut TestAppContext,
    ) {
        // First run: accept while busy.
        let (panel, root) = submission_panel(cx, "pending-restart");
        let first_id = panel.update(cx, |panel, cx| {
            let id = panel.agent.create_home_session().unwrap();
            panel.open_session(&id, cx);
            panel.status = flowy_agent::TurnStatus::Running;
            panel.refresh_working(cx);
            panel.fill_prompt("survive the restart", cx);
            panel.send_prompt(cx);
            panel.pending_receipts[0].id.clone()
        });
        drop(panel);

        // Second run over the same store root: the constructor reads the
        // journal, so the strip is populated before anything is submitted.
        let reopened = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        cx.read_entity(&reopened, |panel, _cx| {
            assert_eq!(
                panel.pending_receipts.len(),
                1,
                "a restart must still show what was accepted"
            );
            assert_eq!(panel.pending_receipts[0].id, first_id);
        });

        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn a_restarted_panel_drains_the_queued_receipt_without_another_submit(
        cx: &mut TestAppContext,
    ) {
        // First run: accept while busy, then die with the receipt still queued.
        let (panel, root) = submission_panel(cx, "restart-drain");
        let receipt_id = panel.update(cx, |panel, cx| {
            let id = panel.agent.create_home_session().unwrap();
            panel.open_session(&id, cx);
            panel.status = flowy_agent::TurnStatus::Running;
            panel.refresh_working(cx);
            panel.fill_prompt("drain me after restart", cx);
            panel.send_prompt(cx);
            panel.pending_receipts[0].id.clone()
        });
        drop(panel);

        // Second run: nothing is submitted; startup alone must claim it.
        let reopened = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        // Deliver the startup event the way the pump would after the agent
        // reports readiness; the panel must claim the waiting receipt itself.
        reopened.update(cx, |panel, cx| {
            panel.process_agent_updates(
                vec![flowy_agent::AgentUpdate::Initialized],
                cx,
            );
        });
        let queued = flowy_agent::AcceptanceQueue::new(root.clone())
            .all()
            .expect("acceptance journal readable");
        let receipt = queued
            .iter()
            .find(|receipt| receipt.id == receipt_id)
            .expect("the queued receipt is still on disk");
        assert!(
            receipt.taken_at_ms.is_some(),
            "startup must claim the queued receipt without a second submit"
        );

        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn a_manual_submit_keeps_the_queue_waiting(cx: &mut TestAppContext) {
        let (panel, root) = submission_panel(cx, "drain-guard");
        let receipt_id = panel.update(cx, |panel, cx| {
            let id = panel.agent.create_home_session().unwrap();
            panel.open_session(&id, cx);
            panel.status = flowy_agent::TurnStatus::Running;
            panel.refresh_working(cx);
            panel.fill_prompt("wait for the submit", cx);
            panel.send_prompt(cx);
            let receipt_id = panel.pending_receipts[0].id.clone();
            panel.initialized = true;
            panel.submission_pending = true;
            panel.refresh_pending_receipts(cx);
            receipt_id
        });
        let queued = flowy_agent::AcceptanceQueue::new(root.clone())
            .all()
            .expect("acceptance journal readable");
        let receipt = queued
            .iter()
            .find(|receipt| receipt.id == receipt_id)
            .expect("the queued receipt is still on disk");
        assert!(
            receipt.taken_at_ms.is_none(),
            "a manual submit must not be interleaved with a queued receipt"
        );

        panel.update(cx, |panel, cx| {
            panel.submission_pending = false;
            panel.status = flowy_agent::TurnStatus::Idle;
            panel.refresh_pending_receipts(cx);
        });
        let queued = flowy_agent::AcceptanceQueue::new(root.clone())
            .all()
            .expect("acceptance journal readable");
        let receipt = queued
            .iter()
            .find(|receipt| receipt.id == receipt_id)
            .expect("the queued receipt is still on disk");
        assert!(
            receipt.taken_at_ms.is_some(),
            "once idle the queued receipt starts"
        );

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn a_queued_receipt_names_the_session_it_belongs_to(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "pending-owner");
        panel.update(cx, |panel, cx| {
            let first = panel.agent.create_home_session().unwrap();
            panel.open_session(&first, cx);
            panel.status = flowy_agent::TurnStatus::Running;
            panel.refresh_working(cx);
            panel.fill_prompt("stay in this session", cx);
            panel.send_prompt(cx);
            let receipt = panel.pending_receipts[0].clone();
            assert_eq!(
                panel.receipt_owner(&receipt),
                panel.t("chat.pending_here")
            );

            // A receipt from another session is labelled by that session, not
            // by whichever one happens to be active now.
            let mut foreign = receipt;
            foreign.session_hint = Some("elsewhere".to_owned());
            let owner = panel.receipt_owner(&foreign);
            assert_ne!(owner, panel.t("chat.pending_here"));
            assert_ne!(owner, panel.t("chat.pending_unassigned"));
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn sending_while_running_is_accepted_into_the_durable_queue(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "busy-accept");
        panel.update(cx, |panel, cx| {
            let id = panel.agent.create_home_session().unwrap();
            panel.open_session(&id, cx);
            // A turn is in flight: this is the state a user is in when the
            // send button used to disappear.
            panel.status = flowy_agent::TurnStatus::Running;
            panel.refresh_working(cx);

            panel.fill_prompt("second prompt while the first turn runs", cx);
            panel.send_prompt(cx);
        });

        // The submission must be durable, not just remembered.
        let pending = flowy_agent::AcceptanceQueue::new(root.clone())
            .pending()
            .expect("acceptance queue readable");
        assert_eq!(
            pending.len(),
            1,
            "busy submit must be accepted, not dropped"
        );
        assert_eq!(pending[0].text, "second prompt while the first turn runs");
        assert!(pending[0].taken_at_ms.is_none(), "nothing ran it yet");

        // The draft is handed over to the queue, so the composer is clear and
        // the user can keep typing.
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).value(), "");
        });

        // Reopening the client is a fresh queue handle over the same root:
        // no in-memory state is shared, only the receipt file.
        let reopened = flowy_agent::AcceptanceQueue::new(root.clone())
            .pending()
            .expect("reopened queue readable");
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened[0].id, pending[0].id);

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn an_accepted_receipt_is_drained_when_the_turn_finishes(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "accepted-drain");
        panel.update(cx, |panel, cx| {
            let id = panel.agent.create_home_session().unwrap();
            panel.open_session(&id, cx);
            panel.status = flowy_agent::TurnStatus::Running;
            panel.refresh_working(cx);
            panel.fill_prompt("queued work", cx);
            panel.send_prompt(cx);
            assert_eq!(panel.pending_receipts.len(), 1);

            // The running turn ends: the refresh drains the receipt and starts
            // it in its hinted session.
            panel.refresh_chat(cx);
            assert!(panel.agent.has_active_claim());
            assert_eq!(panel.pending_receipts.len(), 0);

            for _ in 0..200 {
                if !panel.agent.turn_status().is_busy() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panel.refresh_chat(cx);
            assert!(!panel.agent.has_active_claim());
            assert_eq!(panel.pending_receipts.len(), 0);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn retrying_delivery_shows_saving_state_and_blocks_reentry(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "retry-delivery");
        panel.update(cx, |panel, cx| {
            let id = panel.agent.create_home_session().unwrap();
            panel.open_session(&id, cx);
            panel.status = flowy_agent::TurnStatus::DeliveryFailed;
            panel.retrying_delivery = Some(id.clone());
            panel.refresh_working(cx);

            let label = panel.working.read(cx).label_for_test().to_owned();
            assert_eq!(label, panel.t("chat.status_saving_files"));
            assert!(!panel.working.read(cx).retry_enabled());

            // A second activation while the background retry is in flight is
            // ignored instead of queueing another job behind the dispatcher.
            panel.retry_delivery(cx);
            assert_eq!(panel.retrying_delivery.as_deref(), Some(id.as_str()));
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn failed_background_submission_keeps_draft_and_retry_identity(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "failure");
        let id = panel.update(cx, |panel, cx| {
            panel
                .agent
                .set_workspace_for_new_sessions(Some(AgentWorkspace {
                    project_id: "unavailable".into(),
                    root: root.join("missing-project"),
                }));
            panel.fill_prompt("keep for retry", cx);
            panel.send_prompt(cx);
            panel.submission.unwrap()
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).value(), "keep for retry");
            assert_eq!(panel.submission, Some(id));
            assert!(!panel.agent.has_messages());
            assert_eq!(panel.notice, Some("project.folder_unavailable"));
            panel.send_prompt(cx);
            assert_eq!(panel.submission, Some(id));
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).value(), "keep for retry");
            assert_eq!(panel.submission, Some(id));
            assert!(!panel.submission_pending);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn late_submission_does_not_clear_new_draft_or_change_selected_session(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "navigation");
        let next = panel.update(cx, |panel, cx| {
            let first = panel.agent.create_home_session().unwrap();
            let next = panel.agent.create_home_session().unwrap();
            panel.open_session(&first, cx);
            panel.fill_prompt("request for first", cx);
            panel.send_prompt(cx);
            panel.open_session(&next, cx);
            panel.fill_prompt("new session draft", cx);
            next
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(
                panel.agent.active_session_id().as_deref(),
                Some(next.as_str())
            );
            assert_eq!(panel.prompt.read(cx).value(), "new session draft");
            assert!(!panel.agent.has_messages());
            assert_eq!(panel.surface, StudioSurface::Home);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn background_initialized_event_keeps_home_and_preserves_unsent_draft(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-init-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::write(root.join("sessions/index.json"), r#"{"active":"restored","sessions":[{"id":"restored","title":"Saved conversation","updated_at":1,"lines":[{"kind":"user","text":"original request"},{"kind":"assistant","text":"Saved answer"}],"status":{"kind":"completed"},"origin":{"kind":"home"}}]}"#).unwrap();
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new_background(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        let agent = cx.read_entity(&panel, |panel, _| panel.agent.clone());
        // The loader is a real worker thread (SQLite + official skill seeding),
        // so yielding the test thread lets it finish without advancing the GPUI
        // timer pump or fabricating events. Allow generous wall-clock headroom
        // for parallel NTFS/SQLite contention on Windows CI runners.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut updates = Vec::new();
        loop {
            updates.extend(agent.drain_updates());
            if !agent.is_initializing()
                && updates.iter().any(|update| {
                    matches!(update, flowy_agent::AgentUpdate::Initialized)
                })
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background session initialization did not finish in time (initializing={}, updates={updates:?})",
                agent.is_initializing(),
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(updates.iter().any(|update| matches!(
            update,
            flowy_agent::AgentUpdate::Initialized
        )));
        panel.update(cx, |panel, cx| {
            panel.surface = StudioSurface::Home;
            panel.fill_prompt("keep this draft", cx);
            panel.process_agent_updates(updates, cx);
            assert_eq!(panel.notice, None);
            // Launching must not dive back into the last conversation just
            // because it still has history; the user asked for Home first.
            assert_eq!(panel.surface, StudioSurface::Home);
            assert_eq!(panel.agent.active_session_id().as_deref(), Some("restored"));
            // Programmatic fills are applied at the top of `render`; the test
            // reads the queued draft instead of driving a frame.
            assert_eq!(visible_prompt(panel, cx), "keep this draft");
            let history = panel.agent.chat_history();
            assert_eq!(history.rows.len(), 2);
            assert!(matches!(&history.rows[0], flowy_agent::ChatLine::User { text } if text == "original request"));
        });
        drop(panel);
        drop(agent);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn creating_a_project_persists_and_opens_its_workspace(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-project-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let folder = root.join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: String::new(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        let created = panel
            .update(cx, |panel, cx| panel.create_project("Client", &folder, cx))
            .unwrap();
        assert_eq!(
            crate::projects::load(&root).unwrap(),
            vec![created.clone()]
        );
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(created.id.as_str())
            );
            assert_eq!(
                panel.agent.active_workspace(),
                Some(flowy_agent::AgentWorkspace {
                    project_id: created.id.clone(),
                    root: created.root.clone(),
                })
            );
        });

        let session = cx.read_entity(&panel, |panel, _| {
            panel.agent.create_session().unwrap()
        });
        panel.update(cx, |panel, cx| panel.open_session(&session, cx));
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(created.id.as_str())
            );
            assert_eq!(
                panel
                    .agent
                    .active_workspace()
                    .as_ref()
                    .map(|value| &value.root),
                Some(&created.root)
            );
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn persisted_error_is_visible_on_the_first_frame(cx: &TestAppContext) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-working-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        // The agent boots from its structured index, the panel projection from
        // the UI index; both must agree that the active session ended in error.
        std::fs::write(
            sessions.join("history.json"),
            r#"{"active":"s1","sessions":[{"id":"s1","status":{"kind":"error","message":"chat.status_error"}}]}"#,
        )
        .unwrap();
        std::fs::write(
            sessions.join("index.json"),
            r#"{"active":"s1","sessions":[{"id":"s1","title":"t","updated_at":1,"titled":true,"lines":[],"status":{"kind":"error","message":"chat.status_error"}}]}"#,
        )
        .unwrap();
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: String::new(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(
                panel.status,
                flowy_agent::TurnStatus::Error {
                    message: "chat.status_error".into(),
                }
            );
            let working = panel.working.read(cx);
            assert!(working.is_visible());
            assert!(working.is_error());
            let expected = panel.i18n.t("chat.status_error");
            assert_ne!(expected, "chat.status_error");
            assert_eq!(working.label(), expected);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn chat_surface_enter_submits_through_the_chat_composer(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-chat-enter-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: String::new(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        cx.update(|window, cx| {
            // The composer's editor advertises the `Input` key context, the
            // same one the app binds its submit chord against.
            cx.bind_keys([gpui::KeyBinding::new(
                "enter",
                crate::SubmitComposer,
                Some("Input"),
            )]);
            panel.update(cx, |panel, cx| {
                panel.catalog.replace(vec![flowy_agent::CatalogModel {
                    id: "mock-model".into(),
                    name: "Mock model".into(),
                    ..Default::default()
                }]);
                panel.cloud_ready = true;
                panel.set_surface(StudioSurface::Chat, cx);
                panel.fill_prompt("hello", cx);
            });
            use gpui::Focusable;
            let handle = panel.read(cx).prompt.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            // Submitting queues the cleared draft for the next render.
            assert_eq!(visible_prompt(panel, cx), "");
            assert!(panel.agent.has_messages());
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn enter_submits_the_draft_and_shift_enter_inserts_a_newline(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-enter-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: String::new(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        cx.update(|window, cx| {
            // The app installs these in `flowy_app::app::run`; a studio-only
            // test has to install the same bindings to exercise the handler.
            cx.bind_keys([
                gpui::KeyBinding::new(
                    "enter",
                    crate::SubmitComposer,
                    Some("Input"),
                ),
                gpui::KeyBinding::new(
                    "shift-enter",
                    gpui_kit::base::input::Enter {
                        secondary: false,
                        shift: true,
                    },
                    Some("Input"),
                ),
            ]);
            panel.update(cx, |panel, cx| {
                panel.catalog.replace(vec![flowy_agent::CatalogModel {
                    id: "mock-model".into(),
                    name: "Mock model".into(),
                    ..Default::default()
                }]);
                panel.cloud_ready = true;
                panel.fill_prompt("hello", cx);
            });
            use gpui::Focusable;
            let handle = panel.read(cx).prompt.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        // Home surface: Shift+Enter inserts a newline, Enter submits.
        cx.simulate_keystrokes("shift-enter");
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(visible_prompt(panel, cx), "hello\n");
        });
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            // Submitting queues the cleared draft for the next render.
            assert_eq!(visible_prompt(panel, cx), "");
            assert!(panel.agent.has_messages());
        });

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_turn_keeps_the_working_line_visible() {
        use flowy_agent::TurnStatus;

        assert!(working_line_visible(&TurnStatus::Running));
        assert!(working_line_visible(&TurnStatus::Stopping));
        assert!(working_line_visible(&TurnStatus::WaitingForUser));
        assert!(working_line_visible(&TurnStatus::Error {
            message: "chat.status_error".into(),
        }));
        assert!(working_line_visible(&TurnStatus::DeliveryFailed));
        assert!(!working_line_visible(&TurnStatus::Idle));
        assert!(!working_line_visible(&TurnStatus::Completed));
        assert!(!working_line_visible(&TurnStatus::Stopped));
    }

    #[gpui::test]
    fn background_session_updates_do_not_change_selected_surface(
        cx: &TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-navigation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: String::new(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        let notices = Rc::new(RefCell::new(Vec::new()));
        let subscription = cx.update(|cx| {
            cx.subscribe(&panel, {
                let notices = notices.clone();
                move |_, event: &StudioEvent, _| {
                    if let StudioEvent::Notice(notice) = event {
                        notices.borrow_mut().push(*notice);
                    }
                }
            })
        });
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.catalog.replace(vec![flowy_agent::CatalogModel {
                    id: "test-model".into(),
                    name: "Test model".into(),
                    ..Default::default()
                }]);
                panel.cloud_ready = true;
                panel.fill_prompt("hello", cx);
                panel.send_prompt(cx);
                assert!(panel.submission_pending);
                panel.set_surface(StudioSurface::Automation, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                let updates = panel.agent.drain_updates();
                panel.process_agent_updates(updates, cx);
                assert_eq!(panel.surface, StudioSurface::Automation);
                panel.process_agent_updates(
                    vec![flowy_agent::AgentUpdate::StorageError],
                    cx,
                );
                assert_eq!(panel.notice, Some("notice.session_storage_failed"));
                assert_eq!(panel.surface, StudioSurface::Automation);
            });
        });
        cx.update(|cx| panel.update(cx, StudioPanel::new_task));
        assert_eq!(notices.borrow().last(), Some(&None));
        drop(subscription);
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }
    #[gpui::test]
    fn theme_changes_preserve_draft_and_do_not_leak_between_panels(
        cx: &TestAppContext,
    ) {
        let root = std::env::temp_dir()
            .join(format!("flowy-studio-theme-{}", std::process::id()));
        let (first_ui, second_ui, theme) = cx.update(|cx| {
            let first_ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            let second_ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            let theme = first_ui.theme.clone();
            (first_ui, second_ui, theme)
        });
        let panel_config = |root: &PathBuf| StudioConfig {
            product: "Flowy",
            i18n: I18n::embedded("Flowy"),
            model: Model::Lite,
            plan: Plan::Free,
            model_info_version: String::new(),
            model_info_open: false,
            store_root: root.clone(),
            llm: LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: String::new(),
                ..LlmConfig::default()
            },
        };
        let first = open_panel(cx, {
            let config = panel_config(&root);
            move |window, cx| StudioPanel::new(config, first_ui, window, cx)
        });
        let second = open_panel(cx, {
            let config = panel_config(&root);
            move |window, cx| StudioPanel::new(config, second_ui, window, cx)
        });
        cx.update(|cx| {
            first.update(cx, |panel, cx| {
                panel.fill_prompt("preserve draft", cx);
            });
        });
        cx.update(|cx| {
            theme.update(cx, |theme, cx| {
                theme.update(
                    flowy_ui::ThemeOptions {
                        mode: flowy_ui::ThemeMode::Dark,
                        ..flowy_ui::ThemeOptions::default()
                    },
                    false,
                    cx,
                );
            });
        });
        cx.update(|cx| {
            assert!(first.read(cx).theme.dark);
            assert!(!second.read(cx).theme.dark);
            assert_eq!(
                first.read(cx).prompt.read(cx).value(),
                "preserve draft"
            );
        });
        drop(first);
        drop(second);
        drop(theme);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn catalog_loading_holds_queued_receipts_and_queues_new_prompts(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-catalog-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        flowy_agent::AcceptanceQueue::new(root.clone())
            .accept(None, "persisted before restart", 1)
            .unwrap();
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: "flowy".into(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            panel.catalog_loading = true;
            panel.cloud_ready = false;
            panel.process_agent_updates(
                vec![flowy_agent::AgentUpdate::Initialized],
                cx,
            );
            assert_eq!(
                panel.pending_receipts.len(),
                1,
                "startup before catalog arrives must not drain into echo"
            );
            assert!(!panel.agent.has_active_claim());

            // Typing and sending while catalog is loading accepts into the
            // durable queue instead of failing with notice.agent_failed.
            panel.fill_prompt("typed while catalog loads", cx);
            panel.send_prompt(cx);
            assert_eq!(panel.notice, None);
            assert_eq!(panel.pending_receipts.len(), 2);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn switching_to_home_neither_settles_a_running_claim_nor_binds_home(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "claim-isolation");
        panel.update(cx, |panel, cx| {
            let hinted = panel.agent.create_home_session().unwrap();
            panel.open_session(&hinted, cx);
            panel
                .agent
                .accept_deferred("finish in hinted session")
                .unwrap();

            // User goes back to a blank Home screen before the receipt drains.
            panel.new_task(cx);
            assert_eq!(panel.surface, StudioSurface::Home);
            assert_eq!(panel.agent.active_session_id(), None);

            panel.initialized = true;
            panel.refresh_pending_receipts(cx);
            assert!(panel.agent.has_active_claim());
            assert_eq!(
                panel.agent.active_session_id(),
                None,
                "background drain must not bind a blank Home screen to the old session"
            );

            // Wait for the background turn to finish, then verify the claim settles.
            for _ in 0..200 {
                if panel
                    .agent
                    .claimed_session_status()
                    .is_none_or(|status| !status.is_busy())
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panel.refresh_pending_receipts(cx);
            assert!(!panel.agent.has_active_claim());
            assert_eq!(panel.pending_receipts.len(), 0);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn override_delivery_writes_audit_chain_and_persists_badge_across_reopen(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "override-persist");
        let (session_id, report_id) = panel.update(cx, |panel, cx| {
            let session_id = panel.agent.create_home_session().unwrap();
            panel.open_session(&session_id, cx);
            let workspace =
                root.join("sessions").join("workspaces").join(&session_id);
            let output_dir = workspace.join("outputs/submission-override-test");
            std::fs::create_dir_all(&output_dir).unwrap();
            let deck = output_dir.join("deck.pptx");
            std::fs::write(
                &deck,
                include_bytes!("../examples/fixtures/preview/sample.pptx"),
            )
            .unwrap();

            let gate = flowy_agent::delivery::DeliveryStore::new(
                root.join("delivery"),
            );
            let report = gate
                .record_report(
                    &deck,
                    vec![flowy_agent::delivery::Issue {
                        code: "VALUE_MISMATCH".into(),
                        field: "S0-title".into(),
                    }],
                    100,
                )
                .unwrap();
            assert!(!report.ok);

            panel.confirm_override_for_session(
                &session_id,
                &report.id,
                "outputs/submission-override-test/deck.pptx",
                cx,
            );
            (session_id, report.id)
        });
        let _ = report_id;
        cx.run_until_parked();
        let artifact_id = panel.read_with(cx, |panel, _| {
            assert_eq!(panel.notice, None);
            panel
                .agent
                .chat_history()
                .rows
                .iter()
                .find_map(|line| {
                    if let flowy_agent::ChatLine::Artifact { artifact_id } =
                        line
                    {
                        Some(artifact_id.clone())
                    } else {
                        None
                    }
                })
                .expect("override delivery appends artifact card")
        });

        let events =
            std::fs::read_to_string(root.join("delivery/events.jsonl"))
                .unwrap();
        let blocked_pos =
            events.find("deliver_blocked").expect("blocked event");
        let override_pos =
            events.find("deliver_override").expect("override event");
        let deliver_pos = events.find("\"deliver\"").expect("deliver event");
        assert!(blocked_pos < override_pos && override_pos < deliver_pos);

        drop(panel);

        // Reopen over the same root and confirm both the artifact record and
        // the override marker survive restart.
        let reopened = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig::default(),
                },
                ui,
                window,
                cx,
            )
        });
        reopened.update(cx, |panel, cx| {
            panel.open_session(&session_id, cx);
            let store = panel.agent.artifact_store().unwrap();
            let record = store.get(&artifact_id).unwrap().unwrap();
            assert!(
                panel
                    .agent
                    .artifact_was_overridden(&record.manifest.receipt.sha256),
                "permanent override mark must survive panel reopen"
            );
            assert!(panel.agent.chat_history().rows.iter().any(|line| {
                matches!(line, flowy_agent::ChatLine::Artifact { artifact_id: id } if id == &artifact_id)
            }));
        });
        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn dead_receipt_lifecycle_surfaces_notice_and_syncs_visible_receipts(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "dead-receipt-state");
        panel.update(cx, |panel, cx| {
            panel.process_agent_updates(
                vec![flowy_agent::AgentUpdate::Receipt {
                    id: "rcpt-dead-1".into(),
                    state: ReceiptState::Dead,
                }],
                cx,
            );
            assert_eq!(panel.notice, Some("notice.receipt_gave_up"));
            assert_eq!(
                panel.visible_receipts,
                panel.agent.visible_acceptances()
            );

            panel.process_agent_updates(
                vec![flowy_agent::AgentUpdate::Receipt {
                    id: "rcpt-q-1".into(),
                    state: ReceiptState::Quarantined,
                }],
                cx,
            );
            assert_eq!(panel.notice, Some("notice.receipt_quarantined"));
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn autonomous_drain_apply_config_and_notify_cloud_ready_syncs_queue(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-auto-cloud-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: "flowy".into(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            panel.agent.set_autonomous_drain(true);
            let receipt = panel
                .agent
                .accept_deferred("wake after cloud ready")
                .expect("accepted");
            assert_eq!(panel.agent.pending_acceptances().len(), 1);
            // Kernel 65772f857 wakes autonomous drain inside `apply_config`
            // once the config becomes runnable; `notify_cloud_ready` only
            // syncs visible receipts and rebuilds the board snapshot.
            panel
                .agent
                .apply_config(LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: String::new(),
                    ..LlmConfig::default()
                })
                .expect("apply runnable config");
            panel.notify_cloud_ready(cx);
            assert!(panel.agent.autonomous_drain());
            let _ = receipt;
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn prepare_for_sign_out_cancels_queued_receipts_and_invalidates_submission(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "signout-cancel");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_check = stop.clone();
        panel.update(cx, |panel, cx| {
            let receipt = panel
                .agent
                .accept_deferred("queued before sign-out")
                .expect("accept deferred");
            panel.sync_receipts();
            assert_eq!(panel.visible_receipts.len(), 1);
            panel.submission_pending = true;
            panel.submission = Some(42);
            panel.submission_stop = Some(stop);
            panel.retrying_receipts.insert(receipt.id);
            panel.cached_trace = Some(("s1".into(), Vec::new()));

            panel.prepare_for_sign_out(
                LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: "flowy".into(),
                    ..LlmConfig::default()
                },
                cx,
            );

            assert!(stop_check.load(Ordering::Acquire));
            assert!(!panel.submission_pending);
            assert!(panel.submission.is_none());
            assert!(!panel.cloud_ready);
            assert!(!panel.catalog_loading);
            assert!(!panel.can_submit_prompt());
            assert_eq!(panel.retrying_receipts.len(), 0);
            assert!(panel.cached_trace.is_none());
            assert_eq!(panel.visible_receipts.len(), 0);
            assert_eq!(panel.agent.pending_acceptances().len(), 0);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn bind_default_workspace_preserves_prompt_draft(cx: &mut TestAppContext) {
        let (panel, root) = submission_panel(cx, "bind-default-workspace");
        let project_dir = root.join("alpha-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        panel.update(cx, |panel, cx| {
            panel
                .create_project("Alpha", &project_dir, cx)
                .expect("create project");
            assert!(panel.active_project_id.is_some());

            panel.fill_prompt("draft must survive workspace unbind", cx);
            panel.bind_default_workspace(cx);

            assert!(panel.active_project_id.is_none());
            assert!(panel.agent.active_workspace().is_none());
            assert_eq!(
                visible_prompt(panel, cx),
                "draft must survive workspace unbind"
            );
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn cloud_ready_gate_blocks_when_offline_and_allows_during_catalog_loading(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "cloud-gate");
        panel.update(cx, |panel, _cx| {
            panel
                .agent
                .apply_config(LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: "flowy".into(),
                    ..LlmConfig::default()
                })
                .unwrap();
            panel.cloud_ready = false;
            panel.catalog_loading = false;
            assert!(!panel.can_submit_prompt());

            panel.catalog_loading = true;
            assert!(panel.can_submit_prompt());

            panel.catalog_loading = false;
            panel.cloud_ready = true;
            assert!(panel.can_submit_prompt());
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn active_popover_enforces_mutual_exclusion_and_overlay_dismissal(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "popover-seam");
        panel.update(cx, |panel, cx| {
            assert_eq!(panel.active_popup_scope(), None);

            panel.toggle_popover(ActivePopover::EnvStatus);
            assert_eq!(panel.active_popup_scope(), Some("env-status-popover"));

            // Opening the unified `+` menu automatically replaces EnvStatus.
            panel.toggle_popover(ActivePopover::AddMenu);
            assert_eq!(panel.active_popup_scope(), Some("composer-add-menu"));
            assert!(panel.is_add_menu_open());

            // Opening the composer model menu replaces AddMenu (never two popovers).
            panel.toggle_popover(ActivePopover::ModelMenu);
            assert_eq!(panel.active_popup_scope(), Some("model-list"));
            assert!(panel.is_model_menu_open());
            assert!(!panel.is_add_menu_open());

            // Same popover toggles closed.
            panel.toggle_popover(ActivePopover::ModelMenu);
            assert_eq!(panel.active_popup_scope(), None);

            // Overlay dismissal only closes matching scope and also handles Flow Modal.
            panel.toggle_popover(ActivePopover::BgTasks);
            assert!(!panel.dismiss_overlay("model-list", cx));
            assert_eq!(panel.active_popup_scope(), Some("bg-tasks-popover"));
            assert!(panel.dismiss_overlay("bg-tasks-popover", cx));
            assert_eq!(panel.active_popup_scope(), None);

            // Opening a missing session must not leave a ghost inspector/drawer.
            panel.select_board_session("s-missing".into(), None, true, cx);
            assert_eq!(panel.board_inspector.session, None);
            assert!(!panel.has_board_drawer());

            // A resolvable session selects cleanly and narrow drawer dismisses via overlay scope.
            panel.surface = StudioSurface::Board;
            panel.board_width_px = 680.0;
            panel.board_snapshot.sessions.push(
                crate::ports::SessionBoardGroup {
                    session_id: "s-demo".into(),
                    title: "Demo".into(),
                    project_id: None,
                    project_name: None,
                    workspace_display: String::new(),
                    updated_at: 0,
                    pinned: false,
                    archived: false,
                    preview: None,
                    assistant_summary: None,
                    branches: vec![crate::ports::WorkBranchSummary {
                        id: "b-demo".into(),
                        owner_session_id: None,
                        skill_label: crate::ports::SkillLabel::Key(
                            "board.skill_work",
                        ),
                        phase: crate::ports::BranchPhase::Idle,
                        deliverable_name: "Demo".into(),
                        deliverable_path: None,
                        format: crate::ports::DeliverableFormat::Other,
                        is_workspace_change: false,
                        progress_pct: 100.0,
                        detail: String::new(),
                        work_progress: None,
                    }],
                    workspace_changes: Vec::new(),
                    child_workers: Vec::new(),
                    total_branch_count: 1,
                    total_file_count: 0,
                    session_contract: None,
                    has_blocker: false,
                    has_delivered: false,
                    is_running: false,
                    is_active: false,
                    group_kpi: crate::ports::BoardKpiSummary::default(),
                },
            );
            panel.select_board_session("s-demo".into(), None, true, cx);
            assert_eq!(
                panel.board_inspector.session.as_deref(),
                Some("s-demo")
            );
            assert!(panel.has_board_drawer());
            assert!(panel.dismiss_overlay("board-inspector-drawer", cx));
            assert!(!panel.has_board_drawer());
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn format_skill_prompt_uses_real_slash_skill_protocol() {
        let chip = crate::ports::SkillChip {
            id: "flowy/xlsx-summary".into(),
            title: "Excel Summary".into(),
            summary: String::new(),
            official: true,
        };
        assert_eq!(
            format_skill_prompt(Some(&chip), "summarize Q3.xlsx"),
            "/skill:flowy/xlsx-summary summarize Q3.xlsx"
        );
        // Never double-prefix if the user already typed `/skill:...`.
        assert_eq!(
            format_skill_prompt(Some(&chip), "/skill:other/foo run"),
            "/skill:other/foo run"
        );
        assert_eq!(format_skill_prompt(Some(&chip), ""), "");
        assert_eq!(format_skill_prompt(None, "plain prompt"), "plain prompt");
    }

    #[gpui::test]
    fn pr2b_entry_three_paths_create_zero_empty_sessions_and_inherit_project_or_unbind(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "pr2b-entry-zero");
        let dir_a = root.join("proj-alpha");
        std::fs::create_dir_all(&dir_a).unwrap();
        panel.update(cx, |panel, cx| {
            // Home -> repeated `new_task` creates 0 sessions.
            panel.new_task(cx);
            panel.new_task(cx);
            assert_eq!(panel.agent.list_sessions().len(), 0);
            assert!(panel.active_project_id.is_none());

            // Create & open Project A -> `new_task` inherits Project A without creating an empty session.
            let proj_a = panel
                .create_project("Alpha", &dir_a, cx)
                .expect("create project A");
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(proj_a.id.as_str())
            );
            panel.new_task(cx);
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(proj_a.id.as_str()),
                "new_task from a Project view must inherit active_project_id"
            );
            assert_eq!(
                panel.agent.active_workspace().map(|w| w.project_id),
                Some(proj_a.id.clone())
            );
            assert_eq!(panel.agent.list_sessions().len(), 0);

            // Project capsule `×` (`bind_default_workspace`) unbinds ONLY the draft, keeping text!
            panel.fill_prompt("draft survives project unbind", cx);
            panel.bind_default_workspace(cx);
            assert!(panel.active_project_id.is_none());
            assert!(panel.agent.active_workspace().is_none());
            assert_eq!(
                visible_prompt(panel, cx),
                "draft survives project unbind"
            );
            assert_eq!(panel.agent.list_sessions().len(), 0);

            // Opening Chief Assistant from a project returns cleanly to Home with 0 empty sessions.
            panel.open_project(&proj_a.id, cx).unwrap();
            panel.open_chief_assistant(cx);
            assert!(panel.active_project_id.is_none());
            assert!(panel.agent.active_workspace().is_none());
            assert_eq!(panel.agent.list_sessions().len(), 0);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn pr2b_first_send_project_a_then_switch_to_project_b_preserves_b_draft_and_routes_to_a(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "pr2b-switch-ab");
        let dir_a = root.join("proj-a");
        let dir_b = root.join("proj-b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        let (proj_a_id, proj_b_id) = panel.update(cx, |panel, cx| {
            let a = panel.create_project("ProjA", &dir_a, cx).unwrap();
            let b = panel.create_project("ProjB", &dir_b, cx).unwrap();
            panel.open_project(&a.id, cx).unwrap();
            panel.fill_prompt("message strictly for A", cx);
            panel.send_prompt(cx);
            assert!(panel.submission_pending);

            // Mid-send switch to Project B and type a new draft for B.
            panel.open_project(&b.id, cx).unwrap();
            panel.fill_prompt("unsent draft for B", cx);
            (a.id, b.id)
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            // Project B stays selected and its draft is completely untouched.
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(proj_b_id.as_str())
            );
            assert_eq!(visible_prompt(panel, cx), "unsent draft for B");
            // Exactly 1 Router session exists, belonging strictly to Project A (0 for B).
            let sessions = panel.agent.list_sessions();
            assert_eq!(sessions.len(), 1);
            assert_eq!(
                sessions[0].project_id.as_deref(),
                Some(proj_a_id.as_str())
            );
            assert_eq!(
                panel.agent.session_role(&sessions[0].id),
                flowy_agent::SessionRole::Router
            );
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn pr2b_legacy_standard_session_and_router_stop_and_locate_source_coexist(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "pr2b-legacy-compat");
        let standard_id = panel.update(cx, |panel, cx| {
            let sid = panel.agent.create_home_session().unwrap();
            assert_eq!(
                panel.agent.session_role(&sid),
                flowy_agent::SessionRole::Standard
            );
            panel.open_session(&sid, cx);
            panel.fill_prompt("legacy standard prompt", cx);
            panel.send_prompt(cx);
            sid
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert_eq!(
                panel.agent.active_session_id().as_deref(),
                Some(standard_id.as_str())
            );
            assert_eq!(
                panel.agent.session_role(&standard_id),
                flowy_agent::SessionRole::Standard
            );
            assert!(panel.agent.has_messages());
            panel.stop_turn(cx);

            // Router session uses Router role and supports child card error
            // surfacing on parent Router (`resolve_pending_card`).
            let router_id = panel
                .agent
                .create_router_session()
                .expect("create router session");
            assert_eq!(
                panel.agent.session_role(&router_id),
                flowy_agent::SessionRole::Router
            );
            panel.open_session(&router_id, cx);
            panel.resolve_pending_card(
                "w-missing-child".into(),
                router_id.clone(),
                "card-missing".into(),
                flowy_agent::CardSelection::ApproveTool {
                    args_hash: "deadbeef".into(),
                },
                cx,
            );
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| {
            assert!(
                panel.notice.is_some(),
                "child Worker card resolution failure must surface notice on parent Router"
            );
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn pr2b_cached_popover_and_home_pill_unify_child_work_attention_counts(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = submission_panel(cx, "pr2b-cached-pill");
        panel.update(cx, |panel, cx| {
            // Seed cached popover snapshot with an awaiting_approval child work
            // in another Router conversation and verify Home + Chat pills and
            // `background_work_count` share one exact count and Attention tone.
            panel.popover_works = crate::router_works::PopoverWorksSnapshot {
                current_works: Vec::new(),
                other_works: vec![(
                    "Parent Router".into(),
                    flowy_agent::ChildWorkSummary {
                        work_id: "w-attn-1".into(),
                        root_session_id: "r-parent-1".into(),
                        title: "Dangerous shell step".into(),
                        state: "awaiting_approval".into(),
                        access_mode: flowy_agent::WorkAccessMode::ReadWrite,
                        origin: flowy_agent::SessionOrigin::Home,
                        workspace_root: None,
                        turn_status: flowy_agent::TurnStatus::WaitingForUser,
                        updated_at: 1_760_000_000_000,
                        contract_revision: None,
                        delivered_artifact_ids: Vec::new(),
                        missing_items: Vec::new(),
                        objects: Vec::new(),
                        last_stop_handoff: None,
                        heartbeat: None,
                        pending_cards: Vec::new(),
                    },
                )],
                legacy_running_sessions: Vec::new(),
                legacy_receipts: Vec::new(),
            };
            panel.surface = StudioSurface::Home;
            assert_eq!(panel.background_work_count(), 1);
            assert_eq!(panel.popover_total_count(), 1);
            let pill = panel
                .router_bg_pill_summary()
                .expect("Home pill must surface awaiting_approval child work");
            assert_eq!(pill.count, 1);
            assert_eq!(pill.tone, crate::router_works::BgPillTone::Attention);
            assert_eq!(
                pill.label,
                panel.tf("chat.bg_tasks_needs_approval", &[("count", "1")])
            );

            // Opening Flowchart Modal on a child Worker branch without a report
            // still resolves cleanly and never leaves a dangling state.
            let router_id =
                panel.agent.create_router_session().expect("create router");
            panel.stop_session_turn(&router_id, cx);
        });
        cx.run_until_parked();
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    fn open_cold_panel(
        cx: &mut TestAppContext,
        tag: &str,
    ) -> (Entity<StudioPanel>, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let panel = open_panel(cx, |window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: "flowy".into(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        panel.update(cx, |panel, _| {
            while panel.agent.is_initializing() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        (panel, root)
    }

    #[gpui::test]
    fn cold_start_project_a_draft_switch_to_b_creates_single_project_a_router_and_preserves_b_draft(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = open_cold_panel(cx, "cold-proj");
        let proj_a_dir = root.join("proj-a");
        let proj_b_dir = root.join("proj-b");
        std::fs::create_dir_all(&proj_a_dir).unwrap();
        std::fs::create_dir_all(&proj_b_dir).unwrap();
        let proj_a_id = Rc::new(RefCell::new(String::new()));
        let proj_b_id = Rc::new(RefCell::new(String::new()));
        panel.update(cx, |panel, cx| {
            panel
                .create_project("Alpha", &proj_a_dir, cx)
                .expect("create project A");
            *proj_a_id.borrow_mut() =
                panel.active_project_id.clone().expect("A active");
            panel
                .create_project("Beta", &proj_b_dir, cx)
                .expect("create project B");
            *proj_b_id.borrow_mut() =
                panel.active_project_id.clone().expect("B active");

            panel
                .open_project(&proj_a_id.borrow(), cx)
                .expect("open project A draft");
            panel.catalog_loading = true;
            panel.cloud_ready = false;
            panel.initialized = true;
            panel.fill_prompt("first cold task for project A", cx);
            panel.send_prompt(cx);
            assert_eq!(panel.notice, None);
            assert_eq!(visible_prompt(panel, cx), "");
            assert_eq!(panel.pending_receipts.len(), 1);
            let pending = panel.agent.pending_acceptances();
            assert_eq!(pending.len(), 1);
            let draft = pending[0]
                .draft_target
                .as_deref()
                .expect("cold-start receipt carries draft_target");
            assert_eq!(draft.role, flowy_agent::SessionRole::Router);
            assert_eq!(
                draft.origin,
                flowy_agent::SessionOrigin::Project {
                    project_id: Some(proj_a_id.borrow().clone()),
                }
            );

            // Switch to Project B draft before catalog finishes loading.
            panel
                .open_project(&proj_b_id.borrow(), cx)
                .expect("open project B draft");
            panel.fill_prompt("keep project B draft", cx);
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(proj_b_id.borrow().as_str())
            );

            // Model catalog finishes loading with local echo runner enabled:
            // drain the queued Project A receipt without hijacking Project B.
            panel.agent.apply_config(LlmConfig::default()).unwrap();
            panel.catalog_loading = false;
            panel.cloud_ready = true;
            panel.notify_cloud_ready(cx);
            panel.refresh_pending_receipts(cx);
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            let deadline = std::time::Instant::now()
                + std::time::Duration::from_secs(3);
            while panel.agent.has_active_claim()
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
                panel.refresh_pending_receipts(cx);
            }
            let sessions = panel.agent.list_sessions();
            assert_eq!(
                sessions.len(),
                1,
                "cold-start Project A drain must create strictly one Router session"
            );
            assert_eq!(
                panel.agent.session_role(&sessions[0].id),
                flowy_agent::SessionRole::Router
            );
            assert_eq!(
                sessions[0].project_id.as_deref(),
                Some(proj_a_id.borrow().as_str())
            );
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(proj_b_id.borrow().as_str()),
                "Project B selection must stay intact"
            );
            assert_eq!(
                panel.agent.active_session_id(),
                None,
                "background drain must not steal Project B draft focus"
            );
            assert_eq!(visible_prompt(panel, cx), "keep project B draft");
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn cold_start_existing_router_queues_instead_of_rejecting_library_loading(
        cx: &mut TestAppContext,
    ) {
        let (panel, root) = open_cold_panel(cx, "cold-router");
        let proj_a_dir = root.join("proj-a");
        std::fs::create_dir_all(&proj_a_dir).unwrap();
        let router_id = Rc::new(RefCell::new(String::new()));
        panel.update(cx, |panel, cx| {
            let rid = panel
                .agent
                .create_router_project_session(flowy_agent::AgentWorkspace {
                    project_id: "proj-a".into(),
                    root: proj_a_dir.clone(),
                })
                .expect("create router");
            *router_id.borrow_mut() = rid.clone();
            panel.open_session(&rid, cx);
            panel.catalog_loading = true;
            panel.cloud_ready = false;
            panel.initialized = true;

            panel.fill_prompt("existing router message while catalog loads", cx);
            panel.send_prompt(cx);
            assert_eq!(
                panel.notice, None,
                "existing Router must accept durably instead of rejecting library.loading"
            );
            assert_eq!(visible_prompt(panel, cx), "");
            let pending = panel.agent.pending_acceptances();
            assert_eq!(pending.len(), 1);
            assert!(pending[0].strict_target);
            assert_eq!(pending[0].target_session_id.as_deref(), Some(rid.as_str()));

            panel.agent.apply_config(LlmConfig::default()).unwrap();
            panel.catalog_loading = false;
            panel.cloud_ready = true;
            panel.notify_cloud_ready(cx);
            panel.refresh_pending_receipts(cx);
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(3);
            while panel.agent.has_active_claim()
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
                panel.refresh_pending_receipts(cx);
            }
            assert_eq!(panel.agent.list_sessions().len(), 1);
            let lines = panel.agent.session_lines(&router_id.borrow());
            assert!(lines.iter().any(|l| matches!(
                l,
                flowy_agent::ChatLine::User { text }
                    if text == "existing router message while catalog loads"
            )));
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn quote_banner_overwrites_clears_isolates_sessions_and_roundtrips_skill_prompt(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-quote-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            let mut panel = StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            );
            panel.catalog.replace(vec![flowy_agent::CatalogModel {
                id: "test-model".into(),
                name: "Test model".into(),
                ..Default::default()
            }]);
            panel.cloud_ready = true;
            panel
        });
        let sid_a = Rc::new(RefCell::new(String::new()));
        let sid_b = Rc::new(RefCell::new(String::new()));
        panel.update(cx, |panel, cx| {
            let a = panel
                .agent
                .create_router_session()
                .expect("create router session A");
            let b = panel
                .agent
                .create_router_session()
                .expect("create router session B");
            *sid_a.borrow_mut() = a.clone();
            *sid_b.borrow_mut() = b;
            panel.open_session(&a, cx);

            // Second quote replaces first quote cleanly (single-quote WeChat semantics).
            panel.set_active_quote(
                crate::composer::QuoteRole::Assistant,
                "第一条长引用：先查环境诊断",
                cx,
            );
            panel.set_active_quote(
                crate::composer::QuoteRole::User,
                &"第二条超长中文引用摘要：".repeat(16),
                cx,
            );
            let quote = panel.active_quote.as_ref().expect("latest quote kept");
            assert_eq!(quote.role, crate::composer::QuoteRole::User);
            assert!(quote.excerpt.chars().count() <= 160);
            assert!(quote.excerpt.ends_with('\u{2026}'));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("composer-quote-banner").is_some());

        // Clicking X clears the quote banner immediately.
        let clear_btn = cx
            .debug_bounds("composer-quote-clear")
            .expect("quote clear X button rendered");
        cx.simulate_click(clear_btn.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.read_entity(&panel, |panel, _| {
            assert!(panel.active_quote.is_none());
        });
        assert!(cx.debug_bounds("composer-quote-banner").is_none());

        // Switching sessions isolates quotes per conversation.
        panel.update(cx, |panel, cx| {
            panel.set_active_quote(
                crate::composer::QuoteRole::Assistant,
                "会话 A 的关键结论",
                cx,
            );
            panel.open_session(&sid_b.borrow(), cx);
            assert!(
                panel.active_quote.is_none(),
                "session B must not inherit session A quote"
            );
            panel.open_session(&sid_a.borrow(), cx);
            assert_eq!(
                panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
                Some("会话 A 的关键结论")
            );
        });

        // Cold-start deferred send with mounted `/skill:` preserves `/skill:` prefix & quote cleanly.
        panel.update(cx, |panel, cx| {
            panel.selected_skill = Some(crate::ports::SkillChip {
                id: "flowy/xlsx-summary".into(),
                title: "表格总结".into(),
                summary: String::new(),
                official: true,
            });
            panel.catalog_loading = true;
            panel.cloud_ready = false;
            panel.fill_prompt("整理本季度销售报表", cx);
            panel.send_prompt(cx);
            assert!(
                panel.active_quote.is_none(),
                "quote clears once deferred prompt is accepted"
            );
            let pending = panel.agent.pending_acceptances();
            assert_eq!(pending.len(), 1);
            assert!(
                pending[0]
                    .text
                    .starts_with("/skill:flowy/xlsx-summary > \u{275d} Flowy: 会话 A 的关键结论\n\n整理本季度销售报表"),
                "packed deferred text was: {}",
                pending[0].text
            );

            // Render a User bubble containing that packed text and confirm top quote preview appears.
            panel.feed.update(cx, |feed, cx| {
                feed.sync_history(
                    Some(sid_a.borrow().clone()),
                    flowy_agent::ChatHistory {
                        rows: vec![flowy_agent::ChatLine::User {
                            text: pending[0].text.clone(),
                        }],
                        streaming: None,
                        status: flowy_agent::TurnStatus::Completed,
                    },
                    cx,
                );
            });
            panel.surface = StudioSurface::Chat;
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            cx.debug_bounds("chat-user-quote-preview").is_some(),
            "quoted User bubble renders top excerpt strip"
        );

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn quote_session_sync_covers_home_project_and_artifact_transitions(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy_studio_quote_sync_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let proj_dir = root.join("proj");
        std::fs::create_dir_all(&proj_dir).expect("create project folder");
        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        let sid_a = Rc::new(RefCell::new(String::new()));
        let sid_b = Rc::new(RefCell::new(String::new()));
        panel.update(cx, |panel, cx| {
            let a = panel
                .agent
                .create_router_session()
                .expect("create session A");
            let b = panel
                .agent
                .create_router_session()
                .expect("create session B");
            *sid_a.borrow_mut() = a.clone();
            *sid_b.borrow_mut() = b.clone();
            panel.open_session(&a, cx);
            panel.set_active_quote(
                crate::composer::QuoteRole::Assistant,
                "会话 A 的待保留引用",
                cx,
            );

            // External session clear followed by show_new_home stashes A's quote cleanly.
            let _ = panel.agent.clear_active();
            panel.show_new_home(cx);
            assert!(
                panel.active_quote.is_none(),
                "home view clears active quote banner"
            );
            panel.open_session(&a, cx);
            assert_eq!(
                panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
                Some("会话 A 的待保留引用"),
                "reopening A restores stashed quote after show_new_home"
            );

            // Host selecting B then calling show_selected_artifact syncs active_quote via refresh_chat.
            panel.agent.select_session(&b).expect("host select B");
            let _ = panel.show_selected_artifact("missing-artifact", cx);
            assert!(
                panel.active_quote.is_none(),
                "show_selected_artifact drops A's quote when agent active session is B"
            );
            panel.open_session(&a, cx);
            assert_eq!(
                panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
                Some("会话 A 的待保留引用"),
                "returning to A after show_selected_artifact still preserves A's quote"
            );

            // Rolled-back open_artifact_source keeps A's quote intact.
            assert!(panel
                .open_artifact_source(&b, "missing-artifact", cx)
                .is_err());
            assert_eq!(
                panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
                Some("会话 A 的待保留引用"),
                "failed open_artifact_source rollback restores A's quote"
            );

            // Opening a project stashes A's quote cleanly without double-stashing.
            panel
                .create_project("Quote Proj", &proj_dir, cx)
                .expect("create and open project");
            assert!(panel.active_quote.is_none());
            panel.open_session(&a, cx);
            assert_eq!(
                panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
                Some("会话 A 的待保留引用"),
                "open_project must preserve session A draft quote"
            );

            // clear_thread discards the active session's quote permanently.
            panel.clear_thread(cx);
            assert!(panel.active_quote.is_none());
            panel.open_session(&a, cx);
            assert!(
                panel.active_quote.is_none(),
                "clear_thread permanently removes session A quote"
            );
        });

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn drawer_blocks_overturned_history_clicks_preserves_turn_snapshot_and_evicts_failed_preview(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy_studio_drawer_settlement_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let work_dir = root.join("work");
        std::fs::create_dir_all(&work_dir).expect("create work dir");
        let a_path = work_dir.join("alpha.md");
        let b_path = work_dir.join("beta.md");
        std::fs::write(&a_path, "alpha preview").expect("write alpha");
        std::fs::write(&b_path, "beta preview").expect("write beta");
        let a_str = a_path.to_string_lossy().into_owned();
        let a_variant =
            format!(r"\\?\{}", a_str.to_ascii_uppercase().replace('\\', "/"));
        let b_str = b_path.to_string_lossy().into_owned();

        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });

        let entry_a = FileEntry {
            path: a_str.clone(),
            change: flowy_agent::FileChangeKind::Add,
            added: 1,
            removed: 0,
            binary: false,
        };
        let entry_b = FileEntry {
            path: b_str.clone(),
            change: flowy_agent::FileChangeKind::Add,
            added: 1,
            removed: 0,
            binary: false,
        };
        let verify_line = |id: &str, path: &str, ok: bool| {
            flowy_agent::ChatLine::Verification {
                report_id: id.into(),
                artifact_path: path.into(),
                artifact_hash: format!("hash-{id}"),
                ok,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            }
        };

        // 1. User pins Turn 1 snapshot [A]; Turn 2 delivers independent [B];
        //    sync must preserve [A] instead of kicking user to [B].
        panel.update(cx, |panel, cx| {
            let sid =
                panel.agent.create_router_session().expect("create session");
            panel.open_session(&sid, cx);
            panel.feed.update(cx, |feed, cx| {
                feed.sync_history(
                    Some(sid.clone()),
                    flowy_agent::ChatHistory {
                        rows: vec![
                            flowy_agent::ChatLine::User {
                                text: "turn 1".into(),
                            },
                            flowy_agent::ChatLine::FileChanges {
                                files: vec![entry_a.clone()],
                            },
                            verify_line("r1", &a_str, true),
                        ],
                        streaming: None,
                        status: flowy_agent::TurnStatus::Completed,
                    },
                    cx,
                );
                cx.emit(OpenDeliverables(vec![entry_a.clone()]));
            });
            cx.notify();
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert!(panel.right_drawer_open);
            assert_eq!(
                panel
                    .deliverables
                    .as_ref()
                    .map(crate::deliverables::DeliverableScope::files),
                Some(&[entry_a.clone()][..])
            );

            // Turn 2 adds B while Turn 1 snapshot is pinned open.
            let sid = panel.agent.active_session_id().unwrap();
            panel.feed.update(cx, |feed, cx| {
                feed.sync_history(
                    Some(sid),
                    flowy_agent::ChatHistory {
                        rows: vec![
                            flowy_agent::ChatLine::User {
                                text: "turn 1".into(),
                            },
                            flowy_agent::ChatLine::FileChanges {
                                files: vec![entry_a.clone()],
                            },
                            verify_line("r1", &a_str, true),
                            flowy_agent::ChatLine::User {
                                text: "turn 2".into(),
                            },
                            flowy_agent::ChatLine::FileChanges {
                                files: vec![entry_b.clone()],
                            },
                            verify_line("r2", &b_str, true),
                        ],
                        streaming: None,
                        status: flowy_agent::TurnStatus::Completed,
                    },
                    cx,
                );
            });
            panel.sync_right_drawer_deliverables(cx);
            assert_eq!(
                panel.deliverables,
                Some(crate::deliverables::DeliverableScope::TurnSnapshot(
                    vec![entry_a.clone(),]
                )),
                "live sync must not overwrite pinned historical turn snapshot"
            );
            assert_eq!(panel.deliverable_count(cx), 1);

            // Start previewing A, then Turn 3 fails verification on A via variant path.
            panel.set_preview_file(Some(a_str.clone()), cx);
            assert_eq!(
                panel.drawer_preview.file.as_deref(),
                Some(a_str.as_str())
            );
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert!(matches!(
                panel.drawer_preview.body,
                Some(DrawerPreviewBody::Markdown(_))
            ));

            let sid = panel.agent.active_session_id().unwrap();
            panel.feed.update(cx, |feed, cx| {
                feed.sync_history(
                    Some(sid),
                    flowy_agent::ChatHistory {
                        rows: vec![
                            flowy_agent::ChatLine::User {
                                text: "turn 1".into(),
                            },
                            flowy_agent::ChatLine::FileChanges {
                                files: vec![entry_a.clone()],
                            },
                            verify_line("r1", &a_str, true),
                            flowy_agent::ChatLine::User {
                                text: "turn 2".into(),
                            },
                            flowy_agent::ChatLine::FileChanges {
                                files: vec![entry_b.clone()],
                            },
                            verify_line("r2", &b_str, true),
                            flowy_agent::ChatLine::User {
                                text: "turn 3".into(),
                            },
                            verify_line("r3", &a_variant, false),
                        ],
                        streaming: None,
                        status: flowy_agent::TurnStatus::Completed,
                    },
                    cx,
                );
            });
            panel.sync_right_drawer_deliverables(cx);
            assert!(
                panel.drawer_preview.file.is_none()
                    && panel.drawer_preview.body.is_none(),
                "active preview must be evicted when later turn fails verification"
            );
            assert!(panel.deliverables.is_none());
            assert_eq!(panel.notice, Some("verify.card.failed"));

            // Attempting to re-open historical FileChanges([A]) or preview A is blocked.
            panel.close_right_sheet(cx);
            panel.emit_notice(None, cx);
            panel.open_deliverables(vec![entry_a.clone()], cx);
            assert!(!panel.right_drawer_open);
            assert_eq!(panel.notice, Some("verify.card.failed"));

            panel.emit_notice(None, cx);
            panel.set_preview_file(Some(a_str.clone()), cx);
            assert!(panel.drawer_preview.file.is_none());
            assert_eq!(panel.notice, Some("verify.card.failed"));
        });

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn drawer_workbench_async_preview_fix_in_chat_and_scope_switching(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir()
            .join(format!("flowy-studio-workbench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let work_dir = root.join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        let file_a = work_dir.join("alpha.md");
        let file_b = work_dir.join("beta.csv");
        let file_bad = work_dir.join("broken.csv");
        std::fs::write(&file_a, "# Alpha Report\nDone.").unwrap();
        std::fs::write(&file_b, "col,val\nok,1\n").unwrap();
        std::fs::write(&file_bad, "col,val\nbad,0\n").unwrap();
        let a_str = file_a.to_string_lossy().into_owned();
        let b_str = file_b.to_string_lossy().into_owned();
        let bad_str = file_bad.to_string_lossy().into_owned();

        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig::default(),
                },
                ui,
                window,
                cx,
            )
        });

        panel.update(cx, |panel, cx| {
            let sid = panel.agent.create_session().unwrap();
            panel.agent.select_session(&sid).unwrap();
            let entry_a = flowy_agent::FileEntry {
                path: a_str.clone(),
                change: flowy_agent::FileChangeKind::Add,
                added: 2,
                removed: 0,
                binary: false,
            };
            let entry_b = flowy_agent::FileEntry {
                path: b_str.clone(),
                change: flowy_agent::FileChangeKind::Add,
                added: 2,
                removed: 0,
                binary: false,
            };
            panel.feed.update(cx, |feed, cx| {
                feed.sync_history(
                    Some(sid),
                    flowy_agent::ChatHistory {
                        rows: vec![
                            flowy_agent::ChatLine::User {
                                text: "turn 1".into(),
                            },
                            flowy_agent::ChatLine::FileChanges {
                                files: vec![entry_a],
                            },
                            flowy_agent::ChatLine::Verification {
                                report_id: "r1".into(),
                                artifact_path: a_str.clone(),
                                artifact_hash: "11111111".into(),
                                ok: true,
                                issues: Vec::new(),
                                semantic_hint: None,
                                override_used: false,
                                provenance_warnings: Vec::new(),
                                source_session_id: None,
                            },
                            flowy_agent::ChatLine::User {
                                text: "turn 2".into(),
                            },
                            flowy_agent::ChatLine::FileChanges {
                                files: vec![entry_b],
                            },
                            flowy_agent::ChatLine::Verification {
                                report_id: "r-bad".into(),
                                artifact_path: bad_str.clone(),
                                artifact_hash: "22222222".into(),
                                ok: false,
                                issues: vec![flowy_agent::VerificationIssue {
                                    code: "formula_error".into(),
                                    field: "Sheet1!B2".into(),
                                }],
                                semantic_hint: None,
                                override_used: false,
                                provenance_warnings: Vec::new(),
                                source_session_id: None,
                            },
                        ],
                        streaming: None,
                        status: flowy_agent::TurnStatus::Completed,
                    },
                    cx,
                );
            });

            // Toggle drawer captures both failed banner & latest workspace items.
            panel.toggle_deliverables(cx);
            assert!(panel.right_drawer_open);
            assert!(panel.feed.read(cx).has_trace_attention());
            assert_eq!(panel.feed.read(cx).failed_verifications().len(), 1);
            assert_eq!(
                panel.visible_workspace_deliverables(cx).len(),
                1,
                "Latest scope shows only turn 2 settled beta.csv"
            );

            // Switch to All scope -> surfaces alpha.md + beta.csv.
            panel.set_deliverables_view_scope(
                crate::deliverables::DeliverablesViewScope::All,
                cx,
            );
            assert_eq!(panel.visible_workspace_deliverables(cx).len(), 2);

            // Rapid async preview switch A -> B cancels stale A load.
            panel.set_preview_file(Some(a_str.clone()), cx);
            panel.set_preview_file(Some(b_str.clone()), cx);
            assert_eq!(
                panel.drawer_preview.file.as_deref(),
                Some(b_str.as_str())
            );
            assert!(matches!(
                panel.drawer_preview.body,
                Some(DrawerPreviewBody::Loading)
            ));
        });
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert_eq!(
                panel.drawer_preview.file.as_deref(),
                Some(b_str.as_str())
            );
            assert!(matches!(
                panel.drawer_preview.body,
                Some(DrawerPreviewBody::Table { .. })
            ));
            assert!(
                !panel.deliverable_sizes.is_empty(),
                "background metadata probe populates cached sizes"
            );

            // Fix in chat fills empty prompt, then preserves non-empty draft as quote.
            panel.request_fix_in_chat(
                "Sheet1!B2".into(),
                Some(bad_str.clone()),
                None,
                cx,
            );
            assert!(
                panel.prompt_text(cx).contains("broken.csv"),
                "empty prompt filled with fix request"
            );
            assert!(panel.active_quote.is_none());

            panel.set_prompt_text("keep my draft", cx);
            panel.request_fix_in_chat(
                "Sheet1!B2".into(),
                Some(bad_str),
                None,
                cx,
            );
            assert_eq!(panel.prompt_text(cx), "keep my draft");
            assert!(
                panel
                    .active_quote
                    .as_ref()
                    .is_some_and(|q| q.excerpt.contains("broken.csv")),
                "existing draft must stay intact while fix context moves to quote"
            );
        });

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn open_chief_assistant_reset_preserves_collapsed_dock_visible(
        cx: &mut gpui::TestAppContext,
    ) {
        let root = std::env::temp_dir()
            .join(format!("flowy-panel-dock-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test root");

        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig::default(),
                },
                ui,
                window,
                cx,
            )
        });

        panel.update(cx, |panel, cx| {
            panel.set_dock_visible(false, cx);
            assert!(!panel.dock_visible);
            panel.open_chief_assistant(cx);
            assert!(
                !panel.dock_visible,
                "reset_session from open_chief_assistant must not overwrite collapsed dock_visible"
            );
        });

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn set_artifacts_with_open_drawer_and_empty_workspace_files_notifies_panel(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let root = std::env::temp_dir().join(format!(
            "flowy-panel-artifacts-notify-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test root");

        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig::default(),
                },
                ui,
                window,
                cx,
            )
        });

        let notifies = Arc::new(AtomicUsize::new(0));
        let notifies_ref = Arc::clone(&notifies);
        let _sub = cx.update(|_, cx| {
            cx.observe(&panel, move |_, _| {
                notifies_ref.fetch_add(1, Ordering::SeqCst);
            })
        });

        panel.update(cx, |panel, cx| {
            let sid = panel.agent.create_session().expect("create session");
            panel.agent.select_session(&sid).expect("select session");
            panel.feed.update(cx, |feed, cx| {
                feed.sync_history(
                    Some(sid),
                    flowy_agent::ChatHistory {
                        rows: vec![flowy_agent::ChatLine::Artifact {
                            artifact_id: "art-empty-ws".into(),
                        }],
                        streaming: None,
                        status: flowy_agent::TurnStatus::Completed,
                    },
                    cx,
                );
            });
            panel.toggle_deliverables(cx);
            assert!(panel.right_drawer_open);
            assert_eq!(panel.visible_workspace_deliverables(cx).len(), 0);
        });
        cx.run_until_parked();
        notifies.store(0, Ordering::SeqCst);

        let item = crate::library::LibraryItem {
            id: "art-empty-ws".into(),
            title: "Summary Deck".into(),
            original_name: "summary.pptx".into(),
            description: String::new(),
            category: crate::library::Category::Slides,
            date: String::new(),
            time: String::new(),
            pending: false,
            preview: None,
            size_bytes: Some(2048),
            extension: "pptx".into(),
            media_type: None,
            delivered_at_ms: 0,
            delivered_date: "2026-11-27".into(),
            delivered_time: "10:00".into(),
            source_session_id: String::new(),
            source_title: String::new(),
            source_deleted: false,
            source_unavailable: false,
            favorite: false,
            availability: crate::library::ArtifactAvailability::Available,
            override_used: false,
        };
        panel.update(cx, |panel, cx| {
            panel.set_artifacts(vec![item], cx);
        });
        cx.run_until_parked();
        assert!(
            notifies.load(Ordering::SeqCst) >= 1,
            "set_artifacts with open right drawer must notify StudioPanel even when workspace deliverable paths are empty"
        );
        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.feed.read(cx).session_artifacts().len(), 1);
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }
}

use crate::composer::{ChatComposer, ComposerEvent};
use crate::deliverables::PreviewBody;
use crate::feed::{
    ArtifactAction, ChatFeed, OpenDeliverables, OpenTask, RevealFile,
    WorkingIndicator, WorkingIndicatorEvent,
};
use flowy_agent::{
    AgentHandle, AgentWorkspace, CardAction, FileEntry, LlmConfig, Model, Plan,
    ProviderCatalog, ReceiptState, SessionOrigin, TaskTarget,
};
use flowy_core::DocumentSession;
use flowy_i18n::I18n;
use flowy_ui::{ResolvedTheme, UiContext};
use gpui::{
    Context, Entity, EventEmitter, IntoElement, Task, Window, prelude::*,
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
    SaveArtifact(String),
    LocateArtifactSource {
        session_id: String,
        artifact_id: String,
    },
    Rated {
        stars: u32,
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

/// Self-contained UI state for the centered Flowchart Modal.
#[derive(Clone, Debug, Default)]
pub struct FlowModalState {
    pub session: Option<String>,
    pub branch: Option<String>,
    pub tab: crate::overview::FlowModalTab,
    /// Bound to `"{report_id}:{artifact_path}"` so background snapshot refreshes
    /// that switch branches never carry an override acknowledgement onto another
    /// artifact (§8.3).
    pub override_ack_key: Option<String>,
}

impl FlowModalState {
    pub fn close(&mut self) {
        self.session = None;
        self.branch = None;
        self.override_ack_key = None;
    }

    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.session.is_some()
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
    pub(crate) local_env: crate::ports::LocalEnvSnapshot,
    pub(crate) board_filter: crate::overview::BoardFilter,
    pub(crate) board_project_filter: Option<String>,
    pub(crate) flow_modal: FlowModalState,
    pub(crate) trace_filter: crate::overview::TraceFilter,
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
    /// Files the deliverables drawer lists; `None` keeps it closed.
    pub(crate) deliverables: Option<Vec<FileEntry>>,
    /// Single background work or queued receipt inspected in the right sheet.
    pub(crate) inspected_work: Option<crate::deliverables::InspectedWork>,
    /// Path being previewed inside the drawer.
    pub(crate) preview_file: Option<String>,
    /// Loaded once when the file is picked, so rendering never touches disk.
    pub(crate) preview_body: Option<PreviewBody>,
    /// Composer drafts keyed by session, so switching tasks does not leak text.
    drafts: std::collections::HashMap<String, String>,
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
                        report_id,
                        artifact_path,
                    } => {
                        panel.confirm_override_delivery(
                            report_id,
                            artifact_path,
                            cx,
                        );
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
            local_env: crate::ports::LocalEnvSnapshot::default(),
            board_filter: crate::overview::BoardFilter::All,
            board_project_filter: None,
            flow_modal: FlowModalState::default(),
            trace_filter: crate::overview::TraceFilter::All,
            cached_trace: None,
            drawer_tab: crate::deliverables::RightDrawerTab::Deliverables,
            right_drawer_open: false,
            selected_skill: None,
            installed_skills: Vec::new(),
            board_generation: 0,
            last_board_refresh: None,
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
            preview_file: None,
            preview_body: None,
            drafts: std::collections::HashMap::new(),
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
        } else if self.surface == StudioSurface::Board
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
            && self
                .last_trace_refresh
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(300))
        {
            self.refresh_trace_cache();
            self.last_trace_refresh = Some(std::time::Instant::now());
            cx.notify();
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
        let session = self.agent.active_session_id();
        let history = self.agent.chat_history();
        let status = history.status.clone();
        self.feed
            .update(cx, |feed, cx| feed.sync_history(session, history, cx));
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
        if self.surface != surface {
            self.invalidate_submission(cx);
            self.surface = surface;
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
        self.preview_file = None;
        self.preview_body = None;
        self.refresh_trace_cache();
    }

    pub fn reset_session(&mut self, cx: &mut Context<'_, Self>) {
        self.invalidate_submission(cx);
        self.close_all_popovers();
        self.flow_modal.close();
        self.clear_right_sheet();
        self.active_project_id = None;
        self.document = DocumentSession::new(self.product);
        let _ = self.agent.clear_active();
        self.agent.set_workspace_for_new_sessions(None);
        self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
        self.surface = StudioSurface::Home;
        self.refresh_chat(cx);
        self.refresh_board_snapshot();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    pub fn new_task(&mut self, cx: &mut Context<'_, Self>) {
        match self.agent.clear_active() {
            Ok(()) => {
                self.invalidate_submission(cx);
                self.close_all_popovers();
                self.flow_modal.close();
                self.clear_right_sheet();
                self.active_project_id = None;
                self.agent.set_workspace_for_new_sessions(None);
                self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
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
        self.flow_modal.close();
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

    /// Total active/queued works shown in the background-tasks pill.
    #[must_use]
    pub fn background_work_count(&self) -> usize {
        let running_ids = self.background_running_session_ids();
        let mut running = running_ids.len();
        if self.surface != StudioSurface::Chat
            && running == 0
            && (self.status.is_busy() || self.submission_pending)
        {
            running = 1;
        }
        running + self.popover_visible_receipts(&running_ids).len()
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
        if self.agent.active_session_id().is_some()
            && self.active_project_id.is_none()
            && self.agent.has_messages()
        {
            self.surface = StudioSurface::Chat;
            self.refresh_chat(cx);
            cx.notify();
            return;
        }
        let latest_default = self
            .agent
            .list_sessions()
            .into_iter()
            .find(|session| session.project_id.is_none());
        if let Some(session) = latest_default {
            self.open_session(&session.id, cx);
        } else {
            self.new_task(cx);
        }
    }

    /// Creates an empty session in the workspace `open_project` just prepared.
    pub fn create_task(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        self.agent.set_origin_for_new_sessions(
            match self.active_project_id.as_ref() {
                Some(project_id) => SessionOrigin::Project {
                    project_id: Some(project_id.clone()),
                },
                None => SessionOrigin::Home,
            },
        );
        self.agent.create_session()?;
        self.invalidate_submission(cx);
        self.clear_right_sheet();
        self.surface = StudioSurface::Home;
        self.refresh_chat(cx);
        self.set_prompt_text("", cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
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
        if !same && let Some(current) = self.agent.active_session_id() {
            let draft = self.prompt_text(cx);
            self.drafts.insert(current, draft);
        }
        match self.agent.select_session(id) {
            Ok(()) => {
                if !same {
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
    }

    pub(crate) fn confirm_override_delivery(
        &mut self,
        report_id: &str,
        artifact_path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(session_id) = self.agent.active_session_id() else {
            self.emit_notice(Some("chat.publish.context"), cx);
            return;
        };
        self.confirm_override_for_session(
            &session_id,
            report_id,
            artifact_path,
            cx,
        );
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
                        panel.flow_modal.override_ack_key = None;
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
        if self.agent.active_session_id().is_none() {
            self.invalidate_submission(cx);
            self.clear_right_sheet();
            self.surface = StudioSurface::Home;
            self.document = DocumentSession::new(self.product);
            self.active_project_id = None;
            self.agent.set_workspace_for_new_sessions(None);
            self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
            self.set_prompt_text("", cx);
        } else if let Some(crate::deliverables::InspectedWork::Session {
            id,
            ..
        }) = &self.inspected_work
        {
            let still_exists = self
                .agent
                .list_sessions()
                .iter()
                .any(|session| &session.id == id);
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
        if !self
            .agent
            .list_sessions()
            .iter()
            .any(|session| session.id == session_id)
        {
            return Err("library.source.unavailable");
        }
        let previous = self.agent.active_session_id();
        self.agent.select_session(session_id)?;
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
        self.surface = StudioSurface::Chat;
        self.flow_modal.close();
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
        self.surface = StudioSurface::Chat;
        self.flow_modal.close();
        self.clear_right_sheet();
        self.refresh_trace_cache();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
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
        let text = format_skill_prompt(self.selected_skill.as_ref(), trimmed);
        tracing::debug!(
            target: "flowy.prompt",
            stage = "StudioPanel::send_prompt",
            cloud_ready = self.cloud_ready,
            model = ?model,
            chars = text.chars().count(),
            "enter"
        );
        if text.is_empty() {
            tracing::debug!(
                target: "flowy.prompt",
                stage = "StudioPanel::send_prompt",
                reason = "empty",
                "reject"
            );
            self.emit_notice(Some("notice.prompt_empty"), cx);
            return;
        }
        let has_pending_card =
            matches!(self.status, flowy_agent::TurnStatus::WaitingForUser)
                || self.agent.active_session_id().is_some_and(|sid| {
                    !self.agent.session_pending_cards(&sid).is_empty()
                });
        if has_pending_card {
            self.emit_notice(Some("notice.card_pending"), cx);
            return;
        }
        let running = self.status.is_busy();
        let waiting_for_cloud = self.catalog_loading
            || ((!self.cloud_ready || model.is_none())
                && self.agent.requires_cloud_ready());
        if !running
            && !waiting_for_cloud
            && (!self.cloud_ready || model.is_none())
        {
            tracing::debug!(
                target: "flowy.prompt",
                stage = "StudioPanel::send_prompt",
                reason = "cloud_or_model",
                "reject"
            );
            self.emit_notice(Some("notice.agent_failed"), cx);
            return;
        }
        // A turn that is already running (or waiting for cloud catalog readiness)
        // cannot start immediately. The host takes responsibility for the draft
        // right away and hands back a durable receipt, so nothing is lost.
        if running || waiting_for_cloud {
            match self.agent.accept_deferred(&text) {
                Ok(receipt) => {
                    tracing::debug!(
                        target: "flowy.prompt",
                        stage = "StudioPanel::send_prompt",
                        receipt = %receipt.id,
                        "accepted-deferred"
                    );
                    // gpui-pre dropped `emplace`; the queueing entry point
                    // is `set_prompt_text`, drained at the top of render.
                    self.set_prompt_text("", cx);
                    // The receipt is the feedback now. Saying "added to this
                    // session's task list" while no such list existed was a
                    // lie; `pending_receipts` is rendered by `workspace()`.
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
        let id = *self.submission.get_or_insert_with(new_submission_id);
        self.submission_generation = self.submission_generation.wrapping_add(1);
        let generation = self.submission_generation;
        self.submission_pending = true;
        let stop = Arc::new(AtomicBool::new(false));
        self.submission_stop = Some(stop.clone());
        self.refresh_working(cx);
        cx.notify();
        let agent = self.agent.clone();
        let expected_generation = agent.context_generation();
        let expected_active = agent.active_session_id();
        let workspace = agent.active_workspace();
        let submitted_text = text.clone();
        let expected = expected_active.clone();
        let worker_stop = stop.clone();
        let worker_agent = agent.clone();
        let job = cx.background_executor().spawn(async move {
            let result = if workspace.is_some_and(|workspace| {
                !crate::projects::root_available(&workspace.root)
            }) {
                Err("project.folder_unavailable")
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
                let _ = worker_agent
                    .cancel_task(TaskTarget::Session(session_id.clone()));
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
                        let _ =
                            agent.cancel_task(TaskTarget::Session(session_id));
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
                        panel.surface = StudioSurface::Chat;
                        panel.refresh_chat(cx);
                        panel.emit_notice(None, cx);
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
            self.cancel_task_target(TaskTarget::Session(session_id), cx);
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
        self.clear_right_sheet();
        self.right_drawer_open = true;
        self.deliverables = Some(files);
        self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        self.close_all_popovers();
        cx.notify();
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
        let files = self.feed.read(cx).latest_deliverables();
        self.clear_right_sheet();
        self.right_drawer_open = true;
        if let Some(files) = files {
            self.deliverables = Some(files);
            self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        } else {
            self.drawer_tab = crate::deliverables::RightDrawerTab::Steps;
        }
        self.close_all_popovers();
        cx.notify();
    }

    /// Closes whichever right-hand sheet is open (deliverables or single-work
    /// step inspector).
    pub(crate) fn close_right_sheet(&mut self, cx: &mut Context<'_, Self>) {
        self.clear_right_sheet();
        cx.notify();
    }

    /// Number of deliverables available in the current session's latest turn.
    pub(crate) fn deliverable_count(&self, cx: &Context<'_, Self>) -> usize {
        self.feed
            .read(cx)
            .latest_deliverables()
            .map_or(0, |files| files.len())
    }

    /// Picks a file to preview; `None` returns the drawer to the list.
    pub(crate) fn set_preview_file(
        &mut self,
        path: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.preview_body =
            path.as_deref().map(crate::deliverables::load_preview);
        self.preview_file = path;
        cx.notify();
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
            self.active_project_id = None;
            let _ = self.agent.clear_active();
            self.agent.set_workspace_for_new_sessions(None);
            self.surface = StudioSurface::Home;
            self.flow_modal.close();
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
        let Some(project) =
            self.projects.iter().find(|project| project.id == id)
        else {
            return Err("project.folder_unavailable");
        };
        if !crate::projects::root_available(&project.root) {
            self.emit_notice(Some("project.folder_unavailable"), cx);
            return Err("project.folder_unavailable");
        }
        let workspace = AgentWorkspace {
            project_id: project.id.clone(),
            root: project.root.clone(),
        };
        let project_id = project.id.clone();
        let clear_error = self.agent.clear_active().err();
        self.invalidate_submission(cx);
        self.flow_modal.close();
        self.clear_right_sheet();
        self.agent.set_workspace_for_new_sessions(Some(workspace));
        self.agent
            .set_origin_for_new_sessions(SessionOrigin::Project {
                project_id: Some(project_id.clone()),
            });
        self.active_project_id = Some(project_id);
        self.surface = StudioSurface::Home;
        self.set_prompt_text("", cx);
        self.refresh_chat(cx);
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
        // The loader is a real worker thread, so yielding the test thread lets
        // it finish without advancing the GPUI timer pump or fabricating events.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
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
                "background session initialization finished"
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

            panel.confirm_override_delivery(
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

            // Opening a missing session must not leave a ghost modal focus trap.
            panel.open_flow_modal("s-missing".into(), None, cx);
            assert!(!panel.has_flow_modal());

            // A resolvable session opens cleanly and dismisses via overlay scope.
            panel.board_snapshot.sessions.push(
                crate::ports::SessionBoardGroup {
                    session_id: "s-demo".into(),
                    title: "Demo".into(),
                    project_id: None,
                    project_name: None,
                    workspace_display: String::new(),
                    updated_at: 0,
                    branches: vec![crate::ports::WorkBranchSummary {
                        id: "b-demo".into(),
                        skill_label: crate::ports::SkillLabel::Key(
                            "board.skill_work",
                        ),
                        phase: crate::ports::BranchPhase::Idle,
                        deliverable_name: "Demo".into(),
                        deliverable_path: None,
                        format: crate::ports::DeliverableFormat::Other,
                        progress_pct: 100.0,
                        detail: String::new(),
                        work_progress: None,
                    }],
                    total_branch_count: 1,
                    session_contract: None,
                    has_blocker: false,
                    has_delivered: false,
                    is_running: false,
                    is_active: false,
                    group_kpi: crate::ports::BoardKpiSummary::default(),
                },
            );
            panel.open_flow_modal("s-demo".into(), None, cx);
            assert!(panel.has_flow_modal());
            assert!(panel.dismiss_overlay("flow-modal-panel", cx));
            assert!(!panel.has_flow_modal());
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
}

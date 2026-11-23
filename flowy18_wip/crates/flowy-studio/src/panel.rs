use crate::composer::{ChatComposer, ComposerEvent};
use crate::deliverables::PreviewBody;
use crate::feed::{
    ArtifactAction, ChatFeed, OpenDeliverables, OpenTask, RevealFile,
    WorkingIndicator,
};
use crate::starter::catalog::Category;
use flowy_agent::{
    AgentHandle, AgentWorkspace, FileEntry, LlmConfig, Model, Plan,
    ProviderCatalog, SessionOrigin,
};
use flowy_core::DocumentSession;
use flowy_i18n::I18n;
use flowy_ui::{ResolvedTheme, UiContext};
use gpui::{
    Context, Entity, EventEmitter, IntoElement, Task, Window, prelude::*,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
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

/// Which workspace surface the panel should show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudioSurface {
    Home,
    Chat,
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

/// A failed turn keeps its localized error line visible until the next turn.
const fn working_line_visible(status: &flowy_agent::TurnStatus) -> bool {
    status.is_busy() || matches!(status, flowy_agent::TurnStatus::Error { .. })
}

pub struct StudioPanel {
    pub ui: UiContext,
    pub theme: ResolvedTheme,
    pub prompt: Entity<EditableTextState>,
    pub(crate) feed: Entity<ChatFeed>,
    pub(crate) composer: Entity<ChatComposer>,
    _composer_sub: gpui::Subscription,
    _feed_sub: gpui::Subscription,
    _reveal_sub: gpui::Subscription,
    _deliverables_sub: gpui::Subscription,
    _artifact_sub: gpui::Subscription,
    submission: Option<u128>,
    submission_pending: bool,
    submission_generation: u64,
    submission_stop: Option<Arc<AtomicBool>>,
    pub(crate) working: Entity<WorkingIndicator>,
    pub(crate) status: flowy_agent::TurnStatus,
    last_elapsed: Option<u64>,
    _theme_sub: gpui::Subscription,
    _overlay_sub: gpui::Subscription,
    pub projects: Vec<crate::projects::Project>,
    pub active_project_id: Option<String>,
    project_store_failed: bool,
    pub document: DocumentSession,
    pub agent: AgentHandle,
    pub credits: crate::CreditsViewState,
    pub credits_hover: bool,
    pub credits_close: Option<Task<()>>,
    pub model_info_open: bool,
    pub model_menu_open: bool,
    pub tiers: Vec<TierView>,
    pub(crate) model_menu_anchor: &'static str,
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
    /// Starter copy held back until the user confirms replacing the draft.
    /// Category whose examples the home stage shows.
    pub(crate) starter_category: Category,
    /// Horizontal offset of the example row in logical pixels, 0 or negative.
    /// Owned here because `ScrollHandle::max_offset` stays 0 until the node has
    /// been laid out, which made arrow presses clamp straight back to zero.
    pub(crate) starter_offset: f32,
    /// Files the deliverables drawer lists; `None` keeps it closed.
    pub(crate) deliverables: Option<Vec<FileEntry>>,
    /// Path being previewed inside the drawer.
    pub(crate) preview_file: Option<String>,
    /// Loaded once when the file is picked, so rendering never touches disk.
    pub(crate) preview_body: Option<PreviewBody>,
    /// Composer drafts keyed by session, so switching tasks does not leak text.
    drafts: std::collections::HashMap<String, String>,
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
        cx: &mut Context<'_, Self>,
    ) -> Self {
        Self::new_with_mode(config, ui, false, cx)
    }

    pub fn new_background(
        config: StudioConfig,
        ui: UiContext,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        Self::new_with_mode(config, ui, true, cx)
    }

    fn new_with_mode(
        config: StudioConfig,
        ui: UiContext,
        background: bool,
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
        let prompt =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let prompt_sub =
            cx.subscribe(&prompt, |panel, _, _: &TextChanged, cx| {
                if !panel.submission_pending {
                    panel.submission = None;
                }
                if panel.surface != StudioSurface::Chat {
                    cx.notify();
                }
            });
        tracing::debug!(
            target: "flowy.boot",
            stage = "StudioPanel::new",
            "→ AgentHandle::with_config"
        );
        let agent = if background {
            AgentHandle::with_config_background(store_root.clone(), llm)
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
                match scope.as_ref() {
                    "model-list" => panel.model_menu_open = false,
                    "model-info" => panel.model_info_open = false,
                    "credit-card" => panel.credits_hover = false,
                    _ => return,
                }
                cx.notify();
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
        let artifact_sub = cx.subscribe(
            &feed,
            |_, _, event: &ArtifactAction, cx| match event {
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
            },
        );
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
                    ComposerEvent::ModelMenu(anchor) => {
                        panel.model_menu_open = !panel.model_menu_open
                            || panel.model_menu_anchor != *anchor;
                        panel.model_menu_anchor = anchor;
                        cx.notify();
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
        let mut panel = Self {
            ui,
            theme,
            feed,
            composer,
            _composer_sub: composer_sub,
            _feed_sub: feed_sub,
            _reveal_sub: reveal_sub,
            _deliverables_sub: deliverables_sub,
            _artifact_sub: artifact_sub,
            submission: None,
            submission_pending: false,
            submission_generation: 0,
            submission_stop: None,
            working,
            status,
            last_elapsed: None,
            _theme_sub: theme_sub,
            _overlay_sub: overlay_sub,
            prompt,
            projects,
            active_project_id,
            project_store_failed,
            document: DocumentSession::new(product),
            agent,
            credits: crate::CreditsViewState::Unavailable,
            credits_hover: false,
            credits_close: None,
            model_info_open,
            model_menu_open: false,
            tiers: Vec::new(),
            model_menu_anchor: "model-menu-trigger",
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
            starter_category: Category::Website,
            starter_offset: 0.,
            deliverables: None,
            preview_file: None,
            preview_body: None,
            drafts: std::collections::HashMap::new(),
            _prompt_sub: prompt_sub,
            pump: None,
        };
        panel.start_pump(cx);
        panel.refresh_chat(cx);
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
                });
                if ok.is_err() {
                    break;
                }
            }
        }));
    }

    fn process_agent_updates(
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
            self.active_project_id = self
                .agent
                .active_workspace()
                .map(|workspace| workspace.project_id);
            if matches!(self.surface, StudioSurface::Home | StudioSurface::Chat)
            {
                self.surface = if self.agent.has_messages() {
                    StudioSurface::Chat
                } else {
                    StudioSurface::Home
                };
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
        if initialized
            || updates.iter().any(|update| {
                matches!(update, flowy_agent::AgentUpdate::StorageError)
            })
        {
            self.emit_notice(Some("notice.session_storage_failed"), cx);
        }
        self.refresh_chat(cx);
    }

    fn refresh_chat(&mut self, cx: &mut Context<'_, Self>) {
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
                cx.emit(StudioEvent::TurnFinished);
            }
        }
    }

    fn refresh_working(&self, cx: &mut Context<'_, Self>) {
        let label = if self.submission_pending {
            self.t("chat.status_running")
        } else {
            self.working_label(&self.status)
        };
        let error = !self.submission_pending
            && matches!(self.status, flowy_agent::TurnStatus::Error { .. });
        let visible =
            self.submission_pending || working_line_visible(&self.status);
        self.working.update(cx, |working, cx| {
            working.set_label(label, visible, error, cx);
        });
    }

    fn invalidate_submission(&mut self, cx: &mut Context<'_, Self>) {
        self.submission_generation = self.submission_generation.wrapping_add(1);
        self.submission_pending = false;
        self.submission_stop = None;
        self.submission = None;
        self.refresh_working(cx);
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

    /// Open the folder holding `path`. This is the only step that can fail, so
    /// a failure goes through the existing notice channel instead of silence.
    fn reveal_file(&mut self, path: &str, cx: &mut Context<'_, Self>) {
        let folder = std::path::Path::new(path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let Some(folder) = folder else {
            self.emit_notice(Some("project.view_failed"), cx);
            return;
        };
        if open::that(folder).is_err() {
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

    pub fn reset_session(&mut self, cx: &mut Context<'_, Self>) {
        self.invalidate_submission(cx);
        self.model_info_open = false;
        self.model_menu_open = false;
        self.active_project_id = None;
        self.document = DocumentSession::new(self.product);
        let _ = self.agent.clear_active();
        self.agent.set_workspace_for_new_sessions(None);
        self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
        self.surface = StudioSurface::Home;
        self.refresh_chat(cx);
        self.emit_notice(None, cx);
        self.credits_hover = false;
        self.credits_close = None;
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    pub fn new_task(&mut self, cx: &mut Context<'_, Self>) {
        match self.agent.clear_active() {
            Ok(()) => {
                self.invalidate_submission(cx);
                self.active_project_id = None;
                self.agent.set_workspace_for_new_sessions(None);
                self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
                self.surface = StudioSurface::Home;
                // A fresh task starts at the first card, not wherever the last
                // row happened to be scrolled to.
                self.starter_offset = 0.;
                self.refresh_chat(cx);
                self.emit_notice(None, cx);
                self.prompt.update(cx, |state, cx| state.emplace("", cx));
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
        self.model_info_open = false;
        self.model_menu_open = false;
        self.active_project_id = None;
        self.document = DocumentSession::new(self.product);
        self.submission = None;
        self.surface = StudioSurface::Home;
        self.prompt.update(cx, |state, cx| state.emplace("", cx));
        self.refresh_chat(cx);
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
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
        self.surface = StudioSurface::Home;
        self.refresh_chat(cx);
        self.prompt.update(cx, |state, cx| state.emplace("", cx));
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    pub fn open_session(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        // Reopening the current session must not round-trip its own draft.
        let same = self.agent.active_session_id().as_deref() == Some(id);
        if !same && let Some(current) = self.agent.active_session_id() {
            let draft = self.prompt.read(cx).as_str().to_owned();
            self.drafts.insert(current, draft);
        }
        match self.agent.select_session(id) {
            Ok(()) => {
                if !same {
                    // Restore whatever this session had typed before.
                    let draft =
                        self.drafts.get(id).cloned().unwrap_or_default();
                    self.fill_prompt(&draft, cx);
                }
                self.invalidate_submission(cx);
                let workspace = self.agent.active_workspace();
                self.active_project_id =
                    workspace.as_ref().map(|value| value.project_id.clone());
                self.surface = if self.agent.has_messages() {
                    StudioSurface::Chat
                } else {
                    StudioSurface::Home
                };
                self.refresh_chat(cx);
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
            self.surface = StudioSurface::Home;
            self.document = DocumentSession::new(self.product);
            self.active_project_id = None;
            self.agent.set_workspace_for_new_sessions(None);
            self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
            self.prompt.update(cx, |state, cx| state.emplace("", cx));
        }
        self.refresh_chat(cx);
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
        tracing::debug!(
            target: "flowy.prompt",
            stage = "StudioPanel::send_prompt",
            cloud_ready = self.cloud_ready,
            model = ?model,
            chars = self.prompt.read(cx).as_str().trim().chars().count(),
            "enter"
        );
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
        let text = self.prompt.read(cx).as_str().trim().to_string();
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
                worker_agent.stop_if_active(Some(session_id));
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
                        agent.stop_if_active(Some(&session_id));
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
                    panel.prompt.read(cx).as_str().trim() == text;
                match result {
                    Ok(session_id)
                        if active.as_deref() == Some(session_id.as_str()) =>
                    {
                        panel.submission = None;
                        if draft_matches {
                            panel
                                .prompt
                                .update(cx, |state, cx| state.emplace("", cx));
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

    pub fn stop_turn(&mut self, cx: &mut Context<'_, Self>) {
        if self.submission_pending {
            if let Some(stop) = &self.submission_stop {
                stop.store(true, Ordering::Release);
            }
        } else {
            let agent = self.agent.clone();
            let expected = agent.active_session_id();
            cx.background_executor()
                .spawn(async move {
                    agent.stop_if_active(expected.as_deref());
                })
                .detach();
        }
        cx.notify();
    }

    pub fn fill_prompt(&mut self, text: &str, cx: &mut Context<'_, Self>) {
        self.prompt.update(cx, |state, cx| state.emplace(text, cx));
        cx.notify();
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

    /// Switches which category's examples the home stage shows.
    pub(crate) fn set_starter_category(
        &mut self,
        category: Category,
        cx: &mut Context<'_, Self>,
    ) {
        if self.starter_category == category {
            return;
        }
        self.starter_category = category;
        // Each category is a different row; carrying the old offset over would
        // open the new one mid-scroll, potentially past its own end.
        self.starter_offset = 0.;
        cx.notify();
    }

    pub(crate) fn starter_category(&self) -> Category {
        self.starter_category
    }
    pub(crate) fn open_deliverables(
        &mut self,
        files: Vec<FileEntry>,
        cx: &mut Context<'_, Self>,
    ) {
        self.deliverables = Some(files);
        self.preview_file = None;
        self.preview_body = None;
        cx.notify();
    }

    /// Opens the drawer with the latest turn's files, or closes it if open.
    pub(crate) fn toggle_deliverables(&mut self, cx: &mut Context<'_, Self>) {
        if self.deliverables.is_some() {
            self.close_deliverables(cx);
            return;
        }
        let files = self.feed.read(cx).latest_deliverables();
        match files {
            Some(files) => self.open_deliverables(files, cx),
            None => self.emit_notice(Some("chat.no_deliverables"), cx),
        }
    }

    pub(crate) fn close_deliverables(&mut self, cx: &mut Context<'_, Self>) {
        self.deliverables = None;
        self.preview_file = None;
        self.preview_body = None;
        cx.notify();
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
        if was_active {
            self.active_project_id = None;
            let _ = self.agent.clear_active();
            self.agent.set_workspace_for_new_sessions(None);
            self.surface = StudioSurface::Home;
            self.refresh_chat(cx);
            self.emit_notice(None, cx);
        }
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
        open::that(&project.root).map_err(|_| "project.view_failed")
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
        self.agent.set_workspace_for_new_sessions(Some(workspace));
        self.agent
            .set_origin_for_new_sessions(SessionOrigin::Project {
                project_id: Some(project_id.clone()),
            });
        self.active_project_id = Some(project_id);
        self.surface = StudioSurface::Home;
        self.prompt.update(cx, |state, cx| state.emplace("", cx));
        self.refresh_chat(cx);
        self.emit_notice(clear_error, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    pub fn note(&mut self, key: &'static str, cx: &mut Context<'_, Self>) {
        self.emit_notice(Some(key), cx);
    }

    pub fn open_thread(&mut self, title: String, cx: &mut Context<'_, Self>) {
        self.fill_prompt(&title, cx);
        self.send_prompt(cx);
    }

    pub fn clear_thread(&mut self, cx: &mut Context<'_, Self>) {
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
        let model_label = self.model_label();
        let cloud_ready = self.cloud_ready;
        self.composer.update(cx, |composer, cx| {
            composer.sync_state(
                self.model,
                self.model_menu_open,
                self.submission_pending || self.status.is_busy(),
                model_label,
                cloud_ready,
                cx,
            );
        });
        self.workspace(window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::{cell::RefCell, rc::Rc};

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
        let panel = cx.new(|cx| {
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
            assert_eq!(panel.prompt.read(cx).as_str(), "");
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
            assert_eq!(panel.prompt.read(cx).as_str(), "keep for retry");
            assert_eq!(panel.submission, Some(id));
            assert!(!panel.agent.has_messages());
            assert_eq!(panel.notice, Some("project.folder_unavailable"));
            panel.send_prompt(cx);
            assert_eq!(panel.submission, Some(id));
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).as_str(), "keep for retry");
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
            assert_eq!(panel.prompt.read(cx).as_str(), "new session draft");
            assert!(!panel.agent.has_messages());
            assert_eq!(panel.surface, StudioSurface::Home);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn background_initialized_event_restores_chat_and_preserves_unsent_draft(
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
        let panel = cx.new(|cx| {
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
            assert_eq!(panel.surface, StudioSurface::Chat);
            assert_eq!(panel.agent.active_session_id().as_deref(), Some("restored"));
            assert_eq!(panel.prompt.read(cx).as_str(), "keep this draft");
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
        let panel = cx.update(|cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            cx.new(|cx| {
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
                    cx,
                )
            })
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
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test functions use the harness-provided mutable test context"
    )]
    fn persisted_error_is_visible_on_the_first_frame(cx: &mut TestAppContext) {
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
        let panel = cx.update(|cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            cx.new(|cx| {
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
                    cx,
                )
            })
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
        let (panel, cx) = cx.add_window_view(|_, cx| {
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
                cx,
            )
        });
        cx.update(|window, cx| {
            cx.bind_keys([gpui::KeyBinding::new(
                "enter",
                crate::SubmitComposer,
                Some("EditableText"),
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
            assert_eq!(panel.prompt.read(cx).as_str(), "");
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
        let (panel, cx) = cx.add_window_view(|_, cx| {
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
                    Some("EditableText"),
                ),
                gpui::KeyBinding::new(
                    "shift-enter",
                    gpui_ce_elements::editable_text::actions::Enter,
                    Some("EditableText"),
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
            assert_eq!(panel.prompt.read(cx).as_str(), "hello\n");
        });
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).as_str(), "");
            assert!(panel.agent.has_messages());
        });

        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_turn_keeps_the_working_line_visible() {
        use flowy_agent::TurnStatus;

        assert!(working_line_visible(&TurnStatus::Running));
        assert!(working_line_visible(&TurnStatus::Error {
            message: "chat.status_error".into(),
        }));
        assert!(!working_line_visible(&TurnStatus::Idle));
        assert!(!working_line_visible(&TurnStatus::Completed));
        assert!(!working_line_visible(&TurnStatus::Stopped));
    }

    #[gpui::test]
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test functions use the harness-provided mutable test context"
    )]
    fn background_session_updates_do_not_change_selected_surface(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-navigation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let panel = cx.update(|cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            cx.new(|cx| {
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
                    cx,
                )
            })
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
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test functions use the harness-provided mutable test context"
    )]
    fn theme_changes_preserve_draft_and_do_not_leak_between_panels(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir()
            .join(format!("flowy-studio-theme-{}", std::process::id()));
        let (first, second, theme) = cx.update(|cx| {
            let first_ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            let second_ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            let theme = first_ui.theme.clone();
            let make = |ui, cx: &mut gpui::App| {
                cx.new(|cx| {
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
                        cx,
                    )
                })
            };
            (make(first_ui, cx), make(second_ui, cx), theme)
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
                first.read(cx).prompt.read(cx).as_str(),
                "preserve draft"
            );
        });
        drop(first);
        drop(second);
        drop(theme);
        let _ = std::fs::remove_dir_all(root);
    }
}

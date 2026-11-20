use crate::composer::{ChatComposer, ComposerEvent};
use crate::feed::{ChatFeed, OpenTask, WorkingIndicator};
use flowy_agent::{
    AgentHandle, AgentWorkspace, LlmConfig, Model, Plan, ProviderCatalog,
};
use flowy_core::DocumentSession;
use flowy_i18n::I18n;
use flowy_ui::{ResolvedTheme, UiContext};
use gpui::{
    Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Task, Window, div, prelude::*,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
};
use std::path::PathBuf;
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
    submission: Option<u128>,
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
                panel.submission = None;
                if panel.surface != StudioSurface::Chat {
                    cx.notify();
                }
            });
        tracing::debug!(
            target: "flowy.boot",
            stage = "StudioPanel::new",
            "→ AgentHandle::with_config"
        );
        let agent = AgentHandle::with_config(store_root.clone(), llm);
        let surface = if agent.has_messages() {
            StudioSurface::Chat
        } else {
            StudioSurface::Home
        };
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
            submission: None,
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
        // Background activity updates session contents, not the selected surface.
        if updates
            .iter()
            .any(|update| matches!(update, flowy_agent::AgentUpdate::Sessions))
        {
            cx.emit(StudioEvent::SessionsChanged);
        }
        if updates.iter().any(|update| {
            matches!(update, flowy_agent::AgentUpdate::StorageError)
        }) {
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
        let label = self.working_label(&self.status);
        let error =
            matches!(self.status, flowy_agent::TurnStatus::Error { .. });
        let visible = working_line_visible(&self.status);
        self.working.update(cx, |working, cx| {
            working.set_label(label, visible, error, cx);
        });
    }

    pub fn set_surface(
        &mut self,
        surface: StudioSurface,
        cx: &mut Context<'_, Self>,
    ) {
        if self.surface != surface {
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
        self.model_info_open = false;
        self.model_menu_open = false;
        self.active_project_id = None;
        self.document = DocumentSession::new(self.product);
        let _ = self.agent.clear_active();
        self.agent.set_workspace_for_new_sessions(None);
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
                self.surface = StudioSurface::Home;
                self.refresh_chat(cx);
                self.emit_notice(None, cx);
                self.prompt.update(cx, |state, cx| state.emplace("", cx));
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    /// Creates an empty session in the workspace `open_project` just prepared.
    pub fn create_task(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        self.agent.create_session()?;
        self.surface = StudioSurface::Home;
        self.refresh_chat(cx);
        self.prompt.update(cx, |state, cx| state.emplace("", cx));
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    pub fn open_session(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        match self.agent.select_session(id) {
            Ok(()) => {
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

    pub fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
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
        if self.agent.active_workspace().is_some_and(|workspace| {
            !crate::projects::root_available(&workspace.root)
        }) {
            self.emit_notice(Some("project.folder_unavailable"), cx);
            return;
        }
        let id = *self.submission.get_or_insert_with(new_submission_id);
        let result = self.agent.submit_chat(id, &text).and_then(|session| {
            if self.agent.active_session_id().as_deref() != Some(&session) {
                self.agent.select_session(&session)?;
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                self.prompt.update(cx, |state, cx| state.emplace("", cx));
                self.surface = StudioSurface::Chat;
                self.refresh_chat(cx);
                self.emit_notice(None, cx);
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => {
                tracing::debug!(
                    target: "flowy.prompt",
                    stage = "StudioPanel::send_prompt",
                    reason = key,
                    "AgentHandle::send failed"
                );
                self.emit_notice(Some(key), cx);
            }
        }
    }

    pub fn stop_turn(&mut self, cx: &mut Context<'_, Self>) {
        self.agent.stop();
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
        self.agent.set_workspace_for_new_sessions(Some(workspace));
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

    pub(crate) fn ideas_entry(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        flowy_ui::interactive(
            "studio-ideas",
            self.t("gallery.browse"),
            &self.theme,
        )
        .px(self.theme.space(16.))
        .py(self.theme.space(12.))
        .rounded_xl()
        .bg(self.theme.soft())
        .cursor_pointer()
        .child(
            div()
                .text_sm()
                .text_color(self.theme.muted())
                .child(self.t("gallery.browse")),
        )
        .on_click(cx.listener(|_panel, _, _, cx| {
            cx.emit(StudioEvent::OpenIdeas);
            cx.notify();
        }))
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
                self.status.is_busy(),
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
                assert_eq!(panel.surface, StudioSurface::Chat);
                panel.set_surface(StudioSurface::Automation, cx);
                let updates = panel.agent.drain_updates();
                assert!(updates.iter().any(|update| matches!(
                    update,
                    flowy_agent::AgentUpdate::Session { .. }
                )));
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

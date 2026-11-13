use flowy_agent::{AgentHandle, LlmConfig, Model, Plan};
use flowy_core::DocumentSession;
use flowy_i18n::I18n;
use gpui::{
    Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Task, Window, div, prelude::*,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
};
use std::path::PathBuf;
use std::time::Duration;

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

#[derive(Clone, Debug)]
pub enum StudioEvent {
    Notice(Option<&'static str>),
    ModelChanged {
        model: Model,
        plan: Plan,
        model_info_version: String,
    },
    OpenFlex,
    OpenIdeas,
    SessionsChanged,
    Rated { stars: u32 },
}

pub struct StudioPanel {
    pub prompt: Entity<EditableTextState>,
    pub projects: Vec<usize>,
    pub document: DocumentSession,
    pub agent: AgentHandle,
    pub credits: u32,
    pub credits_hover: bool,
    pub credits_close: Option<Task<()>>,
    pub model_info_open: bool,
    pub model_menu_open: bool,
    pub i18n: I18n,
    pub model: Model,
    pub plan: Plan,
    pub model_info_version: String,
    pub store_root: PathBuf,
    pub product: &'static str,
    pub surface: StudioSurface,
    pub notice: Option<&'static str>,
    _prompt_sub: gpui::Subscription,
    _pump: Option<Task<()>>,
}

impl EventEmitter<StudioEvent> for StudioPanel {}

impl StudioPanel {
    pub fn new(
        product: &'static str,
        i18n: I18n,
        model: Model,
        plan: Plan,
        model_info_version: String,
        model_info_open: bool,
        store_root: PathBuf,
        llm: LlmConfig,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let prompt =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let prompt_sub =
            cx.subscribe(&prompt, |_, _, _: &TextChanged, cx| cx.notify());
        let agent = AgentHandle::with_config(store_root.clone(), llm);
        let surface = if agent.has_messages() {
            StudioSurface::Chat
        } else {
            StudioSurface::Home
        };
        let mut panel = Self {
            prompt,
            projects: Vec::new(),
            document: DocumentSession::new(product),
            agent,
            credits: 2500,
            credits_hover: false,
            credits_close: None,
            model_info_open,
            model_menu_open: false,
            i18n,
            model,
            plan,
            model_info_version,
            store_root,
            product,
            surface,
            notice: None,
            _prompt_sub: prompt_sub,
            _pump: None,
        };
        panel.start_pump(cx);
        panel
    }

    fn start_pump(&mut self, cx: &mut Context<'_, Self>) {
        self._pump = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let ok = weak.update(cx, |panel, cx| {
                    let updates = panel.agent.drain_updates();
                    if updates.is_empty() {
                        return;
                    }
                    let mut sessions = false;
                    for update in updates {
                        match update {
                            flowy_agent::AgentUpdate::Sessions => sessions = true,
                            flowy_agent::AgentUpdate::Session { .. } => {
                                if panel.agent.has_messages() {
                                    panel.surface = StudioSurface::Chat;
                                }
                            }
                        }
                    }
                    if sessions {
                        cx.emit(StudioEvent::SessionsChanged);
                    }
                    cx.notify();
                });
                if ok.is_err() {
                    break;
                }
            }
        }));
    }

    pub fn set_surface(&mut self, surface: StudioSurface, cx: &mut Context<'_, Self>) {
        if self.surface != surface {
            self.surface = surface;
            cx.notify();
        }
    }

    pub fn sync_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n;
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
        if self.notice != notice {
            self.notice = notice;
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
        self.notice = notice;
        cx.emit(StudioEvent::Notice(notice));
        cx.notify();
    }

    pub(crate) fn emit_model(&mut self, cx: &mut Context<'_, Self>) {
        cx.emit(StudioEvent::ModelChanged {
            model: self.model,
            plan: self.plan,
            model_info_version: self.model_info_version.clone(),
        });
    }

    pub fn reset_session(&mut self, cx: &mut Context<'_, Self>) {
        self.model_info_open = false;
        self.model_menu_open = false;
        self.projects.clear();
        self.document = DocumentSession::new(self.product);
        let _ = self.agent.create_session();
        self.surface = StudioSurface::Home;
        self.notice = None;
        self.credits_hover = false;
        self.credits_close = None;
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    pub fn new_task(&mut self, cx: &mut Context<'_, Self>) {
        match self.agent.create_session() {
            Ok(_) => {
                self.surface = StudioSurface::Home;
                self.notice = None;
                self.prompt.update(cx, |state, cx| state.emplace("", cx));
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    pub fn open_session(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        match self.agent.select_session(id) {
            Ok(()) => {
                self.surface = if self.agent.has_messages() {
                    StudioSurface::Chat
                } else {
                    StudioSurface::Home
                };
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    pub fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
        let text = self.prompt.read(cx).as_str().trim().to_string();
        if text.is_empty() {
            self.emit_notice(Some("notice.prompt_empty"), cx);
            return;
        }
        match self.agent.send(&text) {
            Ok(()) => {
                self.prompt.update(cx, |state, cx| state.emplace("", cx));
                self.surface = StudioSurface::Chat;
                self.notice = None;
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    pub fn stop_turn(&mut self, cx: &mut Context<'_, Self>) {
        self.agent.stop();
        cx.notify();
    }

    pub fn fill_prompt(&mut self, text: &str, cx: &mut Context<'_, Self>) {
        self.prompt
            .update(cx, |state, cx| state.emplace(text, cx));
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

    pub fn add_project(&mut self, cx: &mut Context<'_, Self>) {
        let index = self.projects.len().saturating_add(1);
        self.projects.push(index);
        self.emit_notice(Some("notice.project_created"), cx);
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

    pub(crate) fn ideas_entry(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("studio-ideas")
            .w_full()
            .px_4()
            .py_3()
            .rounded_xl()
            .bg(flowy_ui::soft())
            .cursor_pointer()
            .child(
                div()
                    .text_sm()
                    .text_color(flowy_ui::muted())
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
        _window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        self.workspace(cx)
    }
}

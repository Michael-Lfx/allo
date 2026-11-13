#![allow(clippy::derive_partial_eq_without_eq)]

mod appearance;
mod dock;
mod flex;
mod gallery;
mod invite;
mod login;
mod more;
mod ota;
mod settings;
mod state;

use crate::i18n::{I18n, file_stamps};
use crate::prefs::Preferences;
use crate::session::SessionStore;
use flowy_auth::{
    AuthClient, AuthError, EMAIL_OTP_COOLDOWN_SECS, EMAIL_OTP_LENGTH, normalize_otp,
};
use flowy_studio::{StudioEvent, StudioPanel, StudioSurface};
use gpui::{
    App, Bounds, Context, Entity, KeyBinding, Subscription, Task, Window,
    WindowAppearance, WindowBounds, WindowOptions, actions, div, prelude::*,
    px, size,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
    actions::{DEFAULT_INPUT_CONTEXT, default_bindings},
};
use state::{
    AppearanceChrome, AuthState, FlexState, GalleryState, InviteState, LegalDoc,
    LoginPhase, LoginStep, PersonaState, SettingsState,
};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

actions!(flowy, [SignIn]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Login,
    Main,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Studio,
    // Computer,
    // Agent,
    Search,
    Library,
    Create,
    Automation,
    Plugins,
    More,
}

impl Section {
    pub(crate) const fn title_key(self) -> &'static str {
        match self {
            Self::Studio => "studio.brand",
            // Self::Computer => "dock.computer",
            // Self::Agent => "dock.agent",
            Self::Search => "dock.search",
            Self::Library => "dock.library",
            Self::Create => "dock.create",
            Self::Automation => "dock.automation",
            Self::Plugins => "dock.plugins",
            Self::More => "dock.more",
        }
    }

    pub(crate) const fn detail_key(self) -> &'static str {
        match self {
            Self::Studio => "studio.brand",
            // Self::Computer => "section.computer",
            // Self::Agent => "section.agent",
            Self::Search => "section.search",
            Self::Library => "section.library",
            Self::Create => "section.create",
            Self::Automation => "section.automation",
            Self::Plugins => "section.plugins",
            Self::More => "section.more",
        }
    }
}

pub struct FlowyApp {
    pub store: SessionStore,
    pub screen: Screen,
    pub section: Section,
    pub notice: Option<&'static str>,
    pub auth: AuthState,
    pub studio: Entity<StudioPanel>,
    pub gallery: GalleryState,
    pub ota: ota::OtaController,
    pub settings: SettingsState,
    pub invite: InviteState,
    pub persona: PersonaState,
    pub appearance: AppearanceChrome,
    pub flex: FlexState,
    pub prefs: Preferences,
    pub i18n: I18n,
    locale_watch: Option<Task<()>>,
    shell_watch: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
}

impl FlowyApp {
    fn new(store: SessionStore, cx: &mut Context<'_, Self>) -> Self {
        let session = store.load();
        let screen = if session.is_some() {
            Screen::Main
        } else {
            Screen::Login
        };
        let prefs = Preferences::load(store.root());
        let model_info_open =
            screen == Screen::Main && prefs.shows_model_info();
        let mut i18n = I18n::embedded(crate::brand::product());
        i18n.set_locale(prefs.locale);
        if let Some(dir) = I18n::dev_dir() {
            i18n.reload_dir(&dir);
        }
        let email =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let code =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let settings_query =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let invite_email =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let invite_book = invite::load(store.root());
        let saved = crate::persona::load(store.root());
        let nickname = text_field(cx);
        let occupation = text_field(cx);
        let about = text_field(cx);
        let instructions = text_field(cx);
        let import_paste = text_field(cx);
        let knowledge_name = text_field(cx);
        let knowledge_usage = text_field(cx);
        let knowledge_body = text_field(cx);
        let font_size_field = text_field(cx);
        let flex_api_key = text_field(cx);
        place(&nickname, &saved.nickname, cx);
        place(&occupation, &saved.occupation, cx);
        place(&about, &saved.about, cx);
        place(&instructions, &saved.instructions, cx);
        place(&font_size_field, &prefs.font_size.to_string(), cx);
        place(&flex_api_key, &prefs.flex_api_key, cx);
        let studio = {
            let product = crate::brand::product();
            let store_root = store.root().to_path_buf();
            let i18n = i18n.clone();
            let model = prefs.model;
            let plan = prefs.plan;
            let model_info_version = prefs.model_info_version.clone();
            let llm = prefs.llm_config();
            cx.new(move |cx| {
                StudioPanel::new(
                    product,
                    i18n,
                    model,
                    plan,
                    model_info_version,
                    model_info_open,
                    store_root,
                    llm,
                    cx,
                )
            })
        };
        let subscriptions = vec![
            cx.subscribe(&email, |_, _, _: &TextChanged, cx| cx.notify()),
            cx.subscribe(&code, |app, _, _: &TextChanged, cx| {
                app.on_otp_changed(cx);
            }),
            cx.subscribe(&settings_query, |_, _, _: &TextChanged, cx| {
                cx.notify();
            }),
            cx.subscribe(&flex_api_key, |app, _, _: &TextChanged, cx| {
                app.touch_flex_key(cx);
            }),
            cx.subscribe(&invite_email, |_, _, _: &TextChanged, cx| {
                cx.notify();
            }),
            cx.subscribe(&nickname, |app, _, _: &TextChanged, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&occupation, |app, _, _: &TextChanged, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&about, |app, _, _: &TextChanged, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&instructions, |app, _, _: &TextChanged, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&import_paste, |app, _, _: &TextChanged, cx| {
                app.touch_paste(cx);
            }),
            cx.subscribe(&knowledge_name, |app, _, _: &TextChanged, cx| {
                app.touch_knowledge_name(cx);
            }),
            cx.subscribe(&knowledge_usage, |app, _, _: &TextChanged, cx| {
                app.touch_knowledge_usage(cx);
            }),
            cx.subscribe(&knowledge_body, |app, _, _: &TextChanged, cx| {
                app.touch_knowledge_body(cx);
            }),
            cx.subscribe(&font_size_field, |app, _, _: &TextChanged, cx| {
                app.touch_font_size(cx);
            }),
            cx.subscribe(&studio, |app, _, event: &StudioEvent, cx| {
                app.on_studio_event(event, cx);
            }),
        ];
        Self {
            store,
            screen,
            section: Section::Studio,
            notice: None,
            auth: AuthState {
                session,
                email,
                code,
                step: LoginStep::Email,
                phase: LoginPhase::Idle,
                message: None,
                pending_req_no: None,
                cooldown_left: 0,
                generation: 0,
                task: None,
                cooldown_task: None,
                legal: None,
            },
            studio,
            gallery: GalleryState {
                page: 0,
                ideas_open: false,
                idea_filter: gallery::Filter::All,
                idea_id: None,
                idea_note: false,
            },
            ota: ota::OtaController::new(),
            settings: SettingsState {
                open: false,
                page: settings::Page::General,
                picker: settings::Picker::None,
                query: settings_query,
            },
            invite: InviteState {
                open: false,
                panel: invite::Panel::Share,
                status: None,
                code: invite_book.code,
                inbound_code: None,
                emails: invite_book.emails,
                email: invite_email,
            },
            persona: PersonaState {
                menu: false,
                page: more::Page::Personal,
                tab: more::Tab::Profile,
                dialog: more::Dialog::None,
                status: None,
                knowledge: saved.knowledge,
                nickname,
                occupation,
                about,
                instructions,
                import_paste,
                knowledge_name,
                knowledge_usage,
                knowledge_body,
            },
            appearance: AppearanceChrome {
                known: false,
                forced: None,
                font_size_field,
            },
            flex: FlexState {
                open: false,
                provider_menu: false,
                draft_provider: prefs.flex_provider,
                api_key: flex_api_key,
                secret: prefs.flex_api_key.clone(),
                key_visible: false,
            },
            prefs,
            i18n,
            locale_watch: None,
            shell_watch: None,
            subscriptions,
        }
    }

    pub fn receive_invite_code(
        &mut self,
        code: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.invite.inbound_code = Some(code);
        self.invite.status = Some("invite.deep_link_received");
        self.open_invite(cx);
    }

    fn submit_login(&mut self, cx: &mut Context<'_, Self>) {
        match self.auth.step {
            LoginStep::Email => self.send_login_code(cx),
            LoginStep::Otp => self.verify_login_code(cx),
        }
    }

    fn send_login_code(&mut self, cx: &mut Context<'_, Self>) {
        if matches!(
            self.auth.phase,
            LoginPhase::Sending | LoginPhase::Verifying | LoginPhase::Success
        ) || self.auth.cooldown_left > 0
        {
            return;
        }
        let email = self.auth.email.read(cx).as_str().to_string();
        self.auth.generation = self.auth.generation.saturating_add(1);
        let generation = self.auth.generation;
        self.auth.phase = LoginPhase::Sending;
        self.auth.message = None;
        self.auth
            .code
            .update(cx, |state, cx| state.emplace("", cx));
        cx.notify();

        let client = AuthClient::from_env();
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.send_email_code(&email);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.auth.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(result) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.auth.generation != generation {
                        return;
                    }
                    match result {
                        Ok(req_no) => {
                            app.auth.pending_req_no = Some(req_no);
                            app.auth.step = LoginStep::Otp;
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.message = Some("login.code_sent");
                            app.start_login_cooldown(cx);
                        }
                        Err(error) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.message = Some(error.key());
                        }
                    }
                    cx.notify();
                });
                break;
            }
        }));
    }

    fn verify_login_code(&mut self, cx: &mut Context<'_, Self>) {
        if matches!(
            self.auth.phase,
            LoginPhase::Sending | LoginPhase::Verifying | LoginPhase::Success
        ) {
            return;
        }
        let Some(req_no) = self.auth.pending_req_no.clone() else {
            self.auth.message = Some("login.send_first");
            cx.notify();
            return;
        };
        let email = self.auth.email.read(cx).as_str().to_string();
        let code = normalize_otp(self.auth.code.read(cx).as_str());
        if code.len() != EMAIL_OTP_LENGTH {
            self.auth.message = Some(AuthError::OtpRequired.key());
            cx.notify();
            return;
        }
        self.auth.generation = self.auth.generation.saturating_add(1);
        let generation = self.auth.generation;
        self.auth.phase = LoginPhase::Verifying;
        self.auth.message = Some("login.verifying");
        cx.notify();

        let client = AuthClient::from_env();
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.complete_login(&email, &code, &req_no);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.auth.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(result) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.auth.generation != generation {
                        return;
                    }
                    match result {
                        Ok(session) => {
                            if let Err(error) = app.store.save(&session) {
                                app.auth.phase = LoginPhase::Idle;
                                app.auth.message = Some(error.key());
                                cx.notify();
                                return;
                            }
                            app.auth
                                .code
                                .update(cx, |state, cx| state.emplace("", cx));
                            app.auth.session = Some(session);
                            app.auth.phase = LoginPhase::Success;
                            app.auth.message = Some("login.success");
                            app.auth.pending_req_no = None;
                            app.auth.cooldown_left = 0;
                            app.notice = None;
                            app.screen = Screen::Main;
                            let open = app.prefs.shows_model_info();
                            app.studio.update(cx, |panel, cx| {
                                panel.model_info_open = open;
                                cx.notify();
                            });
                        }
                        Err(AuthError::InvalidCode) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.message = Some(AuthError::InvalidCode.key());
                        }
                        Err(AuthError::SessionExpired) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.step = LoginStep::Email;
                            app.auth.pending_req_no = None;
                            app.auth.cooldown_left = 0;
                            app.auth.message =
                                Some(AuthError::SessionExpired.key());
                        }
                        Err(error) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.message = Some(error.key());
                        }
                    }
                    cx.notify();
                });
                break;
            }
        }));
    }

    fn on_otp_changed(&mut self, cx: &mut Context<'_, Self>) {
        if self.screen != Screen::Login || self.auth.step != LoginStep::Otp {
            cx.notify();
            return;
        }
        let raw = self.auth.code.read(cx).as_str().to_string();
        let normalized = normalize_otp(&raw);
        if normalized != raw {
            self.auth
                .code
                .update(cx, |state, cx| state.emplace(&normalized, cx));
        }
        cx.notify();
        if normalized.len() == EMAIL_OTP_LENGTH
            && self.auth.phase == LoginPhase::Idle
        {
            self.verify_login_code(cx);
        }
    }

    fn reset_login_to_email(&mut self, cx: &mut Context<'_, Self>) {
        self.auth.generation = self.auth.generation.saturating_add(1);
        self.auth.step = LoginStep::Email;
        self.auth.phase = LoginPhase::Idle;
        self.auth.message = None;
        self.auth.pending_req_no = None;
        self.auth.cooldown_left = 0;
        self.auth.task = None;
        self.auth.cooldown_task = None;
        self.auth
            .code
            .update(cx, |state, cx| state.emplace("", cx));
        cx.notify();
    }

    fn open_legal(&mut self, doc: LegalDoc, cx: &mut Context<'_, Self>) {
        self.auth.legal = Some(doc);
        cx.notify();
    }

    fn close_legal(&mut self, cx: &mut Context<'_, Self>) {
        self.auth.legal = None;
        cx.notify();
    }

    fn run_login_diagnose(&mut self, cx: &mut Context<'_, Self>) {
        if matches!(
            self.auth.phase,
            LoginPhase::Sending | LoginPhase::Verifying
        ) {
            return;
        }
        self.auth.generation = self.auth.generation.saturating_add(1);
        let generation = self.auth.generation;
        self.auth.message = Some("login.diagnose_running");
        cx.notify();
        let client = AuthClient::from_env();
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let ok = client.diagnose_network();
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(ok);
            }
        });
        self.auth.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(ok) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.auth.generation != generation {
                        return;
                    }
                    app.auth.message = Some(if ok {
                        "login.diagnose_ok"
                    } else {
                        "login.diagnose_failed"
                    });
                    cx.notify();
                });
                break;
            }
        }));
    }

    fn start_login_cooldown(&mut self, cx: &mut Context<'_, Self>) {
        self.auth.cooldown_left = EMAIL_OTP_COOLDOWN_SECS;
        self.auth.cooldown_task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(1))
                    .await;
                let cont = weak
                    .update(cx, |app, cx| {
                        if app.auth.cooldown_left == 0 {
                            return false;
                        }
                        app.auth.cooldown_left =
                            app.auth.cooldown_left.saturating_sub(1);
                        cx.notify();
                        app.auth.cooldown_left > 0
                    })
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        }));
    }

    fn sign_out(&mut self, cx: &mut Context<'_, Self>) {
        self.store.clear();
        self.auth
            .email
            .update(cx, |state, cx| state.emplace("", cx));
        self.auth
            .code
            .update(cx, |state, cx| state.emplace("", cx));
        self.auth.session = None;
        self.auth.step = LoginStep::Email;
        self.auth.phase = LoginPhase::Idle;
        self.auth.message = None;
        self.auth.pending_req_no = None;
        self.auth.cooldown_left = 0;
        self.auth.task = None;
        self.auth.cooldown_task = None;
        self.auth.legal = None;
        self.screen = Screen::Login;
        self.section = Section::Studio;
        self.settings.open = false;
        self.invite.open = false;
        self.persona.menu = false;
        self.persona.dialog = more::Dialog::None;
        self.persona.status = None;
        self.notice = None;
        self.studio.update(cx, |panel, cx| panel.reset_session(cx));
        self.gallery.page = 0;
        self.gallery.ideas_open = false;
        self.gallery.idea_id = None;
        self.gallery.idea_note = false;
        self.ota.reset();
        cx.notify();
    }

    pub fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
        self.sync_studio_surface(cx);
        self.studio.update(cx, |panel, cx| panel.send_prompt(cx));
    }

    pub fn fill_prompt(&mut self, text: &str, cx: &mut Context<'_, Self>) {
        self.studio
            .update(cx, |panel, cx| panel.fill_prompt(text, cx));
    }

    pub fn fill_prompt_key(
        &mut self,
        key: &'static str,
        cx: &mut Context<'_, Self>,
    ) {
        self.studio
            .update(cx, |panel, cx| panel.fill_prompt_key(key, cx));
    }

    pub fn add_project(&mut self, cx: &mut Context<'_, Self>) {
        self.studio.update(cx, |panel, cx| panel.add_project(cx));
    }

    pub fn open_thread(&mut self, title: String, cx: &mut Context<'_, Self>) {
        self.section = Section::Studio;
        self.sync_studio_surface(cx);
        self.studio
            .update(cx, |panel, cx| panel.open_thread(title, cx));
    }

    pub fn clear_thread(&mut self, cx: &mut Context<'_, Self>) {
        self.studio.update(cx, |panel, cx| panel.clear_thread(cx));
    }

    fn sync_studio_surface(&mut self, cx: &mut Context<'_, Self>) {
        let notice = self.notice;
        match self.section {
            Section::Studio => {
                // Home vs Chat is owned by StudioPanel.
                self.studio.update(cx, |panel, cx| {
                    panel.set_notice(notice, cx);
                });
            }
            Section::Automation => {
                self.studio.update(cx, |panel, cx| {
                    panel.set_surface(StudioSurface::Automation, cx);
                    panel.set_notice(notice, cx);
                });
            }
            Section::Search => {
                self.studio.update(cx, |panel, cx| {
                    panel.set_surface(
                        StudioSurface::Placeholder {
                            title_key: "dock.search",
                            detail_key: "section.search",
                        },
                        cx,
                    );
                    panel.set_notice(notice, cx);
                });
            }
            Section::Library => {
                self.studio.update(cx, |panel, cx| {
                    panel.set_surface(
                        StudioSurface::Placeholder {
                            title_key: "dock.library",
                            detail_key: "section.library",
                        },
                        cx,
                    );
                    panel.set_notice(notice, cx);
                });
            }
            Section::Create => {
                self.studio.update(cx, |panel, cx| {
                    panel.set_surface(
                        StudioSurface::Placeholder {
                            title_key: "dock.create",
                            detail_key: "section.create",
                        },
                        cx,
                    );
                    panel.set_notice(notice, cx);
                });
            }
            Section::Plugins => {
                self.studio.update(cx, |panel, cx| {
                    panel.set_surface(
                        StudioSurface::Placeholder {
                            title_key: "dock.plugins",
                            detail_key: "section.plugins",
                        },
                        cx,
                    );
                    panel.set_notice(notice, cx);
                });
            }
            Section::More => {
                self.studio.update(cx, |panel, cx| {
                    panel.set_notice(notice, cx);
                });
            }
        }
    }

    fn on_studio_event(
        &mut self,
        event: &StudioEvent,
        cx: &mut Context<'_, Self>,
    ) {
        match event {
            StudioEvent::Notice(notice) => {
                self.notice = *notice;
                cx.notify();
            }
            StudioEvent::ModelChanged {
                model,
                plan,
                model_info_version,
            } => {
                self.prefs.model = *model;
                self.prefs.plan = *plan;
                self.prefs.model_info_version = model_info_version.clone();
                self.prefs.save(self.store.root());
                self.sync_agent_llm(cx);
                cx.notify();
            }
            StudioEvent::OpenFlex => {
                self.open_flex(cx);
            }
            StudioEvent::OpenIdeas => {
                self.gallery.ideas_open = true;
                cx.notify();
            }
            StudioEvent::SessionsChanged => {
                cx.notify();
            }
            StudioEvent::Rated { stars } => {
                if let Some(telemetry) = crate::telemetry() {
                    let mut props = serde_json::Map::new();
                    props.insert(
                        "stars".into(),
                        serde_json::Value::Number((*stars).into()),
                    );
                    telemetry.capture("chat_rating", props);
                }
                cx.notify();
            }
        }
    }

    fn main_canvas(
        &self,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut canvas = div()
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(flowy_ui::canvas())
            .text_color(flowy_ui::ink())
            .child(self.dock(window, cx));
        canvas = if self.section == Section::More {
            canvas.child(self.more_canvas(cx))
        } else {
            canvas.child(self.studio.clone())
        };
        if self.persona.menu && self.section == Section::More {
            canvas = canvas.child(self.more_flyout(cx));
        }
        canvas
    }

    pub fn note(&mut self, key: &'static str, cx: &mut Context<'_, Self>) {
        self.notice = Some(key);
        self.studio
            .update(cx, |panel, cx| panel.set_notice(Some(key), cx));
        cx.notify();
    }

    pub fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    pub fn tf(&self, key: &str, vars: &[(&str, &str)]) -> String {
        self.i18n.tf(key, vars)
    }

    fn start_locale_watch(&mut self, cx: &Context<'_, Self>) {
        let Some(dir) = I18n::dev_dir() else {
            return;
        };
        let mut stamps = file_stamps(&dir);
        self.locale_watch = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let next = file_stamps(&dir);
                if next == stamps {
                    continue;
                }
                stamps = next;
                let dir = dir.clone();
                let updated = weak.update(cx, |app, cx| {
                    app.i18n.reload_dir(&dir);
                    let i18n = app.i18n.clone();
                    app.studio.update(cx, |panel, cx| {
                        panel.sync_i18n(i18n, cx);
                    });
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    fn start_shell_watch(&mut self, cx: &Context<'_, Self>) {
        self.shell_watch = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let pending = flowy_shell::take_pending();
                if pending.is_empty() {
                    continue;
                }
                let updated = weak.update(cx, |app, cx| {
                    for url in pending {
                        match flowy_shell::parse_deep_link(&url) {
                            Some(flowy_shell::DeepLink::Invite { code }) => {
                                app.receive_invite_code(code, cx);
                            }
                            Some(flowy_shell::DeepLink::Open) | None => {}
                        }
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }
}

impl Render for FlowyApp {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        self.apply_appearance(window, cx);
        if self.screen == Screen::Main && !self.ota.checked {
            self.ota.checked = true;
            self.start_update_check(cx);
        }
        let mut root = div().id("flowy").size_full().bg(flowy_ui::canvas());
        root = if self.i18n.locale().is_rtl() {
            root.rtl()
        } else {
            root.ltr()
        };
        root = root.relative();
        root = if self.screen == Screen::Login {
            let mut login = div()
                .id("login-root")
                .size_full()
                .relative()
                .child(self.login_canvas(window, cx));
            if self.auth.legal.is_some() {
                login = login.child(self.legal_layer(cx));
            }
            root.child(login)
        } else if self.settings.open {
            root.child(self.settings_canvas(window, cx))
        } else {
            self.sync_studio_surface(cx);
            root.child(self.main_canvas(window, cx))
        };
        if self.gallery.ideas_open
            && self.screen == Screen::Main
            && !self.settings.open
        {
            root = root.child(self.ideas_layer(cx));
        }
        if self.gallery.idea_id.is_some()
            && self.screen == Screen::Main
            && !self.settings.open
        {
            root = root.child(self.idea_layer(cx));
        }
        if self.invite.open && self.screen == Screen::Main {
            root = root.child(self.invite_layer(cx));
        }
        if self.persona.dialog != more::Dialog::None
            && self.screen == Screen::Main
            && !self.settings.open
        {
            root = root.child(self.more_layer(cx));
        }
        if self.flex.open && self.screen == Screen::Main && !self.settings.open
        {
            root = root.child(self.flex_layer(cx));
        }
        if self.prefs.activity && self.screen == Screen::Main {
            root = root.child(self.activity_floater());
        }
        root
    }
}

impl FlowyApp {
    fn apply_appearance(
        &mut self,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) {
        let forced = match self.prefs.theme {
            crate::prefs::ThemeMode::Light => Some(WindowAppearance::Light),
            crate::prefs::ThemeMode::Dark => Some(WindowAppearance::Dark),
            crate::prefs::ThemeMode::Auto => None,
        };
        if !self.appearance.known || self.appearance.forced != forced {
            self.appearance.known = true;
            self.appearance.forced = forced;
            cx.set_window_appearance(forced);
        }
        let system_dark = matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let dark = match self.prefs.theme {
            crate::prefs::ThemeMode::Dark => true,
            crate::prefs::ThemeMode::Light => false,
            crate::prefs::ThemeMode::Auto => system_dark,
        };
        flowy_ui::install(dark, self.prefs.accent, self.prefs.density);
        window.set_rem_size(px(f32::from(self.prefs.font_size) / 0.875));
    }
}

fn text_field(cx: &mut Context<'_, FlowyApp>) -> Entity<EditableTextState> {
    cx.new(|cx| EditableTextState::new(StringStorage::default(), cx))
}

fn place(
    field: &Entity<EditableTextState>,
    text: &str,
    cx: &mut Context<'_, FlowyApp>,
) {
    field.update(cx, |state, cx| state.emplace(text, cx));
}

fn product_mark(size: f32) -> gpui::Img {
    static LOGO: OnceLock<Arc<gpui::Image>> = OnceLock::new();
    let image = LOGO.get_or_init(|| {
        Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            crate::brand::logo_png().to_vec(),
        ))
    });
    gpui::img(image.clone()).size(px(size)).rounded(px(6.))
}

pub fn run(cx: &mut App) {
    cx.set_app_identity(crate::brand::bundle_id(), crate::brand::product());
    cx.bind_keys(
        default_bindings().as_keybindings(Some(DEFAULT_INPUT_CONTEXT)),
    );
    cx.bind_keys([KeyBinding::new("enter", SignIn, Some("Login"))]);
    flowy_shell::install(
        flowy_shell::Brand {
            product: crate::brand::product(),
            bundle_id: crate::brand::bundle_id(),
            url_scheme: crate::brand::url_scheme(),
            logo_png: crate::brand::logo_png(),
        },
        |cx| {
            ensure_shell_main_window(cx);
        },
        cx,
    );
    let window = open_main_window(cx);
    bind_shell_main_window(window);
    cx.activate(true);
}

pub fn open_main_window(cx: &mut App) -> gpui::WindowHandle<FlowyApp> {
    let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(gpui::SharedString::from(crate::brand::product())),
            ..gpui::TitlebarOptions::default()
        }),
        window_min_size: Some(size(px(960.), px(640.))),
        ..WindowOptions::default()
    };
    let store = SessionStore::new(crate::session::default_root());
    let window = cx
        .open_window(options, |_window, cx| {
            cx.new(|cx| FlowyApp::new(store, cx))
        })
        .expect("open main window");
    window
        .update(cx, |app, window, cx| {
            app.start_locale_watch(cx);
            app.start_shell_watch(cx);
            let subscription =
                cx.observe_window_appearance(window, |app, window, cx| {
                    app.apply_appearance(window, cx);
                    cx.notify();
                });
            app.subscriptions.push(subscription);
            apply_pending_deep_links(app, cx);
        })
        .ok();
    window
}

pub fn default_store_root() -> std::path::PathBuf {
    crate::session::default_root()
}

static SHELL_MAIN: OnceLock<
    std::sync::Mutex<Option<gpui::WindowHandle<FlowyApp>>>,
> = OnceLock::new();

fn bind_shell_main_window(window: gpui::WindowHandle<FlowyApp>) {
    *SHELL_MAIN
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap() = Some(window);
}

fn ensure_shell_main_window(cx: &mut App) {
    let existing = *SHELL_MAIN
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap();
    if let Some(window) = existing {
        let alive = window
            .update(cx, |_app, window, _cx| {
                window.activate();
            })
            .is_ok();
        if alive {
            cx.activate(true);
            return;
        }
    }
    let window = open_main_window(cx);
    bind_shell_main_window(window);
    cx.activate(true);
}

fn apply_pending_deep_links(
    app: &mut FlowyApp,
    cx: &mut Context<'_, FlowyApp>,
) {
    for url in flowy_shell::take_pending() {
        match flowy_shell::parse_deep_link(&url) {
            Some(flowy_shell::DeepLink::Invite { code }) => {
                app.receive_invite_code(code, cx);
            }
            Some(flowy_shell::DeepLink::Open) | None => {}
        }
    }
}

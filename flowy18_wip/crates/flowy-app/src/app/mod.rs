#![allow(clippy::derive_partial_eq_without_eq)]

mod appearance;
mod automation;
mod dock;
mod gallery;
mod invite;
mod login;
mod more;
mod ota;
mod settings;
mod state;
mod studio;
mod thread;

use crate::i18n::{I18n, file_stamps};
use crate::prefs::Preferences;
use crate::session::{AuthError, Store};
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
    AppearanceChrome, AuthState, GalleryState, InviteState, PersonaState,
    SettingsState, StudioState,
};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

actions!(flowy, [SignIn]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Login,
    Main,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Studio,
    // Computer,
    Agent,
    Search,
    Library,
    Create,
    Automation,
    Plugins,
    More,
}

impl Section {
    const fn title_key(self) -> &'static str {
        match self {
            Self::Studio => "studio.brand",
            // Self::Computer => "dock.computer",
            Self::Agent => "dock.agent",
            Self::Search => "dock.search",
            Self::Library => "dock.library",
            Self::Create => "dock.create",
            Self::Automation => "dock.automation",
            Self::Plugins => "dock.plugins",
            Self::More => "dock.more",
        }
    }

    const fn detail_key(self) -> &'static str {
        match self {
            Self::Studio => "studio.brand",
            // Self::Computer => "section.computer",
            Self::Agent => "section.agent",
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
    store: Store,
    screen: Screen,
    section: Section,
    notice: Option<&'static str>,
    auth: AuthState,
    studio: StudioState,
    gallery: GalleryState,
    ota: ota::OtaController,
    settings: SettingsState,
    invite: InviteState,
    persona: PersonaState,
    appearance: AppearanceChrome,
    prefs: Preferences,
    i18n: I18n,
    locale_watch: Option<Task<()>>,
    shell_watch: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
}

impl FlowyApp {
    fn new(store: Store, cx: &mut Context<'_, Self>) -> Self {
        let session = store.load();
        let account_exists = store.has_accounts();
        let screen = if session.is_some() {
            Screen::Main
        } else {
            Screen::Login
        };
        let prefs = Preferences::load(store.root());
        let model_info_open =
            screen == Screen::Main && prefs.shows_model_info();
        let mut i18n = I18n::embedded();
        i18n.set_locale(prefs.locale);
        if let Some(dir) = I18n::dev_dir() {
            i18n.reload_dir(&dir);
        }
        let email =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let password =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let prompt =
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
        place(&nickname, &saved.nickname, cx);
        place(&occupation, &saved.occupation, cx);
        place(&about, &saved.about, cx);
        place(&instructions, &saved.instructions, cx);
        place(&font_size_field, &prefs.font_size.to_string(), cx);
        let subscriptions = vec![
            cx.subscribe(&email, |_, _, _: &TextChanged, cx| cx.notify()),
            cx.subscribe(&password, |_, _, _: &TextChanged, cx| cx.notify()),
            cx.subscribe(&prompt, |_, _, _: &TextChanged, cx| cx.notify()),
            cx.subscribe(&settings_query, |_, _, _: &TextChanged, cx| {
                cx.notify();
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
        ];
        Self {
            store,
            screen,
            section: Section::Studio,
            notice: None,
            auth: AuthState {
                session,
                account_exists,
                email,
                password,
                password_visible: false,
                login_error: None,
            },
            studio: StudioState {
                prompt,
                projects: Vec::new(),
                tasks: Vec::new(),
                thread: None,
                stream: None,
                stream_task: None,
                confirm_rights: false,
                confirm_terms: false,
                document: flowy_core::DocumentSession::new(
                    crate::brand::product(),
                ),
                agent: crate::agent::AgentHandle::new(),
                credits: 2500,
                credits_hover: false,
                credits_close: None,
                model_info_open,
                model_menu_open: false,
            },
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
            prefs,
            i18n,
            locale_watch: None,
            shell_watch: None,
            subscriptions,
        }
    }

    pub(crate) fn receive_invite_code(
        &mut self,
        code: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.invite.inbound_code = Some(code);
        self.invite.status = Some("invite.deep_link_received");
        self.open_invite(cx);
    }

    fn submit_login(&mut self, cx: &mut Context<'_, Self>) {
        let email = self.auth.email.read(cx).as_str().to_string();
        let password = self.auth.password.read(cx).as_str().to_string();
        match self.store.sign_in(&email, &password) {
            Ok(session) => {
                self.auth
                    .password
                    .update(cx, |state, cx| state.emplace("", cx));
                self.auth.session = Some(session);
                self.auth.account_exists = true;
                self.auth.login_error = None;
                self.notice = None;
                self.screen = Screen::Main;
                self.studio.model_info_open = self.prefs.shows_model_info();
            }
            Err(error) => self.auth.login_error = Some(auth_key(error)),
        }
        cx.notify();
    }

    fn sign_out(&mut self, cx: &mut Context<'_, Self>) {
        self.store.clear_session();
        self.auth
            .password
            .update(cx, |state, cx| state.emplace("", cx));
        self.auth.session = None;
        self.screen = Screen::Login;
        self.section = Section::Studio;
        self.settings.open = false;
        self.invite.open = false;
        self.studio.model_info_open = false;
        self.studio.model_menu_open = false;
        self.persona.menu = false;
        self.persona.dialog = more::Dialog::None;
        self.persona.status = None;
        self.studio.projects.clear();
        self.studio.tasks.clear();
        self.studio.thread = None;
        self.studio.stream = None;
        self.studio.stream_task = None;
        self.studio.confirm_rights = false;
        self.studio.confirm_terms = false;
        self.studio.document =
            flowy_core::DocumentSession::new(crate::brand::product());
        self.studio.agent.clear();
        self.notice = None;
        self.studio.credits_hover = false;
        self.studio.credits_close = None;
        self.gallery.page = 0;
        self.gallery.ideas_open = false;
        self.gallery.idea_id = None;
        self.gallery.idea_note = false;
        self.ota.reset();
        self.auth.login_error = None;
        cx.notify();
    }

    fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
        let text = self.studio.prompt.read(cx).as_str().trim().to_string();
        if text.is_empty() {
            self.notice = Some("notice.prompt_empty");
            cx.notify();
            return;
        }
        if self.section == Section::Agent {
            match self.studio.agent.send(&text) {
                Ok(()) => {
                    self.studio
                        .prompt
                        .update(cx, |state, cx| state.emplace("", cx));
                    self.notice = Some("notice.task_added");
                }
                Err(key) => self.notice = Some(key),
            }
            cx.notify();
            return;
        }
        let index = self.studio.document.document().nodes.len() as f32;
        self.studio.document.add_named_node(
            flowy_core::model::NodeKind::Action,
            text.clone(),
            flowy_core::geometry::Vec2::new(index * 40.0, 0.0),
        );
        self.studio
            .prompt
            .update(cx, |state, cx| state.emplace("", cx));
        self.open_thread(text, cx);
    }

    fn fill_prompt(&mut self, text: &str, cx: &mut Context<'_, Self>) {
        let text = text.to_string();
        self.open_thread(text, cx);
    }

    fn fill_prompt_key(
        &mut self,
        key: &'static str,
        cx: &mut Context<'_, Self>,
    ) {
        let text = self.t(key);
        self.fill_prompt(&text, cx);
    }

    fn add_project(&mut self, cx: &mut Context<'_, Self>) {
        let index = self.studio.projects.len().saturating_add(1);
        self.studio.projects.push(index);
        self.notice = Some("notice.project_created");
        cx.notify();
    }

    fn note(&mut self, key: &'static str, cx: &mut Context<'_, Self>) {
        self.notice = Some(key);
        cx.notify();
    }

    fn password_count(&self, cx: &App) -> usize {
        self.auth.password.read(cx).as_str().chars().count()
    }

    fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    fn tf(&self, key: &str, vars: &[(&str, &str)]) -> String {
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
                let pending = crate::shell::take_pending();
                if pending.is_empty() {
                    continue;
                }
                let updated = weak.update(cx, |app, cx| {
                    for url in pending {
                        match crate::shell::parse_deep_link(&url) {
                            Some(crate::shell::DeepLink::Invite { code }) => {
                                app.receive_invite_code(code, cx);
                            }
                            Some(crate::shell::DeepLink::Open) | None => {}
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

const fn auth_key(error: AuthError) -> &'static str {
    match error {
        AuthError::InvalidEmail => "auth.invalid_email",
        AuthError::ShortPassword => "auth.short_password",
        AuthError::UnknownAccount => "auth.unknown_account",
        AuthError::WrongPassword => "auth.wrong_password",
        AuthError::Store => "auth.store",
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
        let mut root = div().id("flowy").size_full().bg(crate::theme::canvas());
        root = if self.i18n.locale().is_rtl() {
            root.rtl()
        } else {
            root.ltr()
        };
        root = root.relative();
        root = if self.screen == Screen::Login {
            root.child(self.login_canvas(window, cx))
        } else if self.settings.open {
            root.child(self.settings_canvas(window, cx))
        } else {
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
        crate::theme::install(dark, self.prefs.accent, self.prefs.density);
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
    crate::shell::install(cx);
    let window = open_main_window(cx);
    crate::shell::bind_main_window(window);
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
    let store = Store::new(Store::default_root());
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
            crate::shell::apply_pending_deep_links(app, cx);
        })
        .ok();
    window
}

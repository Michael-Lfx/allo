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
mod skills;
mod state;
mod titlebar;

use crate::i18n::{I18n, file_stamps};
use crate::prefs::Preferences;
use crate::session::SessionStore;
use flowy_auth::{
    AuthClient, AuthError, EMAIL_OTP_COOLDOWN_SECS, EMAIL_OTP_LENGTH,
    normalize_otp,
};
use flowy_studio::library::{LibraryItem, LibraryPanel};
use flowy_studio::{StudioEvent, StudioPanel, StudioSurface};
use gpui::{
    App, Bounds, Context, Entity, KeyBinding, Subscription, Task, Window,
    WindowAppearance, WindowBounds, WindowOptions, actions, div, prelude::*,
    px, size,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
    actions::{DEFAULT_INPUT_CONTEXT, Tab, default_bindings},
};
use state::{
    AppearanceChrome, AuthState, FlexState, GalleryState, InviteState,
    LegalDoc, LoginPhase, LoginStep, PersonaState, SettingsState, SkillsState,
};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

actions!(
    flowy,
    [
        SignIn,
        FocusNext,
        FocusPrevious,
        ToggleDock,
        NavigateBack,
        NavigateForward,
        OpenSearch,
    ]
);

fn cloud_model_config(
    model: &flowy_agent::CatalogModel,
    token: &str,
) -> flowy_agent::LlmConfig {
    flowy_agent::LlmConfig {
        api_key: (!token.trim().is_empty()).then(|| token.to_owned()),
        base_url: model.endpoint.clone(),
        model: model.id.clone(),
        provider: "flowy".into(),
        context_window: model.context_window,
        max_tokens: model.max_tokens,
    }
}

#[cfg(test)]
mod cloud_config_tests {
    #[test]
    fn selected_cloud_model_sets_fixed_provider_and_account_credential() {
        let model = flowy_agent::CatalogModel {
            id: "selected-model".into(),
            endpoint: "https://example.com/claw/v1".into(),
            ..Default::default()
        };
        let config = super::cloud_model_config(&model, "account-token");
        assert_eq!(config.provider, "flowy");
        assert_eq!(config.model, "selected-model");
        assert_eq!(config.base_url, "https://example.com/claw/v1");
        assert_eq!(config.api_key.as_deref(), Some("account-token"));
        assert!(!super::cloud_model_config(&model, " ").uses_live());
    }
}

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

/// Refresh is single-flight: one request at a time, for a signed-in session
/// that is not running a turn.
const fn catalog_refresh_allowed(
    loading: bool,
    has_session: bool,
    busy: bool,
) -> bool {
    !loading && has_session && !busy
}

pub struct FlowyApp {
    active_dialog: state::ActiveDialog,
    pub ui: flowy_ui::UiContext,
    pub theme: flowy_ui::ResolvedTheme,
    pub store: SessionStore,
    pub screen: Screen,
    pub section: Section,
    pub dock_visible: bool,
    pub nav_back: Vec<Section>,
    pub nav_forward: Vec<Section>,
    pub notice: Option<&'static str>,
    storage_error: bool,
    auth: AuthState,
    pub studio: Entity<StudioPanel>,
    library: Entity<LibraryPanel>,
    gallery: GalleryState,
    skills: SkillsState,
    pub ota: ota::OtaController,
    pub(crate) settings: SettingsState,
    pub(crate) invite: InviteState,
    pub(crate) persona: PersonaState,
    pub(crate) appearance: AppearanceChrome,
    pub(crate) flex: FlexState,
    pub user_menu: bool,
    pub prefs: Preferences,
    pub i18n: I18n,
    catalog_task: Option<Task<()>>,
    catalog_generation: u64,
    catalog_loading: bool,
    locale_watch: Option<Task<()>>,
    shell_watch: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
}

impl std::fmt::Debug for FlowyApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowyApp")
            .field("screen", &self.screen)
            .field("section", &self.section)
            .field("active_dialog", &self.active_dialog)
            .finish_non_exhaustive()
    }
}

impl FlowyApp {
    fn activate_dialog(
        &mut self,
        dialog: state::ActiveDialog,
        cx: &mut Context<'_, Self>,
    ) {
        self.settings.picker = settings::Picker::None;
        self.flex.provider_menu = false;
        self.persona.menu = false;
        self.user_menu = false;
        self.gallery.idea_note = false;
        self.studio.update(cx, |panel, cx| {
            panel.model_menu_open = false;
            panel.credits_hover = false;
            panel.credits_close = None;
            cx.notify();
        });
        self.active_dialog = dialog;
        cx.notify();
    }

    fn close_overlay(&mut self, scope: &str, cx: &mut Context<'_, Self>) {
        match scope {
            "language-menu" | "send-menu" | "accent-menu" | "density-menu" => {
                self.settings.picker = settings::Picker::None;
            }
            "flex-provider-menu" => {
                self.flex.provider_menu = false;
            }
            "more-flyout" => {
                self.persona.menu = false;
            }
            "user-menu" => {
                self.close_user_menu(cx);
            }
            "model-list" | "model-info" | "credit-card" => {
                self.studio.update(cx, |panel, cx| {
                    match scope {
                        "model-list" => {
                            panel.model_menu_open = false;
                        }
                        "model-info" => {
                            panel.model_info_open = false;
                        }
                        _ => {
                            panel.credits_hover = false;
                        }
                    }
                    cx.notify();
                });
            }
            _ if self.active_dialog.scope() == Some(scope) => {
                self.active_dialog = state::ActiveDialog::None;
                self.settings.picker = settings::Picker::None;
                self.flex.provider_menu = false;
            }
            _ => {}
        }
        cx.notify();
    }

    fn new(
        store: SessionStore,
        system_dark: bool,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let session = store.load();
        let screen = if session.is_some() {
            Screen::Main
        } else {
            Screen::Login
        };
        let prefs = Preferences::load(store.root());
        let ui = flowy_ui::UiContext::new(
            Self::theme_options(&prefs),
            system_dark,
            cx,
        );
        let theme = ui.theme.read(cx).resolved();
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
        let skills_query = text_field(cx);
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
            // The cloud model catalog owns the live LLM config. Never fall back
            // to environment/BYOK configuration here.
            let llm = Self::disconnected_config();
            let ui = ui.clone();
            cx.new(move |cx| {
                StudioPanel::new(
                    flowy_studio::StudioConfig {
                        product,
                        i18n,
                        model,
                        plan,
                        model_info_version,
                        model_info_open,
                        store_root,
                        llm,
                    },
                    ui,
                    cx,
                )
            })
        };
        let library = {
            let items = library_items();
            let i18n = i18n.clone();
            let ui = ui.clone();
            cx.new(move |cx| LibraryPanel::new(items, i18n, ui, cx))
        };
        let subscriptions = vec![
            cx.subscribe(
                &ui.overlays,
                |app, _, event: &flowy_ui::OverlayEvent, cx| {
                    let flowy_ui::OverlayEvent::CloseRequested {
                        scope, ..
                    } = event;
                    app.close_overlay(scope.as_ref(), cx);
                },
            ),
            cx.observe(&studio, |_, _, cx| cx.notify()),
            cx.observe(&library, |_, _, cx| cx.notify()),
            cx.observe(&ui.overlays, |_, _, cx| cx.notify()),
            cx.observe(&ui.theme, |app, _, cx| {
                app.theme = app.ui.theme.read(cx).resolved();
                cx.notify();
            }),
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
            cx.subscribe(&skills_query, |_, _, _: &TextChanged, cx| {
                cx.notify();
            }),
            cx.subscribe(&studio, |app, _, event: &StudioEvent, cx| {
                app.on_studio_event(event, cx);
            }),
        ];
        let mut app = Self {
            active_dialog: state::ActiveDialog::None,
            ui,
            theme,
            store,
            screen,
            section: Section::Studio,
            dock_visible: true,
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            notice: None,
            storage_error: false,
            auth: AuthState {
                session,
                email,
                code,
                step: LoginStep::Email,
                phase: LoginPhase::Idle,
                message: None,
                pending_req_no: None,
                attempted_code: String::new(),
                cooldown_left: 0,
                generation: 0,
                task: None,
                cooldown_task: None,
            },
            studio,
            library,
            gallery: GalleryState {
                page: 0,
                idea_filter: gallery::Filter::All,
                idea_note: false,
            },
            skills: SkillsState {
                filter: skills::Category::All,
                query: skills_query,
            },
            ota: ota::OtaController::new(),
            settings: SettingsState {
                page: settings::Page::General,
                picker: settings::Picker::None,
                query: settings_query,
            },
            invite: InviteState {
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
                provider_menu: false,
                draft_provider: prefs.flex_provider,
                api_key: flex_api_key,
                secret: prefs.flex_api_key.clone(),
                key_visible: false,
            },
            user_menu: false,
            prefs,
            i18n,
            catalog_task: None,
            catalog_generation: 0,
            catalog_loading: false,
            locale_watch: None,
            shell_watch: None,
            subscriptions,
        };
        app.refresh_models(cx);
        app
    }

    fn disconnected_config() -> flowy_agent::LlmConfig {
        flowy_agent::LlmConfig {
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            provider: "flowy".into(),
            ..flowy_agent::LlmConfig::default()
        }
    }

    fn select_cloud_model(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        if self.catalog_loading {
            return;
        }
        let Some(session) = self.auth.session.as_ref() else {
            return;
        };
        if session.access_token.trim().is_empty() {
            return;
        }
        if self.studio.read(cx).agent.turn_status().is_busy() {
            return;
        }
        let Some(model) = self
            .studio
            .read(cx)
            .catalog
            .models
            .iter()
            .find(|m| m.id == id)
            .cloned()
        else {
            return;
        };
        let config = cloud_model_config(&model, &session.access_token);
        match self.studio.read(cx).agent.apply_config(config) {
            Ok(()) => {
                self.prefs.model_id.clone_from(&model.id);
                let save_failed =
                    self.prefs.save_checked(self.store.root()).is_err();
                self.studio.update(cx, |panel, cx| {
                    panel.catalog.select(model.id);
                    panel.cloud_ready = true;
                    panel.set_notice(
                        save_failed.then_some("notice.session_storage_failed"),
                        cx,
                    );
                });
            }
            Err(key) => {
                self.studio.update(cx, |panel, cx| {
                    panel.cloud_ready = false;
                    panel.set_notice(Some(key), cx);
                });
            }
        }
    }

    fn refresh_models(&mut self, cx: &mut Context<'_, Self>) {
        let busy = self.studio.read(cx).agent.turn_status().is_busy();
        if !catalog_refresh_allowed(
            self.catalog_loading,
            self.auth.session.is_some(),
            busy,
        ) {
            return;
        }
        let Some(session) = self.auth.session.clone() else {
            return;
        };
        let client = AuthClient::from_env();
        let account = session
            .user_id
            .map(|id| format!("{}:{id}", client.config().effective_base_url()));
        self.catalog_loading = true;
        let was_ready = self.studio.read(cx).cloud_ready;
        self.studio.update(cx, |panel, cx| {
            panel.cloud_ready = false;
            panel.catalog_loading = true;
            cx.notify();
        });
        let saved_id =
            if account.as_deref() == Some(self.prefs.model_account.as_str()) {
                self.prefs.model_id.clone()
            } else {
                String::new()
            };
        if account.is_some()
            && account.as_deref() != Some(self.prefs.model_account.as_str())
        {
            self.studio.update(cx, |panel, _| {
                panel.cloud_ready = false;
                panel.catalog = flowy_agent::ProviderCatalog::new();
            });
        }
        self.catalog_generation = self.catalog_generation.wrapping_add(1);
        let generation = self.catalog_generation;
        let token = session.access_token;
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.available_models(&token);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.catalog_task = Some(cx.spawn(async move |weak, cx| {
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
                    if generation != app.catalog_generation
                        || app.auth.session.is_none()
                    {
                        return;
                    }
                    app.catalog_loading = false;
                    app.studio.update(cx, |panel, _| {
                        panel.catalog_loading = false;
                    });
                    match result {
                        Ok(result) => {
                            let models = result
                                .cloud
                                .into_iter()
                                .map(|m| flowy_agent::CatalogModel {
                                    id: m.id,
                                    name: m.name,
                                    endpoint: m.endpoint,
                                    context_window: m.context_window,
                                    max_tokens: m.max_tokens,
                                    credit_rate: m.credit_rate,
                                })
                                .collect();
                            app.studio.update(cx, |panel, cx| {
                                panel.cloud_ready = false;
                                panel.catalog.replace(models);
                                if !saved_id.is_empty() {
                                    panel.catalog.select(saved_id);
                                }
                                cx.notify();
                            });
                            app.prefs.model_account =
                                account.unwrap_or_default();
                            let selected =
                                app.studio.read(cx).catalog.selected.clone();
                            if let Some(id) = selected {
                                app.select_cloud_model(&id, cx);
                            } else {
                                app.prefs.model_id.clear();
                                app.prefs.save(app.store.root());
                                let _ =
                                    app.studio.read(cx).agent.apply_config(
                                        Self::disconnected_config(),
                                    );
                                app.note("notice.model_unavailable", cx);
                            }
                        }
                        Err(AuthError::AuthRequired) => app.sign_out(cx),
                        Err(error) => {
                            app.studio.update(cx, |panel, _| {
                                panel.cloud_ready = was_ready;
                            });
                            app.note(error.key(), cx);
                        }
                    }
                });
                break;
            }
        }));
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
        self.auth.attempted_code.clear();
        self.auth.code.update(cx, |state, cx| state.emplace("", cx));
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
        self.auth.attempted_code.clone_from(&code);
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
                            app.auth.attempted_code.clear();
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
                            app.refresh_models(cx);
                            let open = false;
                            app.studio.update(cx, |panel, cx| {
                                panel.model_info_open = open;
                                cx.notify();
                            });
                        }
                        Err(AuthError::InvalidCode) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.attempted_code.clear();
                            app.auth
                                .code
                                .update(cx, |state, cx| state.emplace("", cx));
                            app.auth.message =
                                Some(AuthError::InvalidCode.key());
                        }
                        Err(AuthError::SessionExpired) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.pending_req_no = None;
                            app.auth.attempted_code.clear();
                            app.auth
                                .code
                                .update(cx, |state, cx| state.emplace("", cx));
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
            && normalized != self.auth.attempted_code
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
        self.auth.attempted_code.clear();
        self.auth.cooldown_left = 0;
        self.auth.task = None;
        self.auth.cooldown_task = None;
        self.auth.code.update(cx, |state, cx| state.emplace("", cx));
        cx.notify();
    }

    fn open_legal(&mut self, doc: LegalDoc, cx: &mut Context<'_, Self>) {
        self.activate_dialog(state::ActiveDialog::Legal(doc), cx);
        cx.notify();
    }

    fn close_legal(&mut self, cx: &mut Context<'_, Self>) {
        self.active_dialog = state::ActiveDialog::None;
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

    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI Context is forwarded to entity updates and task spawning"
    )]
    fn start_login_cooldown(&mut self, cx: &mut Context<'_, Self>) {
        self.auth.cooldown_left = EMAIL_OTP_COOLDOWN_SECS;
        self.auth.cooldown_task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
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

    pub(super) fn sign_out(&mut self, cx: &mut Context<'_, Self>) {
        self.catalog_generation = self.catalog_generation.wrapping_add(1);
        self.catalog_task = None;
        self.catalog_loading = false;
        self.studio.update(cx, |panel, cx| {
            panel.cloud_ready = false;
            panel.catalog_loading = false;
            panel.catalog = flowy_agent::ProviderCatalog::new();
            panel.agent.stop();
            let _ = panel.agent.apply_config(Self::disconnected_config());
            cx.notify();
        });
        self.store.clear();
        self.auth
            .email
            .update(cx, |state, cx| state.emplace("", cx));
        self.auth.code.update(cx, |state, cx| state.emplace("", cx));
        self.auth.session = None;
        self.auth.step = LoginStep::Email;
        self.auth.phase = LoginPhase::Idle;
        self.auth.message = None;
        self.auth.pending_req_no = None;
        self.auth.attempted_code.clear();
        self.auth.cooldown_left = 0;
        self.auth.task = None;
        self.auth.cooldown_task = None;
        self.active_dialog = state::ActiveDialog::None;
        self.screen = Screen::Login;
        self.section = Section::Studio;
        self.dock_visible = true;
        self.nav_back.clear();
        self.nav_forward.clear();
        self.persona.menu = false;
        self.persona.status = None;
        self.user_menu = false;
        self.notice = None;
        self.studio
            .update(cx, flowy_studio::StudioPanel::reset_session);
        self.gallery.page = 0;
        self.gallery.idea_note = false;
        self.skills.filter = skills::Category::All;
        self.ota.reset();
        cx.notify();
    }

    pub fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
        self.sync_studio_surface(cx);
        self.studio
            .update(cx, flowy_studio::StudioPanel::send_prompt);
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
        self.studio
            .update(cx, flowy_studio::StudioPanel::add_project);
    }

    pub fn open_thread(&mut self, title: String, cx: &mut Context<'_, Self>) {
        self.push_section(Section::Studio, cx);
        self.studio
            .update(cx, |panel, cx| panel.open_thread(title, cx));
    }

    pub fn clear_thread(&mut self, cx: &mut Context<'_, Self>) {
        self.studio
            .update(cx, flowy_studio::StudioPanel::clear_thread);
    }

    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI Context is forwarded to entity updates"
    )]
    fn sync_studio_surface(&mut self, cx: &mut Context<'_, Self>) {
        let notice = self.notice;
        match self.section {
            Section::Studio => {
                self.studio.update(cx, |panel, cx| {
                    // Enter the currently selected session when returning from
                    // another section. Explicit new-task/session actions still
                    // choose Home or Chat themselves.
                    let surface = if panel.agent.has_messages() {
                        StudioSurface::Chat
                    } else {
                        StudioSurface::Home
                    };
                    panel.set_surface(surface, cx);
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
                    panel.model_menu_open = false;
                    panel.model_info_open = false;
                    panel.credits_hover = false;
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
                // Events are delivered after the current action. Reconcile the
                // panel again so a navigation action cannot restore stale text
                // between emitting this event and handling it here.
                if *notice == Some("notice.session_storage_failed") {
                    self.storage_error = true;
                    self.notice = None;
                } else {
                    self.notice = *notice;
                }
                let notice = self.notice;
                self.studio
                    .update(cx, |panel, cx| panel.set_notice(notice, cx));
                cx.notify();
            }
            StudioEvent::ModelChanged {
                model,
                plan,
                model_info_version,
            } => {
                self.prefs.model = *model;
                self.prefs.plan = *plan;
                self.prefs.model_info_version.clone_from(model_info_version);
                self.prefs.save(self.store.root());
                // Legacy model-info events must not restore BYOK/environment configuration.
                cx.notify();
            }
            StudioEvent::OpenFlex => {
                self.open_flex(cx);
            }
            StudioEvent::CloudModelSelected(id) => {
                self.select_cloud_model(id, cx);
            }
            StudioEvent::RefreshModels => self.refresh_models(cx),
            StudioEvent::OpenIdeas => {
                self.activate_dialog(state::ActiveDialog::Ideas, cx);
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
            .bg(self.theme.canvas())
            .text_color(self.theme.ink());
        if self.dock_visible {
            canvas = canvas.child(self.dock(window, cx));
        }
        canvas = if self.section == Section::More {
            canvas.child(self.more_canvas(cx))
        } else if self.section == Section::Library {
            canvas.child(self.library.clone())
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

    fn dismiss_storage_error(&mut self, cx: &mut Context<'_, Self>) {
        self.storage_error = false;
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
                    let i18n = app.i18n.clone();
                    app.library.update(cx, |panel, cx| {
                        panel.set_i18n(i18n, cx);
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
        let modal = self
            .active_dialog
            .scope()
            .map(gpui::SharedString::from)
            .or_else(|| {
                (self.screen == Screen::Main
                    && self.section == Section::Library)
                    .then(|| self.library.read(cx).modal_scope())
                    .flatten()
            });
        let popup = match self.settings.picker {
            settings::Picker::Language => Some("language-menu"),
            settings::Picker::Send => Some("send-menu"),
            settings::Picker::Accent => Some("accent-menu"),
            settings::Picker::Density => Some("density-menu"),
            settings::Picker::None if self.flex.provider_menu => {
                Some("flex-provider-menu")
            }
            settings::Picker::None if self.persona.menu => Some("more-flyout"),
            settings::Picker::None if self.user_menu => Some("user-menu"),
            settings::Picker::None
                if self.active_dialog == state::ActiveDialog::None
                    && self.screen == Screen::Main
                    && self.section != Section::More
                    && self.section != Section::Library
                    && self.studio.read(cx).surface
                        != StudioSurface::Automation
                    && self.studio.read(cx).model_menu_open =>
            {
                Some("model-list")
            }
            settings::Picker::None
                if self.active_dialog == state::ActiveDialog::None
                    && self.screen == Screen::Main
                    && self.section != Section::More
                    && self.section != Section::Library
                    && self.studio.read(cx).surface
                        != StudioSurface::Automation
                    && self.studio.read(cx).model_info_open =>
            {
                Some("model-info")
            }
            settings::Picker::None
                if self.active_dialog == state::ActiveDialog::None
                    && self.screen == Screen::Main
                    && self.section != Section::More
                    && self.section != Section::Library
                    && self.studio.read(cx).surface
                        != StudioSurface::Automation
                    && self.studio.read(cx).credits_hover =>
            {
                Some("credit-card")
            }
            settings::Picker::None => None,
        }
        .map(gpui::SharedString::from)
        .or_else(|| {
            (self.screen == Screen::Main && self.section == Section::Library)
                .then(|| self.library.read(cx).popup_scope())
                .flatten()
        });
        self.ui.overlays.update(cx, |host, cx| {
            host.begin_frame(window);
            host.set_modal(modal, window, cx);
            host.set_popup(popup, window, cx);
        });
        window.set_rem_size(px(f32::from(self.prefs.font_size) / 0.875));
        if self.screen == Screen::Main && !self.ota.checked {
            self.ota.checked = true;
            self.start_update_check(cx);
        }
        let mut root = div()
            .id("flowy")
            .size_full()
            .flex()
            .flex_col()
            .bg(self.theme.canvas())
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
            // Single-line inputs propagate their Tab action to this handler;
            // multiline inputs retain their normal tab insertion behavior.
            .on_action(|_: &Tab, window, cx| window.focus_next(cx))
            .on_action(cx.listener(|app, _: &ToggleDock, _, cx| {
                app.toggle_dock(cx);
            }))
            .on_action(cx.listener(|app, _: &NavigateBack, _, cx| {
                app.navigate_back(cx);
            }))
            .on_action(cx.listener(|app, _: &NavigateForward, _, cx| {
                app.navigate_forward(cx);
            }))
            .on_action(cx.listener(|app, _: &OpenSearch, _, cx| {
                app.open_search(cx);
            }));
        root = if self.i18n.locale().is_rtl() {
            root.rtl()
        } else {
            root.ltr()
        };
        root = root.relative().child(self.titlebar(window, cx));
        let modal = self.active_dialog != state::ActiveDialog::None;
        let background = if self.screen == Screen::Login {
            div()
                .id("login-root")
                .flex_1()
                .min_h_0()
                .w_full()
                .relative()
                .child(self.login_canvas(window, cx))
        } else {
            div()
                .id("main-background")
                .flex_1()
                .min_h_0()
                .w_full()
                .child(self.main_canvas(window, cx))
        };
        root = root.child(
            background
                .when(modal, gpui::StatefulInteractiveElement::aria_hidden),
        );
        root = match self.active_dialog {
            state::ActiveDialog::None => root,
            state::ActiveDialog::Settings => {
                root.child(self.settings_layer(window, cx))
            }
            state::ActiveDialog::Ideas => {
                root.child(self.ideas_layer(window, cx))
            }
            state::ActiveDialog::Idea(_) => {
                root.child(self.idea_layer(window, cx))
            }
            state::ActiveDialog::Skill(_) => {
                root.child(self.skill_layer(window, cx))
            }
            state::ActiveDialog::Invite => root.child(self.invite_layer(cx)),
            state::ActiveDialog::Flex => root.child(self.flex_layer(cx)),
            state::ActiveDialog::Import | state::ActiveDialog::Knowledge => {
                root.child(self.more_layer(cx))
            }
            state::ActiveDialog::Legal(_) => root.child(self.legal_layer(cx)),
        };
        let menu = match self.settings.picker {
            settings::Picker::Language => Some((
                "language-menu",
                "language-button",
                self.language_menu(cx).into_any_element(),
            )),
            settings::Picker::Send => Some((
                "send-menu",
                "send-button",
                self.send_menu(cx).into_any_element(),
            )),
            settings::Picker::Accent => Some((
                "accent-menu",
                "accent-button",
                self.accent_menu(cx).into_any_element(),
            )),
            settings::Picker::Density => Some((
                "density-menu",
                "density-button",
                self.density_menu(cx).into_any_element(),
            )),
            settings::Picker::None if self.flex.provider_menu => Some((
                "flex-provider-menu",
                "flex-provider",
                self.flex_provider_menu(cx).into_any_element(),
            )),
            settings::Picker::None if self.persona.menu => Some((
                "more-flyout",
                "more-trigger",
                self.more_flyout(cx).into_any_element(),
            )),
            settings::Picker::None if self.user_menu => Some((
                "user-menu",
                "dock-user",
                self.user_menu_content(cx).into_any_element(),
            )),
            settings::Picker::None => None,
        };
        if let Some((id, anchor, content)) = menu {
            root = root.child(flowy_ui::popover(
                id,
                anchor,
                content,
                &self.ui,
                window,
                cx,
                self.i18n.locale().is_rtl(),
            ));
        }
        if self.prefs.activity && self.screen == Screen::Main {
            root = root.child(self.activity_floater());
        }
        if self.storage_error && self.screen == Screen::Main {
            let dismiss = self.t("model.got_it");
            root = root.child(
                div()
                    .id("session-storage-error")
                    .absolute()
                    .top(px(16.))
                    .right(px(16.))
                    .max_w(px(480.))
                    .p_3()
                    .rounded_lg()
                    .bg(self.theme.card())
                    .border_1()
                    .border_color(self.theme.danger())
                    .text_color(self.theme.ink())
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(self.t("notice.session_storage_failed"))
                    .child(
                        flowy_ui::button(
                            "dismiss-session-storage-error",
                            dismiss.clone(),
                            false,
                            &self.theme,
                            cx.listener(|app, _, _, cx| {
                                app.dismiss_storage_error(cx);
                            }),
                        )
                        .px_3()
                        .py_1()
                        .rounded_lg()
                        .bg(self.theme.fill())
                        .text_color(self.theme.on_fill())
                        .child(dismiss),
                    ),
            );
        }
        root
    }
}

impl FlowyApp {
    const fn theme_options(prefs: &Preferences) -> flowy_ui::ThemeOptions {
        flowy_ui::ThemeOptions {
            mode: prefs.theme,
            accent: prefs.accent,
            density: prefs.density,
            font_size: prefs.font_size,
        }
    }

    fn apply_appearance(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
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
        let options = Self::theme_options(&self.prefs);
        self.ui
            .theme
            .update(cx, |state, cx| state.update(options, dark, cx));
        self.theme = self.ui.theme.read(cx).resolved();
        window.set_rem_size(px(f32::from(self.prefs.font_size) / 0.875));
    }
}

fn text_field(cx: &mut Context<'_, FlowyApp>) -> Entity<EditableTextState> {
    cx.new(|cx| EditableTextState::new(StringStorage::default(), cx))
}

/// An explicit host data source; examples supply their own isolated fixture.
/// The normal application starts with an empty library until a source is given.
fn library_items() -> Vec<LibraryItem> {
    let Some(path) = std::env::var_os("FLOWY_LIBRARY_DATA") else {
        return Vec::new();
    };
    let result = std::fs::read(path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())
        });
    match result {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!(%error, "could not load library data source");
            Vec::new()
        }
    }
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
    cx.bind_keys([
        KeyBinding::new("enter", SignIn, Some("Login")),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
        // Cursor / VS Code: toggle primary side bar
        KeyBinding::new("secondary-b", ToggleDock, None),
        // Cursor / VS Code: navigate back / forward
        KeyBinding::new("ctrl--", NavigateBack, None),
        KeyBinding::new("alt-left", NavigateBack, None),
        KeyBinding::new("secondary-[", NavigateBack, None),
        KeyBinding::new("ctrl-shift--", NavigateForward, None),
        KeyBinding::new("alt-right", NavigateForward, None),
        KeyBinding::new("secondary-]", NavigateForward, None),
        // Cursor / VS Code: search view
        KeyBinding::new("secondary-shift-f", OpenSearch, None),
    ]);
    cx.set_reduce_motion(true);
    flowy_ui::install_initial_tab_focus(cx).detach();
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
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.), px(12.))),
        }),
        window_min_size: Some(size(px(960.), px(640.))),
        ..WindowOptions::default()
    };
    let store = SessionStore::new(crate::session::default_root());
    let window = cx
        .open_window(options, |window, cx| {
            let dark = matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            );
            cx.new(|cx| FlowyApp::new(store, dark, cx))
        })
        .expect("open main window");
    window
        .update(cx, |app, window, cx| {
            window.on_window_should_close(cx, |window, _cx| {
                window.set_visible(false);
                false
            });
            app.apply_appearance(window, cx);
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
                window.set_visible(true);
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_root() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "flowy-app-notice-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    panic!("could not create test directory: {error}")
                }
            }
        }
    }

    #[test]
    fn catalog_refresh_gate_is_single_flight_for_idle_sessions() {
        assert!(catalog_refresh_allowed(false, true, false));
        assert!(!catalog_refresh_allowed(true, true, false));
        assert!(!catalog_refresh_allowed(false, false, false));
        assert!(!catalog_refresh_allowed(false, true, true));
    }

    #[gpui::test]
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test harness passes &mut TestAppContext"
    )]
    fn new_task_reconciles_deferred_studio_notice(cx: &mut TestAppContext) {
        let root = test_root();
        let app = cx.update(|cx| {
            cx.new(|cx| {
                FlowyApp::new(SessionStore::new(root.clone()), false, cx)
            })
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.studio.update(cx, |panel, cx| {
                    panel.note("notice.attach_later", cx);
                });
            });
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.notice, Some("notice.attach_later"));
                app.select_navigation("new-task", Section::Studio, cx);
            });
        });
        cx.update(|cx| {
            let app = app.read(cx);
            assert_eq!(app.notice, None);
            assert_eq!(app.studio.read(cx).notice, None);
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test harness passes &mut TestAppContext"
    )]
    fn storage_error_survives_login_settings_and_navigation_until_dismissed(
        cx: &mut TestAppContext,
    ) {
        let root = test_root();
        let app = cx.update(|cx| {
            cx.new(|cx| {
                FlowyApp::new(SessionStore::new(root.clone()), false, cx)
            })
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.screen, Screen::Login);
                app.studio.update(cx, |panel, cx| {
                    panel.note("notice.session_storage_failed", cx);
                });
            });
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                assert!(app.storage_error);
                app.screen = Screen::Main;
                app.activate_dialog(state::ActiveDialog::Settings, cx);
                app.select_navigation("automation", Section::Automation, cx);
                app.select_navigation("new-task", Section::Studio, cx);
            });
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                assert!(app.storage_error);
                app.dismiss_storage_error(cx);
                assert!(!app.storage_error);
            });
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test harness passes &mut TestAppContext"
    )]
    fn failed_new_task_is_not_hidden_by_navigation(cx: &mut TestAppContext) {
        let root = test_root();
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        // A broken index must be quarantined and rebuilt from structured history.
        std::fs::write(sessions.join("index.json"), b"{broken").unwrap();
        std::fs::write(
            sessions.join("history.json"),
            br#"{
                "active": "hist-1",
                "sessions": [
                    {
                        "id": "hist-1",
                        "title": "Recovered",
                        "titled": true,
                        "updated_at": 1,
                        "status": { "kind": "idle" },
                        "messages": [],
                        "todos": []
                    }
                ]
            }"#,
        )
        .unwrap();
        let app = cx.update(|cx| {
            cx.new(|cx| {
                FlowyApp::new(SessionStore::new(root.clone()), false, cx)
            })
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.select_navigation("new-task", Section::Studio, cx);
            });
        });
        // The storage error is delivered by the 33ms Studio pump; advance the
        // test executor until it lands instead of assuming one fixed tick.
        let mut delivered = false;
        for _ in 0..8 {
            cx.run_until_parked();
            cx.background_executor
                .advance_clock(Duration::from_millis(33));
            cx.run_until_parked();
            delivered = cx.update(|cx| app.read(cx).storage_error);
            if delivered {
                break;
            }
        }
        assert!(delivered, "storage error should reach the app via the pump");
        // Navigation after the notice arrived must not clear it.
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.select_navigation("automation", Section::Automation, cx);
            });
        });
        assert!(cx.update(|cx| app.read(cx).storage_error));
        // The damaged original is kept under a corrupt-* name.
        let quarantined: Vec<_> = std::fs::read_dir(&sessions)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("index.corrupt-")
            })
            .collect();
        assert_eq!(quarantined.len(), 1, "broken index should be quarantined");
        assert_eq!(std::fs::read(quarantined[0].path()).unwrap(), b"{broken");
        // A fresh index is rebuilt and keeps the session recovered from history.
        let rebuilt: serde_json::Value = serde_json::from_slice(
            &std::fs::read(sessions.join("index.json")).unwrap(),
        )
        .expect("rebuilt index should be valid JSON");
        let ids: Vec<&str> = rebuilt["sessions"]
            .as_array()
            .expect("rebuilt index has sessions")
            .iter()
            .filter_map(|session| session["id"].as_str())
            .collect();
        assert!(
            ids.contains(&"hist-1"),
            "history session should survive the rebuild: {ids:?}"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}

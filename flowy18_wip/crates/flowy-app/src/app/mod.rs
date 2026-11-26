#![allow(clippy::derive_partial_eq_without_eq)]

mod appearance;
mod artifact_cleanup;
mod artifact_preview;
#[cfg(test)]
mod artifact_tests;
mod conversation_actions;
mod dock;
mod flex;
mod gallery;
mod invite;
mod library_actions;
mod login;
mod more;
mod ota;
mod projects;
mod quick_search;
mod settings;
mod skills;
mod skills_jobs;
mod state;
mod titlebar;

use crate::i18n::{I18n, file_stamps};
use crate::prefs::Preferences;
use crate::session::SessionStore;
use flowy_auth::{
    AuthClient, AuthError, EMAIL_OTP_COOLDOWN_SECS, EMAIL_OTP_LENGTH,
    normalize_otp,
};
use flowy_studio::library::{LibraryEvent, LibraryPanel};
use flowy_studio::{StudioEvent, StudioPanel, StudioSurface};
use gpui::{
    App, Bounds, Context, Entity, KeyBinding, SharedString, Subscription, Task,
    Window, WindowAppearance, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, size,
};
use gpui_kit::base::input::{
    Enter, InputBaseState, InputEvent, InputModeKind, InputState, TextareaState,
};
use state::{
    AppearanceChrome, AuthState, FlexState, GalleryState, InviteState,
    LegalDoc, LoginPhase, LoginStep, PersonaState, SettingsState, SkillsState,
};
use std::collections::HashSet;
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
        InsertIndent,
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

fn sanitize_notify_title(raw: &str) -> Option<String> {
    let collapsed = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned();
    if collapsed.is_empty() {
        return None;
    }
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(60).collect();
    Some(if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    })
}

fn turn_finished_notification_payload(
    i18n: &I18n,
    needs_attention: bool,
    has_error: bool,
    still_busy_in_background: bool,
    session_title: Option<&str>,
) -> Option<(&'static str, String, String)> {
    if needs_attention {
        return Some((
            "task-attention",
            i18n.t("shell.notify_task_attention_title"),
            i18n.t("shell.notify_task_attention_body"),
        ));
    }
    if has_error {
        return Some((
            "task-error",
            i18n.t("shell.notify_task_error_title"),
            i18n.t("shell.notify_task_error_body"),
        ));
    }
    if still_busy_in_background {
        return None;
    }
    let body = match session_title.and_then(sanitize_notify_title) {
        Some(title) => {
            i18n.tf("shell.notify_task_done_named_body", &[("title", &title)])
        }
        None => i18n.t("shell.notify_task_done_body"),
    };
    Some(("task-done", i18n.t("shell.notify_task_done_title"), body))
}

#[cfg(test)]
mod notify_tests {
    use super::{
        I18n, sanitize_notify_title, turn_finished_notification_payload,
    };
    use crate::i18n::Locale;

    #[test]
    fn sanitize_notify_title_collapses_whitespace_and_truncates() {
        assert_eq!(sanitize_notify_title("  \n\t "), None);
        assert_eq!(
            sanitize_notify_title("hello\n  world "),
            Some("hello world".to_owned())
        );
        let long = "a".repeat(80);
        let clipped = sanitize_notify_title(&long).unwrap();
        assert_eq!(clipped.chars().count(), 61);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn turn_notification_payload_prioritizes_attention_error_busy_and_done() {
        let mut i18n = I18n::embedded("Flowy");
        i18n.set_locale(Locale::ZhCn);
        let (tag, title, _) = turn_finished_notification_payload(
            &i18n,
            true,
            true,
            true,
            Some("天气"),
        )
        .unwrap();
        assert_eq!(tag, "task-attention");
        assert_eq!(title, "任务需要你确认");

        let (tag, title, _) = turn_finished_notification_payload(
            &i18n,
            false,
            true,
            true,
            Some("天气"),
        )
        .unwrap();
        assert_eq!(tag, "task-error");
        assert_eq!(title, "任务遇到中断");

        assert!(
            turn_finished_notification_payload(
                &i18n,
                false,
                false,
                true,
                Some("天气"),
            )
            .is_none()
        );

        let (tag, title, body) = turn_finished_notification_payload(
            &i18n,
            false,
            false,
            false,
            Some("深圳天气"),
        )
        .unwrap();
        assert_eq!(tag, "task-done");
        assert_eq!(title, "任务已完成");
        assert!(body.contains("深圳天气"));
        assert!(body.contains("Flowy"));

        i18n.set_locale(Locale::En);
        let (tag, title, body) = turn_finished_notification_payload(
            &i18n,
            false,
            false,
            false,
            Some("Weather"),
        )
        .unwrap();
        assert_eq!(tag, "task-done");
        assert_eq!(title, "Task complete");
        assert!(body.contains("Weather"));
        assert!(body.contains("Flowy"));
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
    Board,
    // Computer,
    // Agent,
    Library,
    Create,
    Automation,
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

/// One field's programmatic text, waiting for the window that `set_value` needs.
///
/// Single-line and multi-line fields carry distinct editing states in the kit,
/// so the queue keeps both shapes rather than duplicating the drain loop.
enum PendingText {
    Single(Entity<InputState>, SharedString),
    Multi(Entity<TextareaState>, SharedString),
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
    library_generation: u64,
    artifact_navigation_generation: u64,
    artifact_preview: artifact_preview::ArtifactPreviewState,
    artifact_cleanup: artifact_cleanup::ArtifactCleanupState,
    task_menu: Option<conversation_actions::TaskMenuState>,
    archive_undo: conversation_actions::ArchiveUndoController,
    conversation_delete: conversation_actions::ConversationDeleteState,
    conversation_rename: conversation_actions::ConversationRenameState,
    quick_search: quick_search::QuickSearchState,
    gallery: GalleryState,
    skills: SkillsState,
    pub ota: ota::OtaController,
    pub(crate) settings: SettingsState,
    pub(crate) invite: InviteState,
    pub(crate) project_create: state::ProjectCreateState,
    pub(crate) project_menu: Option<String>,
    pub(crate) expanded_projects: HashSet<String>,
    pub(crate) projects_open: bool,
    pub(crate) tasks_open: bool,
    pub(crate) persona: PersonaState,
    pub(crate) appearance: AppearanceChrome,
    pub(crate) flex: FlexState,
    pub user_menu: bool,
    pub prefs: Preferences,
    pub i18n: I18n,
    catalog_task: Option<Task<()>>,
    catalog_generation: u64,
    catalog_loading: bool,
    /// Highest unexpired plan tier from the cloud catalog (`effectiveModelTier`).
    effective_tier: u8,
    applied_tier: Option<flowy_agent::Model>,
    credits_task: Option<Task<()>>,
    credits_generation: u64,
    credits_poll: Option<Task<()>>,
    locale_watch: Option<Task<()>>,
    shell_watch: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
    /// Programmatic input text waiting for a window; see [`FlowyApp::set_input_text`].
    pending_input_text: std::cell::RefCell<Vec<PendingText>>,
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
        self.user_menu = false;
        self.task_menu = None;
        self.gallery.idea_note = false;
        self.studio.update(cx, |panel, cx| {
            panel.close_all_popovers();
            panel.close_flow_modal(cx);
        });
        self.active_dialog = dialog;
        cx.notify();
    }

    fn close_overlay(&mut self, scope: &str, cx: &mut Context<'_, Self>) {
        if scope == "artifact-cleanup" && self.artifact_cleanup.busy {
            return;
        }
        match scope {
            "language-menu" | "send-menu" | "accent-menu" | "density-menu" => {
                self.settings.picker = settings::Picker::None;
            }
            "flex-provider-menu" => {
                self.flex.provider_menu = false;
            }
            "user-menu" => {
                self.close_user_menu(cx);
            }
            "project-menu" => {
                self.project_menu = None;
            }
            "task-menu" => {
                self.task_menu = None;
            }
            "skill-unlock" => {
                if let state::ActiveDialog::SkillUnlock(id) = self.active_dialog
                {
                    self.active_dialog = state::ActiveDialog::Skill(id);
                }
            }
            "skill-docs" => {
                if let state::ActiveDialog::SkillDocs(id) = self.active_dialog {
                    self.active_dialog = state::ActiveDialog::Skill(id);
                }
            }
            _ if self.active_dialog.scope() == Some(scope) => {
                self.active_dialog = state::ActiveDialog::None;
                self.settings.picker = settings::Picker::None;
                self.flex.provider_menu = false;
                self.conversation_rename.id = None;
                self.conversation_rename.error = None;
            }
            _ => {}
        }
        cx.notify();
    }

    fn new(
        store: SessionStore,
        system_dark: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let app_window_id = window.window_handle().window_id();
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::new",
            system_dark,
            "enter"
        );
        let session = store.load();
        let screen = if session.is_some() {
            Screen::Main
        } else {
            Screen::Login
        };
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::new::SessionStore::load",
            signed_in = session.is_some(),
            ?screen,
            "session"
        );
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
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::new",
            locale = %prefs.locale.code(),
            "ui + i18n ready"
        );
        let email = input_field(window, cx);
        let code = input_field(window, cx);
        let settings_query = input_field(window, cx);
        let invite_email = input_field(window, cx);
        let project_create = state::ProjectCreateState {
            name: input_field(window, cx),
            folder: None,
            status: None,
            title_key: "project.create_title",
            picker_generation: 0,
            editing_project_id: None,
            deleting_project_id: None,
        };
        let invite_book = invite::load(store.root());
        let saved = crate::persona::load(store.root());
        let nickname = input_field(window, cx);
        let occupation = input_field(window, cx);
        let about = textarea_field(window, cx);
        let instructions = textarea_field(window, cx);
        let import_paste = textarea_field(window, cx);
        let knowledge_name = input_field(window, cx);
        let knowledge_usage = input_field(window, cx);
        let knowledge_body = textarea_field(window, cx);
        let font_size_field = input_field(window, cx);
        let flex_api_key = input_field(window, cx);
        let skills_query = input_field(window, cx);
        let quick_search_query = input_field(window, cx);
        let conversation_rename_input = input_field(window, cx);
        place(window, &nickname, &saved.nickname, cx);
        place(window, &occupation, &saved.occupation, cx);
        place(window, &about, &saved.about, cx);
        place(window, &instructions, &saved.instructions, cx);
        place(window, &font_size_field, &prefs.font_size.to_string(), cx);
        place(window, &flex_api_key, &prefs.flex_api_key, cx);
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::new",
            "→ StudioPanel::new"
        );
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
                let config = flowy_studio::StudioConfig {
                    product,
                    i18n,
                    model,
                    plan,
                    model_info_version,
                    model_info_open,
                    store_root,
                    llm,
                };
                if cfg!(test) {
                    // The deterministic GPUI harness uses the synchronous
                    // constructor; production restores storage in the background.
                    StudioPanel::new(config, ui, window, cx)
                } else {
                    StudioPanel::new_background(config, ui, window, cx)
                }
            })
        };
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::new",
            "→ LibraryPanel::new"
        );
        let library = {
            let i18n = i18n.clone();
            let ui = ui.clone();
            cx.new(move |cx| {
                let mut panel = LibraryPanel::new(Vec::new(), i18n, ui, cx);
                panel.set_loading(true, cx);
                panel
            })
        };
        let subscriptions = vec![
            cx.on_app_quit(|app, cx| {
                // Run bounded graceful shutdown synchronously inside `on_app_quit`
                // so GPUI's 200ms async `SHUTDOWN_TIMEOUT` does not abort
                // `persist_index()` or finalizing delivery commits mid-flight.
                let agent = app.studio.read(cx).agent.clone();
                agent
                    .prepare_shutdown_with_timeout(Duration::from_millis(1200));
                async {}
            }),
            cx.subscribe(&library, |app, _, event: &LibraryEvent, cx| {
                app.library_event(event, cx);
            }),
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
            cx.subscribe(&email, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&code, |app, _, _: &InputEvent, cx| {
                app.on_otp_changed(cx);
            }),
            cx.subscribe(&settings_query, |_, _, _: &InputEvent, cx| {
                cx.notify();
            }),
            cx.subscribe(&flex_api_key, |app, _, _: &InputEvent, cx| {
                app.touch_flex_key(cx);
            }),
            cx.subscribe(&invite_email, |_, _, _: &InputEvent, cx| {
                cx.notify();
            }),
            cx.subscribe(&project_create.name, |app, _, _: &InputEvent, cx| {
                app.project_create.status = None;
                cx.notify();
            }),
            cx.subscribe(&nickname, |app, _, _: &InputEvent, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&occupation, |app, _, _: &InputEvent, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&about, |app, _, _: &InputEvent, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&instructions, |app, _, _: &InputEvent, cx| {
                app.touch_profile(cx);
            }),
            cx.subscribe(&import_paste, |app, _, _: &InputEvent, cx| {
                app.touch_paste(cx);
            }),
            cx.subscribe(&knowledge_name, |app, _, _: &InputEvent, cx| {
                app.touch_knowledge_name(cx);
            }),
            cx.subscribe(&knowledge_usage, |app, _, _: &InputEvent, cx| {
                app.touch_knowledge_usage(cx);
            }),
            cx.subscribe(&knowledge_body, |app, _, _: &InputEvent, cx| {
                app.touch_knowledge_body(cx);
            }),
            cx.subscribe(&font_size_field, |app, _, _: &InputEvent, cx| {
                app.touch_font_size(cx);
            }),
            cx.subscribe(&skills_query, |app, _, _: &InputEvent, cx| {
                app.touch_skills_query(cx);
            }),
            cx.subscribe(
                &quick_search_query,
                |app, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        app.quick_search.selected = 0;
                        app.quick_search.scroll.scroll_to_item(0);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe(
                &conversation_rename_input,
                |app, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        app.conversation_rename.error = None;
                        cx.notify();
                    }
                },
            ),
            {
                let weak = cx.entity().downgrade();
                cx.intercept_keystrokes(move |event, window, cx| {
                    if window.window_handle().window_id() != app_window_id
                        || event.input_preference
                            == gpui::InputPreference::CharacterInput
                    {
                        return;
                    }
                    let ks = &event.keystroke;
                    if ks.modifiers.control
                        || ks.modifiers.alt
                        || ks.modifiers.platform
                        || ks.modifiers.function
                    {
                        return;
                    }
                    enum QuickSearchKey {
                        Move(isize),
                        Scope(isize),
                    }
                    let action = match ks.key.as_str() {
                        "up" if !ks.modifiers.shift => QuickSearchKey::Move(-1),
                        "down" if !ks.modifiers.shift => {
                            QuickSearchKey::Move(1)
                        }
                        "tab" => QuickSearchKey::Scope(if ks.modifiers.shift {
                            -1
                        } else {
                            1
                        }),
                        _ => return,
                    };
                    let handled = weak
                        .update(cx, |app, cx| {
                            if app.active_dialog
                                != state::ActiveDialog::QuickSearch
                            {
                                return false;
                            }
                            match action {
                                QuickSearchKey::Move(delta) => {
                                    app.move_quick_search_selection(delta, cx);
                                }
                                QuickSearchKey::Scope(delta) => {
                                    app.cycle_quick_search_scope(
                                        delta, window, cx,
                                    );
                                }
                            }
                            true
                        })
                        .unwrap_or(false);
                    if handled {
                        cx.stop_propagation();
                    }
                })
            },
            cx.subscribe(&studio, |app, _, event: &StudioEvent, cx| {
                app.on_studio_event(event, cx);
            }),
        ];
        let expanded_projects =
            studio.read(cx).active_project_id.iter().cloned().collect();
        let initial_installed_skills = studio.read(cx).skill_catalog();
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
            library_generation: 0,
            artifact_navigation_generation: 0,
            artifact_preview: artifact_preview::ArtifactPreviewState::default(),
            artifact_cleanup: artifact_cleanup::ArtifactCleanupState::default(),
            task_menu: None,
            archive_undo: conversation_actions::ArchiveUndoController::default(
            ),
            conversation_delete:
                conversation_actions::ConversationDeleteState::default(),
            conversation_rename:
                conversation_actions::ConversationRenameState::new(
                    conversation_rename_input,
                ),
            quick_search: quick_search::QuickSearchState::new(
                quick_search_query,
            ),
            gallery: GalleryState {
                page: 0,
                idea_filter: gallery::Filter::All,
                idea_note: false,
            },
            skills: SkillsState {
                category: None,
                query: skills_query,
                cards: Vec::new(),
                cursor: None,
                loading: false,
                unavailable: false,
                exhausted: false,
                generation: 0,
                task: None,
                search_task: None,
                opened: None,
                scroll: gpui::ScrollHandle::new(),
                auto_pages: 0,
                follow_armed: std::cell::Cell::new(false),
                empty_follows: 0,
                follow_mark: 0,
                installed: initial_installed_skills,
                install: None,
                install_task: None,
                script_confirm: None,
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
            project_create,
            project_menu: None,
            expanded_projects,
            projects_open: true,
            tasks_open: true,
            persona: PersonaState {
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
            effective_tier: 0,
            applied_tier: None,
            credits_task: None,
            credits_generation: 0,
            credits_poll: None,
            locale_watch: None,
            shell_watch: None,
            subscriptions,
            pending_input_text: std::cell::RefCell::new(Vec::new()),
        };
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::new",
            "→ refresh_models / refresh_credits"
        );
        app.refresh_models(cx);
        app.refresh_credits(cx);
        app.start_credits_poll(cx);
        app.load_library(cx);
        app.sync_studio_skills(cx);
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::new",
            ?screen,
            "done"
        );
        app
    }

    /// Queue programmatic text for an input field.
    ///
    /// The GPUI Kit editing engine installs text through `set_value`, which
    /// needs a `&mut Window`. Most call sites — async callbacks, action
    /// handlers, locale switches — have none, so the text is applied at the
    /// top of `render`, where the window exists.
    ///
    /// Takes `&self` on purpose: callers pass one of their own
    /// `Entity<InputState>` fields, and a `&mut self` receiver would make that
    /// argument a borrow conflict.
    pub(super) fn set_input_text(
        &self,
        state: &Entity<InputState>,
        text: impl Into<SharedString>,
        cx: &mut Context<'_, Self>,
    ) {
        self.pending_input_text
            .borrow_mut()
            .push(PendingText::Single(state.clone(), text.into()));
        cx.notify();
    }

    /// Multi-line counterpart of [`FlowyApp::set_input_text`].
    pub(super) fn set_textarea_text(
        &self,
        state: &Entity<TextareaState>,
        text: impl Into<SharedString>,
        cx: &mut Context<'_, Self>,
    ) {
        self.pending_input_text
            .borrow_mut()
            .push(PendingText::Multi(state.clone(), text.into()));
        cx.notify();
    }

    /// Write a line break into a multi-line field.
    ///
    /// The app binds plain Enter to the composer's submit action for every
    /// `Input`-context editor, and a multi-line field carries that same
    /// context. The field's own key context sits on an ancestor, so it loses
    /// to the focused element on keymap depth; the field forwards the action
    /// into the editor instead, which keeps Enter a line break here and a
    /// submit in the composer.
    #[expect(
        clippy::unused_self,
        reason = "keeps the call site reading as an app command; the body only forwards to the field state"
    )]
    fn insert_line_break(
        &self,
        state: &Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        state.update(cx, |state, cx| state.insert("\n", window, cx));
    }

    /// Installs input text queued by [`FlowyApp::set_input_text`].
    fn apply_pending_input_text(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let pending =
            std::mem::take(&mut *self.pending_input_text.borrow_mut());
        for queued in pending {
            match queued {
                PendingText::Single(state, text) => {
                    state.update(cx, |state, cx| {
                        state.set_value(text, window, cx);
                    });
                }
                PendingText::Multi(state, text) => {
                    state.update(cx, |state, cx| {
                        state.set_value(text, window, cx);
                    });
                }
            }
        }
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

    const fn plan_for_effective_tier(tier: u8) -> flowy_agent::Plan {
        flowy_agent::Plan::from_effective_tier(tier)
    }

    /// Builds the three tier rows from the current catalog and entitlement.
    /// Raw model ids stay in this layer; the menu only receives display data.
    fn tier_views(
        &self,
        cx: &Context<'_, Self>,
    ) -> Vec<flowy_studio::TierView> {
        let models = &self.studio.read(cx).catalog.models;
        flowy_agent::CLOUD_TIERS
            .iter()
            .map(|spec| {
                let resolved =
                    match flowy_agent::resolve_tier(spec.tier, models) {
                        flowy_agent::TierResolution::Available(model) => {
                            Some(model)
                        }
                        flowy_agent::TierResolution::Unavailable => None,
                    };
                let unavailable = resolved.is_none();
                let locked = flowy_agent::is_locked(
                    spec.tier,
                    self.effective_tier,
                    resolved,
                );
                flowy_studio::TierView {
                    tier: spec.tier,
                    pro_badge: spec.pro_badge,
                    locked,
                    unavailable,
                    selected: self.prefs.model == spec.tier,
                }
            })
            .collect()
    }

    fn publish_tier_state(&self, cx: &mut Context<'_, Self>) {
        let views = self.tier_views(cx);
        let plan = Self::plan_for_effective_tier(self.effective_tier);
        self.studio.update(cx, |panel, cx| {
            panel.set_tiers(views, cx);
            panel.plan = plan;
            panel.model = self.prefs.model;
            cx.notify();
        });
    }

    /// Applies one cloud tier. This is the single place that turns a tier into
    /// an agent configuration; every entry (menu, boot, catalog refresh) goes
    /// through it.
    fn select_tier(
        &mut self,
        tier: flowy_agent::Model,
        cx: &mut Context<'_, Self>,
    ) {
        if !tier.is_cloud() {
            // Flex BYOK is deferred; keep the option documented but inert.
            self.note("notice.model_unavailable", cx);
            return;
        }
        if self.catalog_loading {
            return;
        }
        let Some(session) = self.auth.session.clone() else {
            return;
        };
        if session.access_token.trim().is_empty()
            || self.studio.read(cx).agent.turn_status().is_busy()
        {
            return;
        }
        let models = self.studio.read(cx).catalog.models.clone();
        let flowy_agent::TierResolution::Available(model) =
            flowy_agent::resolve_tier(tier, &models)
        else {
            // Never move the selected tier onto a model that cannot be
            // routed: the chip would claim a tier the agent is not using.
            self.publish_tier_state(cx);
            self.note("notice.model_unavailable", cx);
            return;
        };
        let model = model.clone();
        if flowy_agent::is_locked(tier, self.effective_tier, Some(&model)) {
            // Keep the current route; the locked row is a discovery surface.
            self.publish_tier_state(cx);
            self.note("notice.upgrade_later", cx);
            return;
        }
        let config = cloud_model_config(&model, &session.access_token);
        match self.studio.read(cx).agent.apply_config(config) {
            Ok(()) => {
                self.applied_tier = Some(tier);
                self.prefs.model = tier;
                self.prefs.model_id.clone_from(&model.id);
                let save_failed =
                    self.prefs.save_checked(self.store.root()).is_err();
                let selected_id = model.id.clone();
                self.studio.update(cx, |panel, cx| {
                    panel.catalog.select(selected_id);
                    panel.model = tier;
                    panel.set_notice(
                        save_failed.then_some("notice.session_storage_failed"),
                        cx,
                    );
                    panel.notify_cloud_ready(cx);
                });
                self.publish_tier_state(cx);
            }
            Err(key) => {
                self.applied_tier = None;
                self.studio.update(cx, |panel, cx| {
                    panel.cloud_ready = false;
                    panel.refresh_env_status(cx);
                    panel.set_notice(Some(key), cx);
                });
                self.publish_tier_state(cx);
            }
        }
    }

    fn apply_selected_tier(&mut self, cx: &mut Context<'_, Self>) {
        if !self.prefs.model.is_cloud() {
            self.prefs.model = flowy_agent::Model::Lite;
        }
        self.select_tier(self.prefs.model, cx);
    }

    fn refresh_models(&mut self, cx: &mut Context<'_, Self>) {
        let busy = self.studio.read(cx).agent.turn_status().is_busy();
        let signed_in = self.auth.session.is_some();
        if !catalog_refresh_allowed(self.catalog_loading, signed_in, busy) {
            tracing::debug!(
                target: "flowy.boot",
                stage = "FlowyApp::refresh_models",
                loading = self.catalog_loading,
                signed_in,
                busy,
                "skip"
            );
            return;
        }
        let Some(session) = self.auth.session.clone() else {
            return;
        };
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::refresh_models",
            "fetch available_models"
        );
        let client = AuthClient::from_env();
        let account = session
            .user_id
            .map(|id| format!("{}:{id}", client.config().effective_base_url()));
        self.catalog_loading = true;
        let was_ready = self.studio.read(cx).cloud_ready;
        self.studio.update(cx, |panel, cx| {
            panel.cloud_ready = false;
            panel.catalog_loading = true;
            panel.refresh_env_status(cx);
            cx.notify();
        });
        if account.is_some()
            && account.as_deref() != Some(self.prefs.model_account.as_str())
        {
            self.effective_tier = 0;
            self.applied_tier = None;
            self.studio.update(cx, |panel, _| {
                panel.cloud_ready = false;
                panel.catalog = flowy_agent::ProviderCatalog::new();
                // The previous account's tier rows are meaningless now.
                panel.tiers.clear();
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
                            // `effectiveModelTier` is the server's authoritative
                            // entitlement: it covers unexpired personal plans and
                            // team seats. `currentPlan` is display-only.
                            app.effective_tier = u8::try_from(
                                result.effective_model_tier.max(0),
                            )
                            .unwrap_or(0)
                            .min(2);
                            app.prefs.plan = Self::plan_for_effective_tier(
                                app.effective_tier,
                            );
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
                                    min_tier_level: u8::try_from(
                                        m.min_tier_level.max(0),
                                    )
                                    .unwrap_or(0)
                                    .min(2),
                                })
                                .collect::<Vec<_>>();
                            app.prefs.model_account =
                                account.clone().unwrap_or_default();
                            let catalog = flowy_agent::ProviderCatalog::new();
                            app.studio.update(cx, |panel, cx| {
                                panel.cloud_ready = false;
                                panel.catalog = catalog;
                                panel.catalog.replace(models);
                                panel.refresh_env_status(cx);
                                cx.notify();
                            });
                            app.publish_tier_state(cx);
                            app.apply_selected_tier(cx);
                            let _ = app.prefs.save_checked(app.store.root());
                        }
                        Err(AuthError::AuthRequired) => app.sign_out(cx),
                        Err(error) => {
                            let keep_ready =
                                was_ready && app.applied_tier.is_some();
                            app.studio.update(cx, |panel, cx| {
                                panel.cloud_ready = keep_ready;
                                panel.refresh_env_status(cx);
                                cx.notify();
                            });
                            app.note(error.key(), cx);
                        }
                    }
                });
                break;
            }
        }));
    }

    /// Refresh the account-scoped credit balance. Results are keyed by a
    /// generation and by the session token that produced them, so a slow
    /// response can never overwrite a newer refresh or another account's
    /// balance. Background failures only affect the display state; they never
    /// sign the user out.
    fn refresh_credits(&mut self, cx: &mut Context<'_, Self>) {
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::refresh_credits",
            signed_in = self.auth.session.is_some(),
            "enter"
        );
        let Some(session) = self.auth.session.clone() else {
            self.studio.update(cx, |panel, cx| {
                if panel.credits != flowy_studio::CreditsViewState::Unavailable
                {
                    panel.credits = flowy_studio::CreditsViewState::Unavailable;
                    cx.notify();
                }
            });
            return;
        };
        let client = AuthClient::from_env();
        self.credits_generation = self.credits_generation.wrapping_add(1);
        let generation = self.credits_generation;
        let token = session.access_token;
        let token_marker = token.clone();
        if matches!(
            self.studio.read(cx).credits,
            flowy_studio::CreditsViewState::Unavailable
                | flowy_studio::CreditsViewState::Error
        ) {
            self.studio.update(cx, |panel, cx| {
                panel.credits = flowy_studio::CreditsViewState::Loading;
                cx.notify();
            });
        }
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.credits_balance(&token);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.credits_task = Some(cx.spawn(async move |weak, cx| {
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
                    if generation != app.credits_generation {
                        return;
                    }
                    let same_session = app
                        .auth
                        .session
                        .as_ref()
                        .is_some_and(|s| s.access_token == token_marker);
                    if !same_session {
                        return;
                    }
                    let next = if let Ok(balance) = result {
                        credits_state_from_balance(balance)
                    } else {
                        if matches!(
                            app.studio.read(cx).credits,
                            flowy_studio::CreditsViewState::Ready { .. }
                        ) {
                            // Keep the last confirmed balance; a single failed
                            // poll must not blank it.
                            return;
                        }
                        flowy_studio::CreditsViewState::Error
                    };
                    app.studio.update(cx, |panel, cx| {
                        panel.credits = next;
                        cx.notify();
                    });
                });
                break;
            }
        }));
    }

    /// Poll the balance every ten minutes while a session exists.
    fn start_credits_poll(&mut self, cx: &Context<'_, Self>) {
        self.credits_poll = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(600))
                    .await;
                if weak
                    .update(cx, |app, cx| {
                        if app.auth.session.is_some() {
                            app.refresh_credits(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
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
        let email = self.auth.email.read(cx).value().to_string();
        self.auth.generation = self.auth.generation.saturating_add(1);
        let generation = self.auth.generation;
        self.auth.phase = LoginPhase::Sending;
        self.auth.message = None;
        self.auth.attempted_code.clear();
        self.set_input_text(&self.auth.code, "", cx);
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
        let email = self.auth.email.read(cx).value().to_string();
        let code = normalize_otp(self.auth.code.read(cx).value().as_ref());
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
                            app.set_input_text(&app.auth.code, "", cx);
                            app.auth.session = Some(session);
                            app.auth.phase = LoginPhase::Success;
                            app.auth.message = Some("login.success");
                            app.auth.pending_req_no = None;
                            app.auth.cooldown_left = 0;
                            app.notice = None;
                            app.screen = Screen::Main;
                            app.refresh_models(cx);
                            app.refresh_credits(cx);
                            app.studio.update(cx, |panel, cx| {
                                panel.dismiss_overlay("model-info", cx);
                            });
                        }
                        Err(AuthError::InvalidCode) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.attempted_code.clear();
                            app.set_input_text(&app.auth.code, "", cx);
                            app.auth.message =
                                Some(AuthError::InvalidCode.key());
                        }
                        Err(AuthError::SessionExpired) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.pending_req_no = None;
                            app.auth.attempted_code.clear();
                            app.set_input_text(&app.auth.code, "", cx);
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
        let raw = self.auth.code.read(cx).value().to_string();
        let normalized = normalize_otp(&raw);
        if normalized != raw {
            self.set_input_text(&self.auth.code, normalized.as_str(), cx);
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
        self.set_input_text(&self.auth.code, "", cx);
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
        self.effective_tier = 0;
        self.applied_tier = None;
        self.credits_generation = self.credits_generation.wrapping_add(1);
        self.credits_task = None;
        self.studio.update(cx, |panel, cx| {
            panel.prepare_for_sign_out(Self::disconnected_config(), cx);
        });
        self.store.clear();
        self.set_input_text(&self.auth.email, "", cx);
        self.set_input_text(&self.auth.code, "", cx);
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
        self.persona.status = None;
        self.user_menu = false;
        self.archive_undo.clear();
        self.notice = None;
        self.studio
            .update(cx, flowy_studio::StudioPanel::reset_session);
        self.gallery.page = 0;
        self.gallery.idea_note = false;
        self.skills.category = None;
        self.skills.cards.clear();
        self.sync_studio_skills(cx);
        self.skills.loading = false;
        self.skills.unavailable = false;
        self.skills.exhausted = false;
        self.skills.generation = self.skills.generation.wrapping_add(1);
        self.skills.task = None;
        self.skills.search_task = None;
        self.skills.opened = None;
        self.ota.reset();
        cx.notify();
    }

    pub fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
        tracing::debug!(
            target: "flowy.prompt",
            stage = "FlowyApp::send_prompt",
            "enter"
        );
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

    pub(super) fn open_project_board(
        &mut self,
        project_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.project_menu = None;
        self.user_menu = false;
        self.push_section(Section::Board, cx);
        let pid = project_id.to_owned();
        self.studio.update(cx, |panel, cx| {
            panel.open_board_for_project(Some(pid), cx);
        });
        cx.notify();
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
                    // another section, but never on launch: an existing
                    // history must not pull the app back into the last thread.
                    let surface = if panel.session_opened_by_user()
                        && panel.agent.has_messages()
                    {
                        StudioSurface::Chat
                    } else {
                        StudioSurface::Home
                    };
                    panel.close_all_popovers();
                    panel.close_flow_modal(cx);
                    panel.set_surface(surface, cx);
                    panel.set_notice(notice, cx);
                });
            }
            Section::Board => {
                self.studio.update(cx, |panel, cx| {
                    panel.open_board(cx);
                    panel.set_notice(notice, cx);
                });
            }
            Section::Automation => {
                self.studio.update(cx, |panel, cx| {
                    panel.close_all_popovers();
                    panel.close_flow_modal(cx);
                    panel.set_surface(StudioSurface::Automation, cx);
                    panel.set_notice(notice, cx);
                });
            }
            Section::Library => {
                self.studio.update(cx, |panel, cx| {
                    panel.close_all_popovers();
                    panel.close_flow_modal(cx);
                    panel.set_notice(notice, cx);
                });
            }
            Section::Create => {
                self.studio.update(cx, |panel, cx| {
                    panel.close_all_popovers();
                    panel.close_flow_modal(cx);
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
            Section::More => {
                self.studio.update(cx, |panel, cx| {
                    panel.close_all_popovers();
                    panel.close_flow_modal(cx);
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
            StudioEvent::TierSelected(tier) => {
                self.select_tier(*tier, cx);
            }
            StudioEvent::RefreshModels => {
                self.refresh_models(cx);
                self.refresh_credits(cx);
            }
            StudioEvent::TurnFinished => {
                self.refresh_credits(cx);
                self.load_library(cx);
                self.maybe_notify_turn_finished(cx);
            }
            StudioEvent::PreviewArtifact(id) => {
                self.open_artifact_preview(id, cx);
            }
            StudioEvent::SaveArtifact(id) => self.save_artifact_as(id, cx),
            StudioEvent::LocateArtifactSource {
                session_id,
                artifact_id,
            } => self.library_event(
                &LibraryEvent::LocateSourceRequested {
                    session_id: session_id.clone(),
                    artifact_id: artifact_id.clone(),
                },
                cx,
            ),
            StudioEvent::ArtifactsChanged => self.load_library(cx),
            StudioEvent::OpenIdeas => {
                self.activate_dialog(state::ActiveDialog::Ideas, cx);
                cx.notify();
            }
            StudioEvent::SessionsChanged => {
                if self.section == Section::Board
                    && self.studio.read(cx).surface != StudioSurface::Board
                {
                    self.nav_back.push(Section::Board);
                    self.nav_forward.clear();
                    self.section = Section::Studio;
                }
                self.load_library(cx);
                cx.notify();
            }
            StudioEvent::SkillsChanged => {
                self.skills.installed = self.studio.read(cx).skill_catalog();
                self.sync_studio_skills(cx);
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
            StudioEvent::OpenSessionMenu {
                session_id,
                position,
            } => {
                self.open_task_menu_at(session_id, *position, cx);
            }
            StudioEvent::ToggleSessionMenu {
                session_id,
                anchor_id,
            } => {
                self.toggle_task_menu_anchored(
                    session_id,
                    anchor_id.clone(),
                    cx,
                );
            }
        }
    }

    fn main_canvas(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
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
            canvas.child(self.more_canvas(window, cx))
        } else if self.section == Section::Library {
            canvas.child(self.library.clone())
        } else {
            canvas.child(self.studio.clone())
        };
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
                    app.sync_artifact_preview_locale(cx);
                    app.sync_studio_skills(cx);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    fn maybe_notify_turn_finished(&self, cx: &Context<'_, Self>) {
        if !self.prefs.notify_browser || cx.active_window().is_some() {
            return;
        }
        let studio = self.studio.read(cx);
        let needs_attention =
            studio.agent.list_sessions().iter().any(|session| {
                studio.agent.session_needs_attention(&session.id)
            });
        let has_error = matches!(
            studio.agent.turn_status(),
            flowy_agent::TurnStatus::Error { .. }
        );
        let still_busy = studio.total_active_work_count() > 0;
        let session_title = studio.agent.session_title();
        let Some((tag, title, body)) = turn_finished_notification_payload(
            &self.i18n,
            needs_attention,
            has_error,
            still_busy,
            Some(&session_title),
        ) else {
            return;
        };
        let open = self.t("shell.notify_open");
        flowy_shell::show_task_notification(cx, tag, &title, &body, &open);
    }

    fn start_shell_watch(&mut self, cx: &Context<'_, Self>) {
        let store_root = self.store.root().to_path_buf();
        self.shell_watch = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if weak.upgrade().is_none() {
                    break;
                }
                if let Some(signal) = flowy_shell::take_wake(&store_root) {
                    if !signal.urls.is_empty() {
                        flowy_shell::enqueue_urls(signal.urls);
                    }
                    cx.update(ensure_shell_main_window);
                }
                let pending = flowy_shell::take_pending();
                if pending.is_empty() {
                    continue;
                }
                let mut open_requested = false;
                let updated = weak.update(cx, |app, cx| {
                    for url in pending {
                        match flowy_shell::parse_deep_link(&url) {
                            Some(flowy_shell::DeepLink::Invite { code }) => {
                                app.receive_invite_code(code, cx);
                                open_requested = true;
                            }
                            Some(flowy_shell::DeepLink::Open) => {
                                open_requested = true;
                            }
                            None => {}
                        }
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
                if open_requested {
                    cx.update(ensure_shell_main_window);
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
        self.apply_pending_input_text(window, cx);
        let modal = self
            .active_dialog
            .scope()
            .map(gpui::SharedString::from)
            .or_else(|| {
                (self.screen == Screen::Main
                    && self.section == Section::Library)
                    .then(|| self.library.read(cx).modal_scope())
                    .flatten()
            })
            .or_else(|| {
                (self.screen == Screen::Main
                    && self.section != Section::More
                    && self.section != Section::Library
                    && self.studio.read(cx).has_flow_modal())
                .then(|| gpui::SharedString::from("flow-modal-panel"))
            });
        let popup = match self.settings.picker {
            settings::Picker::Language => Some("language-menu"),
            settings::Picker::Accent => Some("accent-menu"),
            settings::Picker::Density => Some("density-menu"),
            settings::Picker::None if self.flex.provider_menu => {
                Some("flex-provider-menu")
            }
            settings::Picker::None if self.user_menu => Some("user-menu"),
            settings::Picker::None if self.project_menu.is_some() => {
                Some("project-menu")
            }
            settings::Picker::None if self.task_menu.is_some() => {
                Some("task-menu")
            }
            settings::Picker::None
                if self.active_dialog == state::ActiveDialog::None
                    && self.screen == Screen::Main
                    && self.section != Section::More
                    && self.section != Section::Library
                    && !self.studio.read(cx).has_flow_modal()
                    && self.studio.read(cx).surface
                        != StudioSurface::Automation =>
            {
                self.studio.read(cx).active_popup_scope()
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
        self.reconcile_artifact_preview(window, cx);
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
            .on_action(cx.listener(|app, _: &InsertIndent, window, cx| {
                // Reached from every "Input" context, so find the focused
                // field explicitly. A single-line field falls through to the
                // usual focus move instead of swallowing the key.
                let focused = window.focused(cx);
                for field in [
                    &app.persona.about,
                    &app.persona.instructions,
                    &app.persona.import_paste,
                    &app.persona.knowledge_body,
                ] {
                    let presentation = field.read(cx).presentation();
                    if Some(presentation.focus_handle()) == focused.as_ref() {
                        if presentation.is_disabled()
                            || presentation.is_readonly()
                        {
                            return;
                        }
                        field.update(cx, |state, cx| {
                            state.insert("    ", window, cx);
                        });
                        cx.notify();
                        return;
                    }
                }
                window.focus_next(cx);
            }))
            .on_action(cx.listener(|app, _: &ToggleDock, _, cx| {
                if app.ui.overlays.read(cx).modal_active() {
                    return;
                }
                app.toggle_dock(cx);
            }))
            .on_action(cx.listener(|app, _: &NavigateBack, _, cx| {
                if app.ui.overlays.read(cx).modal_active() {
                    return;
                }
                app.navigate_back(cx);
            }))
            .on_action(cx.listener(|app, _: &NavigateForward, _, cx| {
                if app.ui.overlays.read(cx).modal_active() {
                    return;
                }
                app.navigate_forward(cx);
            }))
            .on_action(cx.listener(|app, _: &OpenSearch, window, cx| {
                if app.active_dialog != state::ActiveDialog::None
                    && app.active_dialog != state::ActiveDialog::QuickSearch
                {
                    return;
                }
                app.open_search(window, cx);
            }));
        root = root.relative().child(self.titlebar(window, cx));
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
        // gpui-pre has no `aria_hidden`; the modal layer still covers the
        // background, it just cannot hide it from the accessibility tree.
        root = root.child(background);
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
            state::ActiveDialog::RemoteSkill => {
                root.child(self.skill_shelf_layer(window, cx))
            }
            state::ActiveDialog::Skill(_)
            | state::ActiveDialog::SkillUnlock(_)
            | state::ActiveDialog::SkillDocs(_) => {
                let mut root = root.child(self.skill_layer(window, cx));
                if matches!(
                    self.active_dialog,
                    state::ActiveDialog::SkillUnlock(_)
                ) {
                    root = root.child(self.skill_unlock_layer(window, cx));
                }
                if matches!(
                    self.active_dialog,
                    state::ActiveDialog::SkillDocs(_)
                ) {
                    root = root.child(self.skill_docs_layer(window, cx));
                }
                root
            }
            state::ActiveDialog::SkillScriptConfirm => root
                .child(self.skill_layer(window, cx))
                .child(self.skill_script_confirm_layer(window, cx)),
            state::ActiveDialog::Invite => root.child(self.invite_layer(cx)),
            state::ActiveDialog::ProjectCreate => {
                root.child(self.projects_layer(window, cx))
            }
            state::ActiveDialog::ProjectDelete => {
                root.child(self.project_delete_layer(window, cx))
            }
            state::ActiveDialog::Flex => root.child(self.flex_layer(cx)),
            state::ActiveDialog::Import | state::ActiveDialog::Knowledge => {
                root.child(self.more_layer(window, cx))
            }
            state::ActiveDialog::Legal(_) => root.child(self.legal_layer(cx)),
            state::ActiveDialog::ArtifactPreview => {
                root.child(self.artifact_preview_layer(cx))
            }
            state::ActiveDialog::ConversationDelete => {
                root.child(self.conversation_delete_layer(window, cx))
            }
            state::ActiveDialog::ConversationRename => {
                root.child(self.conversation_rename_layer(window, cx))
            }
            state::ActiveDialog::QuickSearch => {
                root.child(self.quick_search_layer(window, cx))
            }
            state::ActiveDialog::ArtifactCleanup => {
                root.child(self.artifact_cleanup_layer(window, cx))
            }
        };
        if self.active_dialog == state::ActiveDialog::None
            && self.screen == Screen::Main
            && self.section != Section::More
            && self.section != Section::Library
            && let Some(modal) = self
                .studio
                .update(cx, |panel, cx| panel.flow_modal_overlay(window, cx))
        {
            root = root.child(modal);
        }
        let menu = match self.settings.picker {
            settings::Picker::Language => Some((
                "language-menu",
                "language-button",
                self.language_menu(cx).into_any_element(),
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
            settings::Picker::None if self.user_menu => Some((
                "user-menu",
                "dock-user",
                self.user_menu_content(cx).into_any_element(),
            )),
            settings::Picker::None => None,
        };
        if let Some((id, anchor, content)) = menu {
            let popover = if id == "user-menu" {
                flowy_ui::popover_above(
                    id,
                    anchor,
                    content,
                    &self.ui,
                    window,
                    cx,
                    self.i18n.locale().is_rtl(),
                )
            } else {
                flowy_ui::popover(
                    id,
                    anchor,
                    content,
                    &self.ui,
                    window,
                    cx,
                    self.i18n.locale().is_rtl(),
                )
            };
            root = root.child(popover);
        }
        if let Some(project_id) = &self.project_menu {
            let anchor = format!("project-menu-trigger-{project_id}");
            root = root.child(flowy_ui::popover(
                "project-menu",
                anchor,
                self.project_menu_content(project_id, window, cx),
                &self.ui,
                window,
                cx,
                self.i18n.locale().is_rtl(),
            ));
        }
        if let Some(menu) = &self.task_menu {
            root = root.child(flowy_ui::popover(
                "task-menu",
                menu.anchor_id.clone(),
                self.task_menu_content(&menu.session_id, window, cx),
                &self.ui,
                window,
                cx,
                self.i18n.locale().is_rtl(),
            ));
        }
        if self.prefs.activity && self.screen == Screen::Main {
            root = root.child(self.activity_floater());
        }
        if self.screen == Screen::Main
            && !self.ui.overlays.read(cx).modal_active()
            && let Some(banner) = self.archive_undo_banner(cx)
        {
            root = root.child(banner);
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

fn input_field(
    window: &mut Window,
    cx: &mut Context<'_, FlowyApp>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx))
}

/// Multi-line fields need their own state type; the kit keeps the single-line
/// and multi-line engines separate.
fn textarea_field(
    window: &mut Window,
    cx: &mut Context<'_, FlowyApp>,
) -> Entity<TextareaState> {
    cx.new(|cx| TextareaState::new(window, cx))
}

fn place<M: InputModeKind>(
    window: &mut Window,
    field: &Entity<InputBaseState<M>>,
    text: &str,
    cx: &mut Context<'_, FlowyApp>,
) {
    field.update(cx, |state, cx| state.set_value(text, window, cx));
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
    tracing::debug!(
        target: "flowy.boot",
        stage = "app::run",
        "enter"
    );
    cx.set_app_identity(crate::brand::bundle_id(), crate::brand::product());
    // GPUI Kit owns the editing keymap: it registers backspace/arrow/enter/tab
    // bindings for the `"Input"` key context its inputs render with. Install it
    // first so the app's own bindings win at the same context depth.
    gpui_kit::base::init(cx);
    cx.bind_keys([
        KeyBinding::new("enter", SignIn, Some("Login")),
        // The composer submits on Enter; Shift+Enter reaches the editor's own
        // Enter action, which keeps inserting newlines.
        KeyBinding::new("enter", flowy_studio::SubmitComposer, Some("Input")),
        KeyBinding::new(
            "shift-enter",
            Enter {
                secondary: false,
                shift: true,
            },
            Some("Input"),
        ),
        // Tab moves focus instead of inserting an indent in a field.
        KeyBinding::new("tab", FocusNext, Some("Input")),
        KeyBinding::new("shift-tab", FocusPrevious, Some("Input")),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
        // Registered after the two above, at the same depth, so it wins:
        // same-depth bindings resolve last-registered-first. The outer key
        // context of a multi-line field sits on an ancestor and therefore
        // loses to the editor's own "Input", which is why this cannot live
        // there. The handler moves focus itself when no multi-line field holds
        // it, so single-line fields keep their old behaviour.
        KeyBinding::new("tab", InsertIndent, Some("Input")),
        // Cursor / VS Code: toggle primary side bar
        KeyBinding::new("ctrl-b", ToggleDock, None),
        // Cursor / VS Code: navigate back / forward
        KeyBinding::new("ctrl--", NavigateBack, None),
        KeyBinding::new("alt-left", NavigateBack, None),
        KeyBinding::new("ctrl-[", NavigateBack, None),
        KeyBinding::new("ctrl-shift--", NavigateForward, None),
        KeyBinding::new("alt-right", NavigateForward, None),
        KeyBinding::new("ctrl-]", NavigateForward, None),
        // Global quick search (`Ctrl+K` and `Ctrl+Shift+F`), also reachable from inputs.
        KeyBinding::new("ctrl-k", OpenSearch, None),
        KeyBinding::new("ctrl-k", OpenSearch, Some("Input")),
        KeyBinding::new("ctrl-shift-f", OpenSearch, None),
        KeyBinding::new("ctrl-shift-f", OpenSearch, Some("Input")),
        // Keep the platform's quit shortcut available without a tray icon.
        KeyBinding::new("ctrl-q", flowy_shell::QuitApp, None),
    ]);
    cx.set_reduce_motion(
        flowy_ui::ResolvedTheme::default().motion_policy()
            == flowy_ui::MotionPolicy::Reduced,
    );
    flowy_ui::install_initial_tab_focus(cx).detach();
    tracing::debug!(
        target: "flowy.boot",
        stage = "app::run",
        "→ shell::install"
    );
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
    tracing::debug!(
        target: "flowy.boot",
        stage = "app::run",
        "→ open_main_window"
    );
    let window = open_main_window(cx);
    bind_shell_main_window(window);
    cx.activate(true);
    tracing::debug!(
        target: "flowy.boot",
        stage = "app::run",
        "done (window open + activated)"
    );
}

pub fn open_main_window(cx: &mut App) -> gpui::WindowHandle<FlowyApp> {
    tracing::debug!(
        target: "flowy.boot",
        stage = "open_main_window",
        "enter"
    );
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
    tracing::debug!(
        target: "flowy.boot",
        stage = "open_main_window::SessionStore::new",
        root = %store.root().display(),
        "session store"
    );
    let window = cx
        .open_window(options, |window, cx| {
            let dark = matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            );
            tracing::debug!(
                target: "flowy.boot",
                stage = "open_main_window::open_window",
                dark,
                "→ FlowyApp::new"
            );
            cx.new(|cx| FlowyApp::new(store, dark, window, cx))
        })
        .expect("open main window");
    window
        .update(cx, |app, window, cx| {
            window.on_window_should_close(cx, |window, cx| {
                // Hide to the tray only when a tray icon exists to bring the
                // window back; otherwise closing would strand the process
                // with no window and no way to quit.
                if flowy_shell::tray_available() {
                    // gpui-pre has no `Window::set_visible`, so minimizing is
                    // the closest "keep the process alive" move the tray can
                    // still restore from.
                    window.minimize_window();
                    return false;
                }
                cx.quit();
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
            tracing::debug!(
                target: "flowy.boot",
                stage = "open_main_window",
                ?app.screen,
                "post-open hooks done"
            );
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
                // gpui-pre has no `set_visible`; the window is never hidden now,
                // so bringing it to the front is enough.
                window.activate_window();
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

/// Maps the cloud's signed balance onto the display state. Negative values
/// are clamped to zero and values above `u64::MAX` saturate.
fn credits_state_from_balance(balance: i64) -> flowy_studio::CreditsViewState {
    let total = u64::try_from(balance.max(0)).unwrap_or(u64::MAX);
    flowy_studio::CreditsViewState::Ready {
        total,
        free: None,
        daily: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Focusable, TestAppContext, VisualTestContext, accesskit};
    // gpui-pre never builds the accessibility tree in tests (no platform
    // adapter activates it), so `Window::a11y_tree()` has no replacement of
    // that shape. `gpui-kit`'s test support reads the same native role/label
    // properties, but only for elements that registered themselves with
    // `.test_support()` in production code. Until the row/button builders do
    // that, these label queries return nothing and the assertions below
    // report the missing label.
    use gpui_kit::test::TestQueryExt;
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
    fn credits_balance_maps_to_a_ready_total_without_ui_side_effects() {
        use flowy_studio::CreditsViewState;

        assert_eq!(
            credits_state_from_balance(179_620),
            CreditsViewState::Ready {
                total: 179_620,
                free: None,
                daily: None,
            }
        );
        assert_eq!(
            credits_state_from_balance(-5),
            CreditsViewState::Ready {
                total: 0,
                free: None,
                daily: None,
            }
        );
        assert_eq!(
            credits_state_from_balance(i64::MAX),
            CreditsViewState::Ready {
                total: u64::try_from(i64::MAX).unwrap(),
                free: None,
                daily: None,
            }
        );
    }

    #[test]
    fn catalog_refresh_gate_is_single_flight_for_idle_sessions() {
        assert!(catalog_refresh_allowed(false, true, false));
        assert!(!catalog_refresh_allowed(true, true, false));
        assert!(!catalog_refresh_allowed(false, false, false));
        assert!(!catalog_refresh_allowed(false, true, true));
    }

    #[gpui::test]
    fn skills_and_personalization_are_direct_navigation_items(
        cx: &mut TestAppContext,
    ) {
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..4 {
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
            }
            cx.run_until_parked();
        }

        fn click(cx: &mut VisualTestContext, label: &str) {
            let bounds = cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .unwrap_or_else(|| panic!("missing button {label:?}"))
                    .bounds()
            });
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            draw(cx);
        }

        let root = test_root();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                false,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.section = Section::More;
            app.ota.checked = true;
            app
        });
        let (plugins, more, skills, personalization, automation) = cx
            .read_entity(&app, |app, _| {
                (
                    app.t("dock.plugins"),
                    app.t("dock.more"),
                    app.t("more.skills"),
                    app.t("more.personal"),
                    app.t("dock.automation"),
                )
            });
        draw(cx);
        cx.update(|window, _| {
            for label in [&plugins, &more] {
                assert!(
                    window.find_all_by_label(label).iter().all(|node| {
                        node.role() != Some(accesskit::Role::Button)
                            || !node.visible()
                    }),
                    "hidden navigation item {label:?} must not render",
                );
            }
            for label in [&skills, &personalization] {
                assert_eq!(
                    window
                        .find_all_by_label(label)
                        .iter()
                        .filter(|node| {
                            node.role() == Some(accesskit::Role::Button)
                                && node.visible()
                        })
                        .count(),
                    1,
                    "direct navigation item {label:?} must render once",
                );
            }
        });
        click(cx, &skills);
        cx.read_entity(&app, |app, _| {
            assert_eq!(app.persona.page, more::Page::Skills);
            assert_eq!(app.section, Section::More);
        });
        click(cx, &automation);
        cx.read_entity(&app, |app, _| {
            assert_eq!(app.section, Section::Automation);
        });
        click(cx, &personalization);
        cx.read_entity(&app, |app, _| {
            assert_eq!(app.section, Section::More);
            assert_eq!(app.persona.page, more::Page::Personal);
            assert_eq!(app.nav_back.last(), Some(&Section::Automation));
        });
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn project_group_header_hover_covers_label_space_and_add_button(
        cx: &mut TestAppContext,
    ) {
        fn draw(cx: &mut VisualTestContext) {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
        }

        fn has_header_fill(
            cx: &mut VisualTestContext,
            sample: gpui::Point<gpui::Pixels>,
            color: gpui::Rgba,
        ) -> bool {
            cx.update(|window, _| {
                let sample = sample.scale(window.scale_factor());
                window.painted_quads().iter().any(|quad| {
                    quad.bounds.contains(&sample)
                        && quad.background == gpui::Background::from(color)
                })
            })
        }

        fn new_project_bounds(
            cx: &mut VisualTestContext,
            label: &str,
        ) -> Option<gpui::Bounds<gpui::Pixels>> {
            cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .map(|node| node.bounds())
            })
        }

        for dark in [false, true] {
            let root = test_root();
            let folder = root.join("workspace");
            std::fs::create_dir_all(&folder).unwrap();
            let (app, cx) = cx.add_window_view(|window, cx| {
                let mut app = FlowyApp::new(
                    SessionStore::new(root.clone()),
                    dark,
                    window,
                    cx,
                );
                app.screen = Screen::Main;
                app.ota.checked = true;
                app
            });
            app.update(cx, |app, cx| {
                app.studio.update(cx, |panel, cx| {
                    panel.create_project("Client", &folder, cx).unwrap();
                });
                let id = app.studio.read(cx).projects[0].id.clone();
                app.expanded_projects.insert(id);
                cx.notify();
            });
            for _ in 0..4 {
                draw(cx);
            }
            let (add_label, hover) = cx.read_entity(&app, |app, _| {
                (app.t("dock.new_project"), app.theme.dock_selected())
            });
            let header_bounds = cx
                .debug_bounds("project-group-header")
                .expect("project group header bounds");
            let project_bounds =
                cx.debug_bounds("project-row").expect("project row bounds");
            let nav_bounds = cx
                .debug_bounds("dock-nav-item")
                .expect("dock nav item bounds");
            assert_eq!(header_bounds.size, nav_bounds.size);
            assert_eq!(project_bounds.size, nav_bounds.size);
            assert_eq!(
                project_bounds.origin.y
                    - (header_bounds.origin.y + header_bounds.size.height),
                px(2.)
            );
            let sample = gpui::point(px(130.), header_bounds.center().y);
            let project_sample = project_bounds.center();
            let outside = gpui::point(px(300.), px(80.));
            cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
            draw(cx);
            assert!(!has_header_fill(cx, sample, hover));
            assert!(
                !has_header_fill(cx, project_sample, hover),
                "an expanded project row must not stay highlighted: dark={dark}"
            );
            assert!(
                cx.debug_bounds("project-actions").is_none(),
                "project actions stay hidden until hover: dark={dark}"
            );
            assert!(
                new_project_bounds(cx, &add_label).is_none(),
                "new project button stays hidden until the heading is hovered: dark={dark}"
            );
            cx.simulate_mouse_move(
                project_sample,
                None,
                gpui::Modifiers::default(),
            );
            draw(cx);
            assert!(
                has_header_fill(cx, project_sample, hover),
                "project row must highlight while hovered: dark={dark}"
            );
            let actions_bounds = cx
                .debug_bounds("project-actions")
                .expect("hovered project actions bounds");
            assert!(
                actions_bounds.size.width > px(0.),
                "project actions appear on hover: dark={dark}, bounds={actions_bounds:?}"
            );
            cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
            draw(cx);
            assert!(!has_header_fill(cx, project_sample, hover));

            cx.simulate_mouse_move(sample, None, gpui::Modifiers::default());
            draw(cx);
            assert!(
                has_header_fill(cx, sample, hover),
                "project heading must highlight while hovered: dark={dark}"
            );
            let add_bounds = new_project_bounds(cx, &add_label).expect(
                "new project button appears when the heading is hovered",
            );
            // The retired fork centered the add button exactly on the header row. Under
            // gpui-pre the row is 34.5px tall while the small icon button stays
            // 28px and sits ~3px below the centerline, so an exact equality no
            // longer holds. Keep the "same row" intent with a tolerance rather
            // than deleting the check.
            assert!(
                (add_bounds.center().y - header_bounds.center().y).abs()
                    < px(4.),
                "the add button must sit on the header row: \
                 add={add_bounds:?} header={header_bounds:?}"
            );
            for pointer in
                [gpui::point(px(24.), sample.y), sample, add_bounds.center()]
            {
                cx.simulate_mouse_move(
                    pointer,
                    None,
                    gpui::Modifiers::default(),
                );
                draw(cx);
                assert!(
                    has_header_fill(cx, sample, hover),
                    "project heading must highlight across its entire row: dark={dark}, pointer={pointer:?}"
                );
                assert!(
                    new_project_bounds(cx, &add_label).is_some(),
                    "new project button stays visible across the heading: dark={dark}, pointer={pointer:?}"
                );
            }
            cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
            draw(cx);
            assert!(!has_header_fill(cx, sample, hover));
            assert!(
                new_project_bounds(cx, &add_label).is_none(),
                "new project button hides when the pointer leaves the heading: dark={dark}"
            );
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[gpui::test]
    fn conversation_more_button_appears_only_while_its_row_is_hovered(
        cx: &mut TestAppContext,
    ) {
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..4 {
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
            }
            cx.run_until_parked();
        }

        fn button_bounds(
            cx: &mut VisualTestContext,
            label: &str,
        ) -> Option<gpui::Bounds<gpui::Pixels>> {
            cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .map(|node| node.bounds())
            })
        }

        fn contains_box(
            outer: gpui::Bounds<gpui::Pixels>,
            inner: gpui::Bounds<gpui::Pixels>,
        ) -> bool {
            let pad = px(1.);
            inner.origin.x + pad >= outer.origin.x
                && inner.origin.y + pad >= outer.origin.y
                && inner.origin.x + inner.size.width
                    <= outer.origin.x + outer.size.width + pad
                && inner.origin.y + inner.size.height
                    <= outer.origin.y + outer.size.height + pad
        }

        let root = test_root();
        let folder = root.join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                false,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.ota.checked = true;
            app
        });
        app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                panel.create_project("Client", &folder, cx).unwrap();
                panel.agent.create_session().unwrap();
                panel.agent.set_workspace_for_new_sessions(None);
                panel.agent.create_session().unwrap();
            });
            let id = app.studio.read(cx).projects[0].id.clone();
            app.expanded_projects.insert(id);
            cx.notify();
        });
        let more = cx.read_entity(&app, |app, _| app.t("library.more"));
        draw(cx);

        let outside = gpui::point(px(300.), px(80.));
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);
        assert!(
            cx.debug_bounds("task-actions").is_none(),
            "conversation more button stays hidden until the row is hovered"
        );
        assert!(button_bounds(cx, &more).is_none());

        let project = cx.debug_bounds("project-row").expect("project row");
        cx.simulate_mouse_move(
            project.center(),
            None,
            gpui::Modifiers::default(),
        );
        draw(cx);
        let project_actions = cx
            .debug_bounds("project-actions")
            .expect("project more button appears on hover");
        let project_edit = cx
            .debug_bounds("project-edit")
            .expect("project edit button appears on hover");
        assert!(
            contains_box(project, project_actions),
            "project hover fill must contain its more button: row={project:?} actions={project_actions:?}"
        );
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);

        let row = cx.debug_bounds("task-row").expect("conversation row");
        cx.simulate_mouse_move(row.center(), None, gpui::Modifiers::default());
        draw(cx);
        let actions = cx
            .debug_bounds("task-actions")
            .expect("conversation more button appears on hover");
        assert!(
            contains_box(row, actions),
            "conversation hover fill must contain its more button: row={row:?} actions={actions:?}"
        );
        assert!(
            (actions.origin.x - project_edit.origin.x).abs() < px(1.)
                && (actions.size.width - project_edit.size.width).abs()
                    < px(1.),
            "conversation more button must line up with the project edit button: conversation={actions:?} edit={project_edit:?}"
        );
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);
        assert!(
            cx.debug_bounds("task-actions").is_none(),
            "conversation more button hides when the pointer leaves the row"
        );

        let project_task = cx
            .debug_bounds("project-task-row")
            .expect("project conversation row");
        cx.simulate_mouse_move(
            project_task.center(),
            None,
            gpui::Modifiers::default(),
        );
        draw(cx);
        let nested_actions = cx
            .debug_bounds("task-actions")
            .expect("project conversation more button appears on hover");
        assert!(
            contains_box(project_task, nested_actions),
            "project conversation hover fill must contain its more button"
        );
        assert!(
            (nested_actions.origin.x - project_edit.origin.x).abs() < px(1.)
                && (nested_actions.size.width - project_edit.size.width).abs()
                    < px(1.),
            "nested conversation more button must line up with the project edit button: nested={nested_actions:?} edit={project_edit:?}"
        );
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);
        assert!(
            cx.debug_bounds("task-actions").is_none(),
            "project conversation more button hides when the pointer leaves"
        );

        let row = cx.debug_bounds("task-row").expect("conversation row");
        cx.simulate_mouse_move(row.center(), None, gpui::Modifiers::default());
        draw(cx);
        let more_bounds =
            button_bounds(cx, &more).expect("more button is reachable");

        cx.simulate_click(more_bounds.center(), gpui::Modifiers::default());
        draw(cx);
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);
        assert!(
            cx.debug_bounds("task-actions").is_some(),
            "more button stays while its menu is open"
        );
        cx.read_entity(&app, |app, _| {
            assert!(app.task_menu.is_some());
        });

        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn project_and_task_sections_collapse_and_expand(cx: &mut TestAppContext) {
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..4 {
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
            }
            cx.run_until_parked();
        }

        fn click(cx: &mut VisualTestContext, label: &str) {
            let bounds = cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .unwrap_or_else(|| panic!("missing button {label:?}"))
                    .bounds()
            });
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            draw(cx);
        }

        fn button_count(cx: &mut VisualTestContext, label: &str) -> usize {
            cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .iter()
                    .filter(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .count()
            })
        }

        let root = test_root();
        let folder = root.join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                false,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.ota.checked = true;
            app
        });
        app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                panel.create_project("Client", &folder, cx).unwrap();
                panel.agent.set_workspace_for_new_sessions(None);
                panel.agent.create_session().unwrap();
            });
            cx.notify();
        });
        draw(cx);

        let (
            collapse_projects,
            expand_projects,
            collapse_tasks,
            expand_tasks,
            task_label,
        ) = cx.read_entity(&app, |app, _| {
            let projects = app.t("dock.projects");
            let tasks = app.t("dock.tasks");
            (
                app.tf("dock.collapse", &[("name", &projects)]),
                app.tf("dock.expand", &[("name", &projects)]),
                app.tf("dock.collapse", &[("name", &tasks)]),
                app.tf("dock.expand", &[("name", &tasks)]),
                app.t("dock.new_task"),
            )
        });
        assert!(cx.debug_bounds("project-row").is_some());
        let tasks_open = button_count(cx, &task_label);
        assert!(
            tasks_open > 1,
            "a loose task should render under the tasks heading"
        );

        click(cx, &collapse_projects);
        cx.read_entity(&app, |app, _| {
            assert!(!app.projects_open);
            assert!(app.tasks_open);
        });
        assert!(cx.debug_bounds("project-row").is_none());
        assert_eq!(button_count(cx, &task_label), tasks_open);

        click(cx, &expand_projects);
        cx.read_entity(&app, |app, _| assert!(app.projects_open));
        assert!(cx.debug_bounds("project-row").is_some());

        click(cx, &collapse_tasks);
        cx.read_entity(&app, |app, _| {
            assert!(app.projects_open);
            assert!(!app.tasks_open);
        });
        assert!(cx.debug_bounds("project-row").is_some());
        assert!(
            button_count(cx, &task_label) < tasks_open,
            "collapsing tasks hides the loose task row"
        );

        click(cx, &expand_tasks);
        cx.read_entity(&app, |app, _| assert!(app.tasks_open));
        assert_eq!(button_count(cx, &task_label), tasks_open);

        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn project_create_dialog_exposes_fields_and_cancel_keeps_registry_empty(
        cx: &mut TestAppContext,
    ) {
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..4 {
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
            }
            cx.run_until_parked();
        }

        fn click(cx: &mut VisualTestContext, label: &str) {
            let bounds = cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .unwrap_or_else(|| panic!("missing button {label:?}"))
                    .bounds()
            });
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            draw(cx);
        }

        let root = test_root();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                false,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.ota.checked = true;
            app
        });
        let (new_project, name, choose_folder, create, cancel) = cx
            .read_entity(&app, |app, _| {
                (
                    app.t("dock.new_project"),
                    app.t("project.name"),
                    app.t("project.folder"),
                    app.t("project.create"),
                    app.t("more.cancel"),
                )
            });
        draw(cx);
        let header = cx
            .debug_bounds("project-group-header")
            .expect("project group header bounds");
        cx.simulate_mouse_move(
            header.center(),
            None,
            gpui::Modifiers::default(),
        );
        draw(cx);
        click(cx, &new_project);
        cx.read_entity(&app, |app, _| {
            assert_eq!(app.active_dialog, state::ActiveDialog::ProjectCreate);
        });
        cx.update(|window, _| {
            // The retired fork exposed the text editor as an `accesskit::Role::TextInput`
            // node. The gpui-kit `Input` behind `theme::TextField` registers the
            // field frame with its accessible name but no role, and no
            // `TextInput` role exists anywhere in the tree, so the role half of
            // the original assertion is no longer expressible. Assert the
            // labelled, laid-out field instead.
            assert!(
                window.find_all_by_label(&name).iter().any(|node| {
                    node.visible() && node.bounds().size.width > px(0.)
                }),
                "missing project dialog name field"
            );
            for label in [&choose_folder, &create, &cancel] {
                assert!(
                    window.find_all_by_label(label).iter().any(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    }),
                    "missing project dialog control {label:?}"
                );
            }
        });
        click(cx, &cancel);
        cx.read_entity(&app, |app, cx| {
            assert_eq!(app.active_dialog, state::ActiveDialog::None);
            assert_eq!(app.studio.read(cx).projects, []);
        });
        assert!(!root.join("projects.json").exists());
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn project_row_menu_pins_and_unregisters_without_deleting_folder(
        cx: &mut TestAppContext,
    ) {
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..4 {
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
            }
            cx.run_until_parked();
        }

        fn click(cx: &mut VisualTestContext, label: &str) {
            let bounds = cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .unwrap_or_else(|| panic!("missing button {label:?}"))
                    .bounds()
            });
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            draw(cx);
        }

        let root = test_root();
        let folder = root.join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                false,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.ota.checked = true;
            app
        });
        let project = app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                panel.create_project("Client", &folder, cx).unwrap()
            })
        });
        let _project_session = cx.read_entity(&app, |app, cx| {
            app.studio.read(cx).agent.create_session().unwrap()
        });
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.expanded_projects.insert(project.id.clone());
                cx.notify();
            });
        });
        draw(cx);
        fn hover_row(cx: &mut VisualTestContext) {
            let center = cx.update(|window, _| {
                window
                    .find_all_by_label("Client")
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .expect("project name")
                    .bounds()
                    .center()
            });
            cx.simulate_mouse_move(center, None, gpui::Modifiers::default());
            draw(cx);
        }
        hover_row(cx);

        let (options, pin, expand, new_task) =
            cx.read_entity(&app, |app, _| {
                (
                    app.t("project.menu"),
                    app.t("project.pin"),
                    app.tf("project.collapse", &[("name", "Client")]),
                    app.t("project.new_task"),
                )
            });
        for label in [&options, &expand, &new_task] {
            cx.update(|window, _| {
                assert!(
                    window.find_all_by_label(label).iter().any(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    }),
                    "missing project row control {label:?}"
                );
            });
        }
        let nested_task = cx.read_entity(&app, |app, _| app.t("dock.new_task"));
        cx.update(|window, _| {
            assert!(
                window.find_all_by_label(&nested_task).iter().any(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                }),
                "the project's session should render inside its expanded group"
            );
        });

        click(cx, &options);
        cx.update(|window, _| {
            assert!(window.find_all_by_label(&pin).iter().any(|node| {
                node.role() == Some(accesskit::Role::Button) && node.visible()
            }));
        });
        click(cx, &pin);
        cx.read_entity(&app, |app, cx| {
            assert!(app.studio.read(cx).projects[0].pinned);
        });

        hover_row(cx);
        click(cx, &options);
        let delete = cx.read_entity(&app, |app, _| app.t("project.delete"));
        click(cx, &delete);
        cx.read_entity(&app, |app, _| {
            assert_eq!(app.active_dialog, state::ActiveDialog::ProjectDelete);
        });
        let cancel = cx.read_entity(&app, |app, _| app.t("more.cancel"));
        click(cx, &cancel);
        hover_row(cx);
        click(cx, &options);
        click(cx, &delete);
        click(cx, &delete);
        cx.read_entity(&app, |app, cx| {
            assert_eq!(app.studio.read(cx).projects, []);
        });
        assert!(folder.is_dir(), "project removal must preserve local files");
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn project_new_task_click_adds_a_blank_task(cx: &mut TestAppContext) {
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..4 {
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
            }
            cx.run_until_parked();
        }

        let root = test_root();
        let folder = root.join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                false,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.ota.checked = true;
            app
        });
        let project_id = app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                panel
                    .create_project("Client", &folder, cx)
                    .expect("project")
                    .id
            })
        });
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.expanded_projects.insert(project_id.clone());
                cx.notify();
            });
        });
        draw(cx);

        let name = cx.update(|window, _| {
            window
                .find_all_by_label("Client")
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .expect("project name")
                .bounds()
                .center()
        });
        let modifiers = gpui::Modifiers::default();
        cx.simulate_mouse_move(name, None, modifiers);
        draw(cx);

        let label = cx.read_entity(&app, |app, _| app.t("project.new_task"));
        let button = cx.update(|window, _| {
            window
                .find_all_by_label(&label)
                .into_iter()
                .filter(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .map(|node| node.bounds())
                .min_by(|left, right| {
                    left.size
                        .width
                        .partial_cmp(&right.size.width)
                        .expect("button width")
                })
                .expect("project new task button")
                .center()
        });
        cx.simulate_mouse_move(button, None, modifiers);
        draw(cx);
        cx.simulate_mouse_down(button, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_up(button, gpui::MouseButton::Left, modifiers);
        draw(cx);

        cx.read_entity(&app, |app, cx| {
            let panel = app.studio.read(cx);
            let sessions = panel.agent.list_sessions();
            assert_eq!(
                sessions.len(),
                0,
                "clicking project + must defer Router creation until first send (0 empty sessions)"
            );
            assert!(panel.agent.active_session_id().is_none());
            assert_eq!(
                panel.active_project_id.as_deref(),
                Some(project_id.as_str())
            );
            assert_eq!(
                panel
                    .agent
                    .active_workspace()
                    .map(|ws| ws.project_id),
                Some(project_id.clone())
            );
            assert_eq!(panel.surface, StudioSurface::Home);
            assert_eq!(app.section, Section::Studio);
            assert!(app.expanded_projects.contains(&project_id));
        });
        // Clicking project `+` a second time still creates zero empty sessions.
        cx.simulate_mouse_down(button, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_up(button, gpui::MouseButton::Left, modifiers);
        draw(cx);
        cx.read_entity(&app, |app, cx| {
            assert_eq!(
                app.studio.read(cx).agent.list_sessions().len(),
                0,
                "repeated project + clicks must never leave empty sessions"
            );
        });
        let focused = cx.update(|window, cx| {
            let handle = app
                .read(cx)
                .studio
                .read(cx)
                .prompt
                .read(cx)
                .focus_handle(cx);
            handle.is_focused(window)
        });
        assert!(focused, "the composer should take focus");
        // Once a task actually starts in the project, its sidebar row aligns
        // cleanly with the dock navigation row's left edge.
        app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                let project = panel
                    .projects
                    .iter()
                    .find(|p| p.id == project_id)
                    .cloned()
                    .expect("created project exists");
                let sid = panel
                    .agent
                    .create_router_project_session(
                        flowy_agent::AgentWorkspace {
                            project_id: project.id,
                            root: project.root,
                        },
                    )
                    .expect("create project router task");
                panel.open_session(&sid, cx);
            });
        });
        draw(cx);
        let task_label = cx.read_entity(&app, |app, _| app.t("dock.new_task"));
        let (nav_left, task_left) = cx.update(|window, _| {
            let mut rows: Vec<_> = window
                .find_all_by_label(&task_label)
                .into_iter()
                .filter(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .map(|node| node.bounds())
                .collect();
            rows.sort_by(|left, right| {
                left.origin.y.partial_cmp(&right.origin.y).expect("row y")
            });
            assert!(
                rows.len() >= 2,
                "the dock and the project should each show the new task"
            );
            (rows[0].left(), rows[rows.len() - 1].left())
        });
        assert!(
            (nav_left - task_left).abs() < gpui::px(1.),
            "project task highlight should share the dock row's left edge"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn project_name_and_icon_toggle_expansion(cx: &mut TestAppContext) {
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..4 {
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
            }
            cx.run_until_parked();
        }

        fn click(cx: &mut VisualTestContext, label: &str) {
            let bounds = cx.update(|window, _| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .unwrap_or_else(|| panic!("missing button {label:?}"))
                    .bounds()
            });
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            draw(cx);
        }

        let root = test_root();
        let folder = root.join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                false,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.ota.checked = true;
            app
        });
        let project_id = app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                panel
                    .create_project("Client", &folder, cx)
                    .expect("project")
                    .id
            })
        });
        draw(cx);

        let name = "Client";
        let expanded = |cx: &mut VisualTestContext| {
            cx.read_entity(&app, |app, _| {
                app.expanded_projects.contains(&project_id)
            })
        };
        assert!(!expanded(cx), "a new project starts collapsed");

        click(cx, name);
        assert!(expanded(cx), "the project name expands its tasks");
        click(cx, name);
        assert!(!expanded(cx), "the project name collapses its tasks");

        let expand = cx.read_entity(&app, |app, _| {
            app.tf("project.expand", &[("name", name)])
        });
        click(cx, &expand);
        assert!(expanded(cx), "the folder icon expands its tasks");
        let collapse = cx.read_entity(&app, |app, _| {
            app.tf("project.collapse", &[("name", name)])
        });
        click(cx, &collapse);
        assert!(!expanded(cx), "the folder icon collapses its tasks");

        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn unavailable_or_locked_tiers_never_change_the_selected_route(
        cx: &mut TestAppContext,
    ) {
        let root = test_root();
        let app = cx
            .add_window_view(|window, cx| {
                FlowyApp::new(
                    SessionStore::new(root.clone()),
                    false,
                    window,
                    cx,
                )
            })
            .0;
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                // Inject the session after construction so the startup refresh
                // never opens a network request in tests.
                app.auth.session = Some(flowy_auth::Session {
                    access_token: "test-token".into(),
                    email: "test@example.com".into(),
                    display_name: "test".into(),
                    user_id: Some(1),
                });
                app.prefs.model = flowy_agent::Model::Lite;
                app.effective_tier = 0;

                // The mapped id is missing: unavailable must not move the
                // selection or pretend the route was applied.
                app.select_tier(flowy_agent::Model::Standard, cx);
                assert_eq!(app.prefs.model, flowy_agent::Model::Lite);
                assert_eq!(app.notice, Some("notice.model_unavailable"));
                assert_eq!(app.applied_tier, None);

                app.studio.update(cx, |panel, _| {
                    panel.catalog.replace(vec![flowy_agent::CatalogModel {
                        id: "AIPC-deepseek-v4.1-flash".into(),
                        name: "deepseek".into(),
                        endpoint: "https://example.com/claw/v1".into(),
                        ..flowy_agent::CatalogModel::default()
                    }]);
                });
                // Present but gated: a Free account must stay on its route.
                app.select_tier(flowy_agent::Model::Max, cx);
                assert_eq!(app.prefs.model, flowy_agent::Model::Lite);
                assert_eq!(app.notice, Some("notice.upgrade_later"));
                assert_eq!(app.applied_tier, None);
            });
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn new_task_reconciles_deferred_studio_notice(cx: &mut TestAppContext) {
        let root = test_root();
        let app = cx
            .add_window_view(|window, cx| {
                FlowyApp::new(
                    SessionStore::new(root.clone()),
                    false,
                    window,
                    cx,
                )
            })
            .0;
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
        cx.run_until_parked();
        cx.update(|cx| {
            let app = app.read(cx);
            assert_eq!(app.notice, None);
            assert_eq!(app.studio.read(cx).notice, None);
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn storage_error_survives_login_settings_and_navigation_until_dismissed(
        cx: &mut TestAppContext,
    ) {
        let root = test_root();
        let app = cx
            .add_window_view(|window, cx| {
                FlowyApp::new(
                    SessionStore::new(root.clone()),
                    false,
                    window,
                    cx,
                )
            })
            .0;
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
        let app = cx
            .add_window_view(|window, cx| {
                FlowyApp::new(
                    SessionStore::new(root.clone()),
                    false,
                    window,
                    cx,
                )
            })
            .0;
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

    #[gpui::test]
    fn quick_search_opens_modal_without_polluting_nav_history_and_matches_sessions(
        cx: &mut TestAppContext,
    ) {
        let root = test_root();
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("history.json"),
            br#"{
                "active": "s-1",
                "sessions": [
                    {
                        "id": "s-1",
                        "title": "Q3 Revenue Deck",
                        "titled": true,
                        "updated_at": 1700000001000,
                        "status": { "kind": "idle" },
                        "messages": [
                            {
                                "id": "m-1",
                                "role": "user",
                                "content": [
                                    { "type": "text", "text": "Prepare **margin** waterfall for Board" }
                                ],
                                "source": { "kind": "user" }
                            }
                        ],
                        "todos": []
                    },
                    {
                        "id": "s-2",
                        "title": "Weekend Trip",
                        "titled": true,
                        "updated_at": 1700000002000,
                        "status": { "kind": "idle" },
                        "messages": [],
                        "todos": []
                    }
                ]
            }"#,
        )
        .unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        });
        cx.run_until_parked();

        // Opening Quick Search must NOT push a placeholder section onto nav_back.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.screen = Screen::Main;
                let prev_section = app.section;
                app.open_search(window, cx);
                assert_eq!(app.active_dialog, state::ActiveDialog::QuickSearch);
                assert_eq!(app.section, prev_section);
                assert_eq!(app.nav_back, Vec::new());
                let query = app.quick_search.query.clone();
                app.set_input_text(&query, "waterfall", cx);
            });
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        cx.update(|_window, cx| {
            app.update(cx, |app, cx| {
                let rows = app.quick_search_results(cx);
                assert!(
                    rows.iter().any(|row| row.title == "Q3 Revenue Deck"),
                    "message body search should match Q3 Revenue Deck: {rows:?}"
                );
                assert!(
                    rows.iter().all(|row| row.title != "Weekend Trip"),
                    "unrelated session should be filtered out"
                );
                app.confirm_quick_search(cx);
                assert_eq!(app.active_dialog, state::ActiveDialog::None);
            });
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn session_context_menu_supports_point_anchor_pin_rename_and_archive(
        cx: &mut TestAppContext,
    ) {
        let root = test_root();
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("history.json"),
            br#"{
                "active": "s-ctx",
                "sessions": [
                    {
                        "id": "s-ctx",
                        "title": "Draft Plan",
                        "titled": true,
                        "updated_at": 1700000001000,
                        "status": { "kind": "idle" },
                        "messages": [],
                        "todos": []
                    }
                ]
            }"#,
        )
        .unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.screen = Screen::Main;
                // Titlebar dock toggle flips visibility cleanly.
                assert!(app.dock_visible);
                app.toggle_dock(cx);
                assert!(!app.dock_visible);
                app.toggle_dock(cx);
                assert!(app.dock_visible);

                // Point-anchored right-click menu sets sticky anchor + state.
                app.open_task_menu_at(
                    "s-ctx",
                    gpui::point(px(180.), px(240.)),
                    cx,
                );
                let menu = app.task_menu.as_ref().expect("task menu open");
                assert_eq!(menu.session_id, "s-ctx");
                assert!(
                    app.ui
                        .overlays
                        .read(cx)
                        .anchor_bounds(&menu.anchor_id)
                        .is_some()
                );

                // Pin, rename, and archive persist through AgentHandle.
                app.toggle_session_pinned("s-ctx", true, cx);
                let list = app.studio.read(cx).agent.list_sessions();
                assert!(list.iter().find(|s| s.id == "s-ctx").unwrap().pinned);

                app.open_conversation_rename("s-ctx", cx);
                assert_eq!(
                    app.active_dialog,
                    state::ActiveDialog::ConversationRename
                );
                let input = app.conversation_rename.input.clone();
                app.set_input_text(&input, "Final Launch Plan", cx);
            });
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        cx.update(|_window, cx| {
            app.update(cx, |app, cx| {
                app.confirm_conversation_rename(cx);
                assert_eq!(app.active_dialog, state::ActiveDialog::None);
                let list = app.studio.read(cx).agent.list_sessions();
                assert_eq!(
                    list.iter().find(|s| s.id == "s-ctx").unwrap().title,
                    "Final Launch Plan"
                );

                app.toggle_session_archived("s-ctx", true, cx);
                let list = app.studio.read(cx).agent.list_sessions();
                assert!(
                    list.iter().find(|s| s.id == "s-ctx").unwrap().archived
                );
            });
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}

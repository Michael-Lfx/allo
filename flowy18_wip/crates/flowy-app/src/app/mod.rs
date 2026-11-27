#![allow(clippy::derive_partial_eq_without_eq)]

mod appearance;
mod artifact_cleanup;
mod artifact_preview;
mod bootstrap;
mod cloud_sync;
mod automation;
mod conversation_actions;
mod dock;
mod flex;
mod gallery;
mod invite;
mod library_actions;
mod login;
mod login_flow;
mod more;
mod ota;
mod projects;
mod quick_search;
mod settings;
mod skills;
mod skills_jobs;
mod state;
mod titlebar;

use crate::i18n::I18n;
use crate::prefs::Preferences;
use crate::session::SessionStore;
pub use bootstrap::{default_store_root, open_main_window, run};
use flowy_studio::library::{LibraryEvent, LibraryPanel};
use flowy_studio::{StudioEvent, StudioPanel, StudioSurface};
use gpui::{
    Context, Entity, SharedString, Subscription, Task, Window,
    WindowAppearance, actions, div, prelude::*, px,
};
use gpui_kit::base::input::{
    InputBaseState, InputEvent, InputModeKind, InputState, TextareaState,
};
use state::{
    AppearanceChrome, AuthState, FlexState, GalleryState, InviteState,
    LoginPhase, LoginStep, PersonaState, SettingsState, SkillsState,
};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
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
    Skills,
    MySkills,
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
    automations: automation::AutomationState,
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
            panel.close_board_drawer(cx);
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
            "skill-script-confirm"
                if self.active_dialog
                    == state::ActiveDialog::SkillScriptConfirm =>
            {
                self.skills.script_confirm = None;
                self.active_dialog = state::ActiveDialog::None;
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
        let mine_skills_query = input_field(window, cx);
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
            cx.observe(&studio, |app, _, cx| {
                app.sync_section_from_studio(cx);
                cx.notify();
            }),
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
            cx.subscribe(&mine_skills_query, |_, _, _: &InputEvent, cx| {
                cx.notify();
            }),
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
                mine_query: mine_skills_query,
                mine_tab: state::MySkillsTab::default(),
                mine_selected: None,
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
                pending_open: false,
                emails: invite_book.emails,
                email: invite_email,
            },
            project_create,
            project_menu: None,
            expanded_projects,
            projects_open: true,
            tasks_open: true,
            persona: PersonaState {
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
            automations: automation::AutomationState::default(),
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
        app.start_automation(cx);
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
                    panel.close_board_drawer(cx);
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
                    panel.close_board_drawer(cx);
                    panel.set_surface(StudioSurface::Automation, cx);
                    panel.set_notice(notice, cx);
                });
                self.refresh_automation(cx);
            }
            Section::Library => {
                self.studio.update(cx, |panel, cx| {
                    panel.close_all_popovers();
                    panel.close_board_drawer(cx);
                    panel.set_notice(notice, cx);
                });
            }
            Section::Create => {
                self.studio.update(cx, |panel, cx| {
                    panel.close_all_popovers();
                    panel.close_board_drawer(cx);
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
            Section::More | Section::Skills | Section::MySkills => {
                self.studio.update(cx, |panel, cx| {
                    panel.close_all_popovers();
                    panel.close_board_drawer(cx);
                    panel.set_notice(notice, cx);
                });
            }
        }
    }

    /// Reconciles `FlowyApp.section` when `StudioPanel` leaves
    /// `StudioSurface::Board` (e.g. Board "Open chat" or "New task"), keeping
    /// sidebar selection and Back/Forward history synchronized.
    pub(super) fn sync_section_from_studio(
        &mut self,
        cx: &Context<'_, Self>,
    ) -> bool {
        if self.section == Section::Board
            && self.studio.read(cx).surface != StudioSurface::Board
        {
            self.nav_back.push(Section::Board);
            self.nav_forward.clear();
            self.section = Section::Studio;
            return true;
        }
        false
    }

    fn on_studio_event(
        &mut self,
        event: &StudioEvent,
        cx: &mut Context<'_, Self>,
    ) {
        let section_synced = self.sync_section_from_studio(cx);
        match event {
            StudioEvent::Automation(action) => {
                self.handle_automation_action(action, cx);
            }
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
            StudioEvent::PreviewLocalFile(path) => {
                self.open_local_file_preview(path, cx);
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
                self.load_library(cx);
                cx.notify();
            }
            StudioEvent::SkillsChanged => {
                self.skills.installed = self.studio.read(cx).skill_catalog();
                self.sync_studio_skills(cx);
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
        if section_synced {
            cx.notify();
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
        } else if self.section == Section::Skills {
            canvas.child(self.skills_canvas(window, cx))
        } else if self.section == Section::MySkills {
            canvas.child(self.my_skills_canvas(window, cx))
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
                    && self.studio.read(cx).has_board_drawer())
                .then(|| gpui::SharedString::from("board-inspector-drawer"))
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
                    && !matches!(
                        self.section,
                        Section::More | Section::Skills | Section::MySkills
                    )
                    && self.section != Section::Library
                    && !self.studio.read(cx).has_board_drawer()
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
            state::ActiveDialog::SkillScriptConfirm => {
                root.child(self.skill_script_confirm_layer(window, cx))
            }
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

#[cfg(test)]
use cloud_sync::{catalog_refresh_allowed, credits_state_from_balance};

#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod tests;

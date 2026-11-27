//! Desktop entry points, main window lifecycle, background file/shell watchers,
//! and OS task notifications.

use std::sync::OnceLock;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, Context, KeyBinding, WindowAppearance,
    WindowBounds, WindowOptions, px, size,
};
use gpui_kit::base::input::Enter;

use super::{
    FlowyApp, FocusNext, FocusPrevious, InsertIndent, NavigateBack,
    NavigateForward, OpenSearch, SignIn, ToggleDock,
};
use crate::i18n::{I18n, file_stamps};
use crate::session::SessionStore;

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

impl FlowyApp {
    pub(super) fn start_locale_watch(&mut self, cx: &Context<'_, Self>) {
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

    pub(super) fn maybe_notify_turn_finished(&self, cx: &Context<'_, Self>) {
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

    pub(super) fn start_shell_watch(&mut self, cx: &Context<'_, Self>) {
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
                            Some(flowy_shell::DeepLink::AutomationRun { run_id }) => {
                                if let Ok(run_id) = flowy_automation::RunId::parse(&run_id) {
                                    app.open_automation_run(run_id, cx);
                                }
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
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(window);
}

fn ensure_shell_main_window(cx: &mut App) {
    let existing = *SHELL_MAIN
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            Some(flowy_shell::DeepLink::AutomationRun { run_id }) => {
                if let Ok(run_id) = flowy_automation::RunId::parse(&run_id) {
                    app.open_automation_run(run_id, cx);
                }
            }
            Some(flowy_shell::DeepLink::Open) | None => {}
        }
    }
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

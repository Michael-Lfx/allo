use super::{FlowyApp, Screen, Section};
use flowy_studio::StudioSurface;
use flowy_ui as theme;
use gpui::{Context, Window, WindowControlArea, div, prelude::*, px};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PushSectionAction {
    NoOp,
    Transition { back_entry: Section },
}

pub(super) fn decide_push_section(
    current_section: Section,
    target_section: Section,
    studio_surface: StudioSurface,
) -> PushSectionAction {
    let board_desynced = current_section == Section::Board
        && studio_surface != StudioSurface::Board;
    if current_section == target_section {
        if board_desynced {
            return PushSectionAction::Transition {
                back_entry: Section::Studio,
            };
        }
        return PushSectionAction::NoOp;
    }
    let back_entry = if board_desynced && target_section != Section::Studio {
        Section::Studio
    } else {
        current_section
    };
    PushSectionAction::Transition { back_entry }
}

pub(super) const HEIGHT: f32 = theme::TITLEBAR_HEIGHT;

#[cfg(target_os = "macos")]
const LEFT_INSET: f32 = 78.0;
#[cfg(not(target_os = "macos"))]
const LEFT_INSET: f32 = 8.0;

impl FlowyApp {
    pub(super) fn titlebar(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let maximized = window.is_maximized();
        let main = self.screen == Screen::Main;
        let board_desynced = self.section == Section::Board
            && self.studio.read(cx).surface != StudioSurface::Board;
        let can_back = main && (!self.nav_back.is_empty() || board_desynced);
        let can_forward = main && !self.nav_forward.is_empty();
        div()
            .id("titlebar")
            .w_full()
            .h(px(HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .bg(self.theme.canvas())
            .border_b_1()
            .border_color(self.theme.line())
            .child(self.titlebar_left(main, can_back, can_forward, cx))
            .child(
                // Restrict the OS caption drag region strictly to the empty
                // middle spacer so Windows WM_NCHITTEST never swallows clicks
                // meant for the left toolbar buttons.
                div()
                    .id("titlebar-drag-spacer")
                    .flex_1()
                    .min_w(px(16.))
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(self.titlebar_right(maximized, cx))
    }

    fn titlebar_left(
        &self,
        main: bool,
        can_back: bool,
        can_forward: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .occlude()
            .pl(px(LEFT_INSET))
            .pr_2()
            .h_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_start()
            .gap_1()
            .child(self.titlebar_tool(
                "titlebar-search",
                "titlebar.search",
                theme::icons::search().size(px(16.)),
                main,
                cx.listener(|app, _, window, cx| {
                    app.open_search(window, cx);
                }),
            ))
            .child(self.titlebar_tool(
                "titlebar-toggle-dock",
                "titlebar.toggle_dock",
                theme::icons::panel_left().size(px(16.)),
                main,
                cx.listener(|app, _, _, cx| app.toggle_dock(cx)),
            ))
            .child(self.titlebar_tool(
                "titlebar-back",
                "titlebar.back",
                theme::icons::arrow_left().size(px(16.)),
                can_back,
                cx.listener(|app, _, _, cx| app.navigate_back(cx)),
            ))
            .child(self.titlebar_tool(
                "titlebar-forward",
                "titlebar.forward",
                theme::icons::arrow_right().size(px(16.)),
                can_forward,
                cx.listener(|app, _, _, cx| app.navigate_forward(cx)),
            ))
    }

    fn titlebar_right(
        &self,
        maximized: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let zoom_key = if maximized {
            "titlebar.restore"
        } else {
            "titlebar.maximize"
        };
        let zoom_icon = if maximized {
            theme::icons::copy().size(px(14.))
        } else {
            theme::icons::maximize().size(px(14.))
        };
        div()
            .pr_2()
            .h_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_end()
            .gap_1()
            .child(self.titlebar_window_btn(
                "titlebar-minimize",
                "titlebar.minimize",
                theme::icons::minus().size(px(14.)),
                WindowControlArea::Min,
                cx.listener(|_app, _, window, _cx| {
                    window.minimize_window();
                }),
            ))
            .child(self.titlebar_window_btn(
                "titlebar-maximize",
                zoom_key,
                zoom_icon,
                WindowControlArea::Max,
                cx.listener(|_app, _, window, _cx| {
                    window.zoom_window();
                }),
            ))
            .child(self.titlebar_window_btn(
                "titlebar-close",
                "titlebar.close",
                theme::icons::x().size(px(14.)),
                WindowControlArea::Close,
                cx.listener(|_app, _, window, cx| {
                    // gpui-pre has no `Window::set_visible`. Without the tray a
                    // hidden window could never come back, so closing follows
                    // the same rule as `on_window_should_close`.
                    if flowy_shell::tray_available() {
                        window.minimize_window();
                    } else {
                        cx.quit();
                    }
                }),
            ))
    }

    fn titlebar_tool(
        &self,
        id: &'static str,
        label_key: &'static str,
        icon: theme::icons::Icon,
        enabled: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        theme::IconButton::new(id, self.t(label_key), icon)
            .size(theme::ControlSize::Small)
            .disabled(!enabled)
            .render(&self.theme, on_click)
    }

    fn titlebar_window_btn(
        &self,
        id: &'static str,
        label_key: &'static str,
        icon: theme::icons::Icon,
        area: WindowControlArea,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        // `IconButton::render` erases to `AnyElement`, which no longer exposes
        // the interactive surface, so the window-control hitbox lives on a
        // wrapper. `occlude` keeps the drag region painted behind these
        // buttons from swallowing the click: otherwise Windows treats it as a
        // caption drag and minimize never runs.
        div().occlude().window_control_area(area).child(
            theme::IconButton::new(id, self.t(label_key), icon)
                .size(theme::ControlSize::Small)
                .render(&self.theme, on_click),
        )
    }

    pub(super) fn toggle_dock(&mut self, cx: &mut Context<'_, Self>) {
        if self.screen != Screen::Main {
            return;
        }
        self.dock_visible = !self.dock_visible;
        let visible = self.dock_visible;
        self.studio
            .update(cx, |panel, cx| panel.set_dock_visible(visible, cx));
        cx.notify();
    }

    pub(super) fn open_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.screen != Screen::Main {
            return;
        }
        self.open_quick_search(window, cx);
    }

    pub(super) fn navigate_back(&mut self, cx: &mut Context<'_, Self>) {
        self.sync_section_from_studio(cx);
        let Some(previous) = self.nav_back.pop() else {
            return;
        };
        self.nav_forward.push(self.section);
        self.apply_section(previous, cx);
    }

    pub(super) fn navigate_forward(&mut self, cx: &mut Context<'_, Self>) {
        let Some(next) = self.nav_forward.pop() else {
            return;
        };
        let back_entry = if self.section == Section::Board
            && self.studio.read(cx).surface != StudioSurface::Board
        {
            Section::Studio
        } else {
            self.section
        };
        self.nav_back.push(back_entry);
        self.apply_section(next, cx);
    }

    pub(super) fn push_section(
        &mut self,
        section: Section,
        cx: &mut Context<'_, Self>,
    ) {
        let studio_surface = self.studio.read(cx).surface;
        let PushSectionAction::Transition { back_entry } =
            decide_push_section(self.section, section, studio_surface)
        else {
            return;
        };
        self.nav_back.push(back_entry);
        self.nav_forward.clear();
        self.apply_section(section, cx);
    }

    fn apply_section(&mut self, section: Section, cx: &mut Context<'_, Self>) {
        self.section = section;
        self.user_menu = false;
        self.task_menu = None;
        self.project_menu = None;
        if section == Section::Studio {
            self.notice = None;
        }
        self.sync_studio_surface(cx);
        cx.notify();
        if section == Section::More
            && self.persona.page == super::more::Page::Skills
        {
            self.refresh_skills(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionStore;
    use gpui::TestAppContext;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_store_root() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "flowy-app-titlebar-test-{}-{}",
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
    fn push_section_decision_handles_noop_and_board_chat_reentry() {
        assert_eq!(
            decide_push_section(
                Section::Studio,
                Section::Studio,
                StudioSurface::Home,
            ),
            PushSectionAction::NoOp
        );
        assert_eq!(
            decide_push_section(
                Section::Studio,
                Section::Studio,
                StudioSurface::Chat,
            ),
            PushSectionAction::NoOp
        );
        assert_eq!(
            decide_push_section(
                Section::Board,
                Section::Board,
                StudioSurface::Board,
            ),
            PushSectionAction::NoOp
        );
        assert_eq!(
            decide_push_section(
                Section::Library,
                Section::Library,
                StudioSurface::Board,
            ),
            PushSectionAction::NoOp
        );
        assert_eq!(
            decide_push_section(
                Section::Board,
                Section::Board,
                StudioSurface::Chat,
            ),
            PushSectionAction::Transition {
                back_entry: Section::Studio,
            }
        );
        assert_eq!(
            decide_push_section(
                Section::Board,
                Section::Board,
                StudioSurface::Home,
            ),
            PushSectionAction::Transition {
                back_entry: Section::Studio,
            }
        );
        assert_eq!(
            decide_push_section(
                Section::Board,
                Section::Studio,
                StudioSurface::Chat,
            ),
            PushSectionAction::Transition {
                back_entry: Section::Board,
            }
        );
        assert_eq!(
            decide_push_section(
                Section::Board,
                Section::Library,
                StudioSurface::Chat,
            ),
            PushSectionAction::Transition {
                back_entry: Section::Studio,
            }
        );
        assert_eq!(
            decide_push_section(
                Section::Studio,
                Section::Board,
                StudioSurface::Chat,
            ),
            PushSectionAction::Transition {
                back_entry: Section::Studio,
            }
        );
    }

    #[gpui::test]
    fn board_chat_navigation_syncs_section_and_allows_reentry_and_back(
        cx: &mut TestAppContext,
    ) {
        let root = temp_store_root();
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("history.json"),
            br#"{
                "active": "s-board-nav",
                "sessions": [
                    {
                        "id": "s-board-nav",
                        "title": "Board Jump Session",
                        "titled": true,
                        "updated_at": 1700000001000,
                        "status": { "kind": "idle" },
                        "messages": [
                            {
                                "id": "m-1",
                                "role": "user",
                                "content": [
                                    { "type": "text", "text": "Prepare Q4 summary" }
                                ],
                                "source": { "kind": "user" }
                            }
                        ],
                        "todos": []
                    }
                ]
            }"#,
        )
        .unwrap();

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
        cx.run_until_parked();

        // 1. Navigate to Board from Home.
        cx.update(|_window, cx| {
            app.update(cx, |app, cx| {
                app.select_navigation("board", Section::Board, cx);
                assert_eq!(app.section, Section::Board);
                assert_eq!(app.studio.read(cx).surface, StudioSurface::Board);
                assert_eq!(app.nav_back, vec![Section::Studio]);
            });
        });

        // 2. Open a session from Board ("打开对话" flow). Observer /
        // SessionsChanged must sync `app.section` to `Studio` and push
        // `Board` onto `nav_back`.
        cx.update(|_window, cx| {
            app.update(cx, |app, cx| {
                app.studio.update(cx, |panel, cx| {
                    panel.open_session("s-board-nav", cx);
                });
            });
        });
        cx.run_until_parked();
        cx.read_entity(&app, |app, cx| {
            assert_eq!(app.studio.read(cx).surface, StudioSurface::Chat);
            assert_eq!(app.section, Section::Studio);
            assert_eq!(app.nav_back, vec![Section::Studio, Section::Board]);
        });

        // 3. Click sidebar Progress Board again -> returns to Board and
        // pushes Studio onto nav_back.
        cx.update(|_window, cx| {
            app.update(cx, |app, cx| {
                app.select_navigation("board", Section::Board, cx);
                assert_eq!(app.section, Section::Board);
                assert_eq!(app.studio.read(cx).surface, StudioSurface::Board);
                assert_eq!(
                    app.nav_back,
                    vec![Section::Studio, Section::Board, Section::Studio]
                );
            });
        });

        // 4. Back returns to Chat, then Back again returns to Board.
        cx.update(|_window, cx| {
            app.update(cx, |app, cx| {
                app.navigate_back(cx);
                assert_eq!(app.section, Section::Studio);
                assert_eq!(app.studio.read(cx).surface, StudioSurface::Chat);
                app.navigate_back(cx);
                assert_eq!(app.section, Section::Board);
                assert_eq!(app.studio.read(cx).surface, StudioSurface::Board);
            });
        });

        // 5. Simulate desynced re-entry: `app.section == Section::Board` while
        // `studio.surface` moved to `Chat` before observer/event sync ran.
        cx.update(|_window, cx| {
            app.update(cx, |app, cx| {
                app.nav_back = vec![Section::Studio];
                app.nav_forward.clear();
                app.studio.update(cx, |panel, cx| {
                    panel.set_surface(StudioSurface::Chat, cx);
                });
                // Call immediately before GPUI flushes observers: `push_section`
                // must NOT no-op; it must push `Studio` and reopen Board.
                app.select_navigation("board", Section::Board, cx);
                assert_eq!(app.section, Section::Board);
                assert_eq!(app.studio.read(cx).surface, StudioSurface::Board);
                assert_eq!(
                    app.nav_back,
                    vec![Section::Studio, Section::Studio]
                );
                app.navigate_back(cx);
                assert_eq!(app.section, Section::Studio);
                assert_eq!(app.studio.read(cx).surface, StudioSurface::Chat);
            });
        });

        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}

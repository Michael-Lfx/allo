use super::{FlowyApp, Screen, Section};
use flowy_ui as theme;
use gpui::{Context, Window, WindowControlArea, div, prelude::*, px};

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
        let can_back = main && !self.nav_back.is_empty();
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
        self.nav_back.push(self.section);
        self.apply_section(next, cx);
    }

    pub(super) fn push_section(
        &mut self,
        section: Section,
        cx: &mut Context<'_, Self>,
    ) {
        if self.section == section {
            return;
        }
        self.nav_back.push(self.section);
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
